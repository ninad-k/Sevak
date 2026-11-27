//! A workflow's user configuration (Alfred's "User Configuration").
//!
//! The author lists the settings a workflow needs in `[[config]]` tables:
//!
//! ```toml
//! [[config]]
//! variable = "api_host"
//! kind = "textfield"
//! label = "Server"
//! required = true
//!
//! [[config]]
//! variable = "units"
//! kind = "popup"
//! label = "Units"
//! default = "metric"
//! options = [{ label = "Metric", value = "metric" }, { label = "Imperial", value = "imperial" }]
//! ```
//!
//! The user fills them in a form. Their values live **outside** the workflow
//! folder, in `<data dir>/workflows/<folder>/user-config.json`, so updating or
//! reinstalling the workflow never overwrites them, and they are not part of
//! what the user approved (they are the user's own data).
//!
//! At run time a field's value is a variable (`{var:api_host}`, and an
//! environment variable of the script nodes), with this precedence: the
//! user's value, then the field's `default`, then the workflow's `[variables]`.
//! A required field without a value stops the workflow from running.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sevak_core::bounded_read::read_to_string_capped;
use sevak_platform::process::is_reserved_variable;

use super::model::Workflow;
use crate::files::{expand_home, home_dir};

/// Name of the user's values inside the workflow's data folder.
pub const USER_CONFIG_FILE: &str = "user-config.json";
/// Most fields one workflow may declare.
pub const MAX_CONFIG_FIELDS: usize = 50;
/// Longest value of a text field, in bytes.
pub const MAX_CONFIG_VALUE_BYTES: usize = 64 * 1024;
/// The range a slider without `min`/`max` covers.
pub const DEFAULT_SLIDER_MIN: f64 = 0.0;
pub const DEFAULT_SLIDER_MAX: f64 = 100.0;
const MAX_USER_CONFIG_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigKind {
    #[default]
    Textfield,
    Textarea,
    Checkbox,
    Popup,
    Slider,
    Filepicker,
}

impl ConfigKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Textfield => "textfield",
            Self::Textarea => "textarea",
            Self::Checkbox => "checkbox",
            Self::Popup => "popup",
            Self::Slider => "slider",
            Self::Filepicker => "filepicker",
        }
    }
}

/// What a file picker chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickMode {
    #[default]
    File,
    Folder,
}

/// One choice of a popup.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigOption {
    pub label: String,
    pub value: String,
}

/// One setting of the workflow's form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConfigField {
    /// The variable (and environment variable) the value becomes.
    pub variable: String,
    pub kind: ConfigKind,
    /// The form's label; the variable's name when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The value until the user sets one. A checkbox is `true` or `false`, a
    /// slider a number; TOML booleans and numbers are accepted as written.
    #[serde(
        skip_serializing_if = "String::is_empty",
        deserialize_with = "scalar_string"
    )]
    pub default: String,
    /// The workflow does not run until the field has a value.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    /// The choices of a popup.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ConfigOption>,
    /// A slider's range (default 0 to 100) and step (default 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    /// What a file picker chooses.
    #[serde(skip_serializing_if = "is_file_mode")]
    pub mode: PickMode,
    /// Grey text of an empty text field.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
}

fn is_file_mode(mode: &PickMode) -> bool {
    *mode == PickMode::File
}

/// Text, a boolean or a number, all as text.
fn scalar_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::String(text) => Ok(text),
        Value::Bool(flag) => Ok(flag.to_string()),
        Value::Number(number) => Ok(number.to_string()),
        Value::Null => Ok(String::new()),
        _ => Err(serde::de::Error::custom(
            "expected text, a number or true/false",
        )),
    }
}

/// Whether `name` can be a configuration variable: a letter or `_`, then
/// letters, digits and `_` (it is also an environment variable).
pub fn valid_config_variable(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.len() <= 64
}

fn number_text(number: f64) -> String {
    if number.fract() == 0.0 && number.abs() < 1e15 {
        format!("{}", number as i64)
    } else {
        number.to_string()
    }
}

impl ConfigField {
    /// What the form and the "Configure first" message call this field.
    pub fn display_label(&self) -> &str {
        let label = self.label.trim();
        if label.is_empty() {
            &self.variable
        } else {
            label
        }
    }

    pub fn slider_min(&self) -> f64 {
        self.min.unwrap_or(DEFAULT_SLIDER_MIN)
    }

    pub fn slider_max(&self) -> f64 {
        self.max.unwrap_or(DEFAULT_SLIDER_MAX)
    }

    /// `value` as this field stores it, or why it is not acceptable. A
    /// checkbox takes a boolean, a slider a number inside its range, a popup
    /// one of its options' values, the others text.
    pub fn normalize(&self, value: &Value) -> Result<String, String> {
        let label = self.display_label();
        match self.kind {
            ConfigKind::Checkbox => match value {
                Value::Bool(flag) => Ok(flag.to_string()),
                Value::String(text) if text == "true" || text == "false" => Ok(text.clone()),
                _ => Err(format!("\"{label}\" must be on or off")),
            },
            ConfigKind::Slider => {
                let number = match value {
                    Value::Number(number) => number.as_f64(),
                    Value::String(text) => text.trim().parse::<f64>().ok(),
                    _ => None,
                }
                .filter(|number| number.is_finite())
                .ok_or_else(|| format!("\"{label}\" must be a number"))?;
                let (min, max) = (self.slider_min(), self.slider_max());
                if number < min || number > max {
                    return Err(format!(
                        "\"{label}\" must be between {} and {}",
                        number_text(min),
                        number_text(max)
                    ));
                }
                Ok(number_text(number))
            }
            ConfigKind::Popup => {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("\"{label}\" must be one of the choices"))?;
                if self.options.iter().any(|option| option.value == text) {
                    Ok(text.to_owned())
                } else {
                    Err(format!("\"{label}\" must be one of the choices"))
                }
            }
            ConfigKind::Textfield | ConfigKind::Textarea | ConfigKind::Filepicker => {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("\"{label}\" must be text"))?;
                if text.len() > MAX_CONFIG_VALUE_BYTES {
                    return Err(format!("\"{label}\" is too long"));
                }
                if text.contains('\0') {
                    return Err(format!("\"{label}\" contains an invalid character"));
                }
                Ok(text.to_owned())
            }
        }
    }

    /// Problems with how the author declared this field.
    fn declaration_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let name = &self.variable;
        if self.kind == ConfigKind::Popup {
            if self.options.is_empty() {
                problems.push(format!("\"{name}\": a popup needs at least one option"));
            }
            let mut seen = BTreeSet::new();
            for option in &self.options {
                if option.value.is_empty() {
                    problems.push(format!("\"{name}\": an option has no value"));
                } else if !seen.insert(option.value.as_str()) {
                    problems.push(format!(
                        "\"{name}\": the option value \"{}\" is used twice",
                        option.value
                    ));
                }
            }
        }
        if self.kind == ConfigKind::Slider {
            let (min, max) = (self.slider_min(), self.slider_max());
            if !min.is_finite() || !max.is_finite() || min >= max {
                problems.push(format!(
                    "\"{name}\": a slider's min must be less than its max"
                ));
            }
            if self
                .step
                .is_some_and(|step| !step.is_finite() || step <= 0.0)
            {
                problems.push(format!("\"{name}\": a slider's step must be more than 0"));
            }
        }
        if !self.default.is_empty() {
            let checked = match self.kind {
                ConfigKind::Checkbox => Value::Bool(self.default == "true"),
                _ => Value::String(self.default.clone()),
            };
            let valid = self.kind != ConfigKind::Checkbox
                || self.default == "true"
                || self.default == "false";
            if !valid || self.normalize(&checked).is_err() {
                problems.push(format!(
                    "\"{name}\": the default is not a valid value for a {}",
                    self.kind.key()
                ));
            }
        }
        problems
    }
}

/// What is wrong with the declared fields (messages only).
pub fn field_problems(fields: &[ConfigField]) -> Vec<String> {
    let mut problems = Vec::new();
    if fields.len() > MAX_CONFIG_FIELDS {
        problems.push(format!(
            "a workflow may have at most {MAX_CONFIG_FIELDS} configuration fields"
        ));
    }
    let mut seen = BTreeSet::new();
    for field in fields {
        let name = &field.variable;
        if !valid_config_variable(name) {
            problems.push(format!(
                "the configuration variable \"{name}\" must start with a letter or _ and use \
                 only letters, digits and _"
            ));
            continue;
        }
        if is_reserved_variable(name) {
            problems.push(format!(
                "\"{name}\" cannot be a configuration variable: it changes how programs start \
                 or where they look for things"
            ));
        }
        if !seen.insert(name.as_str()) {
            problems.push(format!(
                "the configuration variable \"{name}\" is used twice"
            ));
        }
        problems.extend(field.declaration_problems());
    }
    problems
}

/// Where the user's values of the workflow with data folder `data_dir` live.
pub fn user_config_path(data_dir: &Path) -> PathBuf {
    data_dir.join(USER_CONFIG_FILE)
}

/// The values the user saved (empty when there are none or the file is
/// unreadable: the workflow then asks again).
pub fn load_user_values(data_dir: &Path) -> BTreeMap<String, String> {
    let Ok(text) = read_to_string_capped(&user_config_path(data_dir), MAX_USER_CONFIG_BYTES) else {
        return BTreeMap::new();
    };
    match serde_json::from_str::<BTreeMap<String, Value>>(&text) {
        Ok(map) => map
            .into_iter()
            .filter_map(|(name, value)| {
                let text = match value {
                    Value::String(text) => text,
                    Value::Bool(flag) => flag.to_string(),
                    Value::Number(number) => number.to_string(),
                    _ => return None,
                };
                Some((name, text))
            })
            .collect(),
        Err(err) => {
            tracing::warn!("ignoring an unreadable {USER_CONFIG_FILE}: {err}");
            BTreeMap::new()
        }
    }
}

/// Saves the user's values (atomically, so a crash never leaves half a file).
pub fn save_user_values(data_dir: &Path, values: &BTreeMap<String, String>) -> io::Result<()> {
    fs::create_dir_all(data_dir)?;
    let path = user_config_path(data_dir);
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let text = serde_json::to_string_pretty(values)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let written = fs::write(&temp, text).and_then(|()| fs::rename(&temp, &path));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

impl Workflow {
    /// The field that sets `variable`.
    pub fn config_field(&self, variable: &str) -> Option<&ConfigField> {
        self.config.iter().find(|field| field.variable == variable)
    }

    /// The variables every run starts with: the workflow's `[variables]`, then
    /// each field's non-empty default over them, then the user's values over
    /// both (values for fields that no longer exist are ignored).
    pub fn effective_variables(&self, user: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        let mut variables = self.variables.clone();
        for field in &self.config {
            if let Some(value) = user.get(&field.variable) {
                variables.insert(field.variable.clone(), value.clone());
            } else if !field.default.is_empty() {
                variables.insert(field.variable.clone(), field.default.clone());
            }
        }
        // The picker returns `~/...` for the home folder; scripts get a real path.
        for field in self
            .config
            .iter()
            .filter(|f| f.kind == ConfigKind::Filepicker)
        {
            if let Some(value) = variables.get_mut(&field.variable) {
                if value.starts_with('~') {
                    *value = expand_home(value, home_dir().as_deref())
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        variables
    }

    /// The labels of the required fields that have no value yet. A checkbox
    /// always has one (off).
    pub fn missing_config(&self, user: &BTreeMap<String, String>) -> Vec<String> {
        let variables = self.effective_variables(user);
        self.config
            .iter()
            .filter(|field| field.required && field.kind != ConfigKind::Checkbox)
            .filter(|field| {
                variables
                    .get(&field.variable)
                    .is_none_or(|value| value.trim().is_empty())
            })
            .map(|field| field.display_label().to_owned())
            .collect()
    }

    /// Checks values from the form against the declared fields and returns
    /// them as stored. Unknown variables are refused.
    pub fn check_user_values(
        &self,
        values: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, String>, String> {
        let mut checked = BTreeMap::new();
        for (name, value) in values {
            let field = self
                .config_field(name)
                .ok_or_else(|| format!("this workflow has no setting \"{name}\""))?;
            checked.insert(name.clone(), field.normalize(value)?);
        }
        Ok(checked)
    }
}

/// The message of a workflow that cannot run until `missing` is filled in.
pub fn configure_message(workflow_name: &str, missing: &[String]) -> String {
    format!("Configure {workflow_name} first: {}", missing.join(", "))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn field(variable: &str, kind: ConfigKind) -> ConfigField {
        ConfigField {
            variable: variable.to_owned(),
            kind,
            ..ConfigField::default()
        }
    }

    fn workflow(config: Vec<ConfigField>) -> Workflow {
        Workflow {
            name: "W".to_owned(),
            config,
            ..Workflow::default()
        }
    }

    #[test]
    fn fields_parse_from_toml_with_defaults_and_scalars() {
        let text = r#"
format = 1
name = "W"

[[config]]
variable = "host"
required = true

[[config]]
variable = "units"
kind = "popup"
default = "metric"
options = [{ label = "Metric", value = "metric" }, { label = "Imperial", value = "imperial" }]

[[config]]
variable = "volume"
kind = "slider"
default = 40
min = 0
max = 100
step = 5

[[config]]
variable = "loud"
kind = "checkbox"
default = true

[[config]]
variable = "root"
kind = "filepicker"
mode = "folder"
"#;
        let parsed: Workflow = toml::from_str(text).unwrap();
        assert_eq!(parsed.config.len(), 5);
        assert_eq!(parsed.config[0].kind, ConfigKind::Textfield);
        assert!(parsed.config[0].required);
        assert_eq!(parsed.config[1].options[1].value, "imperial");
        assert_eq!(parsed.config[2].default, "40");
        assert_eq!(parsed.config[2].step, Some(5.0));
        assert_eq!(parsed.config[3].default, "true");
        assert_eq!(parsed.config[4].mode, PickMode::Folder);
        assert!(field_problems(&parsed.config).is_empty());
        // And it round-trips.
        let again: Workflow = toml::from_str(&toml::to_string(&parsed).unwrap()).unwrap();
        assert_eq!(again.config, parsed.config);
    }

    #[test]
    fn a_workflow_without_config_writes_none() {
        let text = toml::to_string(&workflow(Vec::new())).unwrap();
        assert!(!text.contains("config"), "{text}");
    }

    #[test]
    fn declarations_are_checked() {
        let mut popup = field("p", ConfigKind::Popup);
        let mut slider = field("s", ConfigKind::Slider);
        slider.min = Some(5.0);
        slider.max = Some(5.0);
        let mut bad_default = field("b", ConfigKind::Slider);
        bad_default.default = "500".to_owned();
        let fields = vec![
            field("ok", ConfigKind::Textfield),
            field("ok", ConfigKind::Textarea),
            field("1bad", ConfigKind::Textfield),
            field("has space", ConfigKind::Textfield),
            field("PATH", ConfigKind::Textfield),
            popup.clone(),
            slider,
            bad_default,
        ];
        let text = field_problems(&fields).join("\n");
        assert!(text.contains("\"ok\" is used twice"), "{text}");
        assert!(text.contains("\"1bad\""), "{text}");
        assert!(text.contains("\"has space\""), "{text}");
        assert!(text.contains("\"PATH\" cannot be"), "{text}");
        assert!(text.contains("popup needs at least one option"), "{text}");
        assert!(text.contains("min must be less than its max"), "{text}");
        assert!(text.contains("default is not a valid value"), "{text}");

        popup.options = vec![
            ConfigOption {
                label: "A".into(),
                value: "a".into(),
            },
            ConfigOption {
                label: "A2".into(),
                value: "a".into(),
            },
        ];
        assert!(field_problems(&[popup.clone()])[0].contains("used twice"));
        popup.options.pop();
        popup.default = "zzz".to_owned();
        assert!(field_problems(&[popup])[0].contains("default is not a valid value"));
    }

    #[test]
    fn values_are_validated_by_kind() {
        let checkbox = field("c", ConfigKind::Checkbox);
        assert_eq!(checkbox.normalize(&json!(true)).unwrap(), "true");
        assert!(checkbox.normalize(&json!("yes")).is_err());
        assert!(checkbox.normalize(&json!(1)).is_err());

        let mut slider = field("s", ConfigKind::Slider);
        slider.min = Some(1.0);
        slider.max = Some(10.0);
        assert_eq!(slider.normalize(&json!(5)).unwrap(), "5");
        assert_eq!(slider.normalize(&json!(2.5)).unwrap(), "2.5");
        assert!(slider.normalize(&json!(11)).is_err());
        assert!(slider.normalize(&json!(0.5)).is_err());
        assert!(slider.normalize(&json!("abc")).is_err());

        let mut popup = field("p", ConfigKind::Popup);
        popup.options = vec![ConfigOption {
            label: "A".into(),
            value: "a".into(),
        }];
        assert_eq!(popup.normalize(&json!("a")).unwrap(), "a");
        assert!(popup.normalize(&json!("b")).is_err());

        let text = field("t", ConfigKind::Textfield);
        assert_eq!(text.normalize(&json!("hi")).unwrap(), "hi");
        assert!(text.normalize(&json!(5)).is_err());
        assert!(text.normalize(&json!("a\0b")).is_err());
        assert!(text
            .normalize(&json!("x".repeat(MAX_CONFIG_VALUE_BYTES + 1)))
            .is_err());
    }

    #[test]
    fn user_value_beats_default_beats_workflow_variable() {
        let mut host = field("host", ConfigKind::Textfield);
        host.default = "default.example".to_owned();
        let other = field("other", ConfigKind::Textfield);
        let mut w = workflow(vec![host, other]);
        w.variables.insert("host".into(), "variable.example".into());
        w.variables.insert("other".into(), "kept".into());
        w.variables.insert("plain".into(), "p".into());

        let none = w.effective_variables(&BTreeMap::new());
        assert_eq!(none["host"], "default.example");
        assert_eq!(none["other"], "kept", "no default: the variable stays");
        assert_eq!(none["plain"], "p");

        let user: BTreeMap<_, _> = [
            ("host".to_owned(), "user.example".to_owned()),
            ("gone".to_owned(), "x".to_owned()),
        ]
        .into();
        let some = w.effective_variables(&user);
        assert_eq!(some["host"], "user.example");
        assert!(
            !some.contains_key("gone"),
            "values of removed fields are ignored"
        );
    }

    #[test]
    fn a_picked_home_path_is_expanded_for_scripts() {
        let mut root = field("root", ConfigKind::Filepicker);
        root.default = "~/Projects".to_owned();
        let text = field("note", ConfigKind::Textfield);
        let w = workflow(vec![root, text]);
        let user: BTreeMap<_, _> = [("note".to_owned(), "~/kept as typed".to_owned())].into();
        let vars = w.effective_variables(&user);
        if let Some(home) = home_dir() {
            assert_eq!(
                vars["root"],
                home.join("Projects").to_string_lossy().into_owned()
            );
        }
        assert_eq!(vars["note"], "~/kept as typed");
    }

    #[test]
    fn bad_declarations_make_the_workflow_invalid_but_user_values_do_not_change_approval() {
        use super::super::host::approval_key;
        use super::super::model::NodeKind;
        use super::super::testing::node;

        let mut flow = workflow(vec![
            field("dup", ConfigKind::Textfield),
            field("dup", ConfigKind::Textfield),
        ]);
        flow.nodes = vec![keyword("k"), copy("c", "x")];
        flow.connections = vec![wire("k", "c")];
        let errors: Vec<_> = flow
            .validate()
            .into_iter()
            .filter(|p| p.is_error())
            .collect();
        assert!(
            errors.iter().any(|p| p.message.contains("used twice")),
            "{errors:?}"
        );

        // Settings are the user's data: declaring them never changes what was
        // approved.
        let run = node(
            "r",
            NodeKind::RunScript {
                command: vec!["tool".into()],
                script: None,
                args: vec!["{var:host}".into()],
                stdin: None,
                env: BTreeMap::new(),
                timeout_ms: None,
                log_stderr: false,
            },
        );
        let mut plain = workflow(Vec::new());
        plain.nodes = vec![keyword("k"), run];
        plain.connections = vec![wire("k", "r")];
        let mut configured = plain.clone();
        configured.config = vec![field("host", ConfigKind::Textfield)];
        let dir = tempfile::tempdir().unwrap();
        assert!(approval_key(&plain, dir.path()).is_some());
        assert_eq!(
            approval_key(&plain, dir.path()),
            approval_key(&configured, dir.path())
        );
    }

    #[test]
    fn required_fields_without_a_value_are_missing() {
        let mut host = field("host", ConfigKind::Textfield);
        host.required = true;
        host.label = "Server".to_owned();
        let mut key = field("key", ConfigKind::Textfield);
        key.required = true;
        key.default = "d".to_owned();
        let mut flag = field("flag", ConfigKind::Checkbox);
        flag.required = true;
        let w = workflow(vec![host, key, flag]);

        assert_eq!(w.missing_config(&BTreeMap::new()), ["Server"]);
        let blank: BTreeMap<_, _> = [("host".to_owned(), "  ".to_owned())].into();
        assert_eq!(w.missing_config(&blank), ["Server"]);
        let filled: BTreeMap<_, _> = [("host".to_owned(), "h".to_owned())].into();
        assert!(w.missing_config(&filled).is_empty());
        assert_eq!(
            configure_message("My flow", &["Server".into(), "Key".into()]),
            "Configure My flow first: Server, Key"
        );
    }

    #[test]
    fn user_values_are_checked_against_the_declared_fields() {
        let mut slider = field("vol", ConfigKind::Slider);
        slider.max = Some(10.0);
        let w = workflow(vec![slider, field("name", ConfigKind::Textfield)]);
        let good: BTreeMap<String, Value> = [
            ("vol".to_owned(), json!(3)),
            ("name".to_owned(), json!("n")),
        ]
        .into();
        let stored = w.check_user_values(&good).unwrap();
        assert_eq!(stored["vol"], "3");
        let unknown: BTreeMap<String, Value> = [("nope".to_owned(), json!("x"))].into();
        assert!(w.check_user_values(&unknown).unwrap_err().contains("nope"));
        let out: BTreeMap<String, Value> = [("vol".to_owned(), json!(99))].into();
        assert!(w.check_user_values(&out).is_err());
    }

    #[test]
    fn user_values_are_stored_apart_and_survive_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data").join("flow");
        assert!(load_user_values(&data).is_empty());
        let values: BTreeMap<_, _> = [("a".to_owned(), "1".to_owned())].into();
        save_user_values(&data, &values).unwrap();
        assert_eq!(load_user_values(&data), values);
        fs::write(user_config_path(&data), "{ not json").unwrap();
        assert!(load_user_values(&data).is_empty());
        fs::write(
            user_config_path(&data),
            r#"{"a": 5, "b": true, "c": [1], "d": "x"}"#,
        )
        .unwrap();
        let loaded = load_user_values(&data);
        assert_eq!(loaded["a"], "5");
        assert_eq!(loaded["b"], "true");
        assert_eq!(loaded["d"], "x");
        assert!(!loaded.contains_key("c"));
    }
    use std::sync::Arc;

    use sevak_core::Config;

    use super::super::exec::{Ctx, Runtime};
    use super::super::testing::{clipboard, copy, keyword, wire, Fixture, RecordingSink};
    use crate::test_util::MockPlatform;

    fn configured(
        fields: Vec<ConfigField>,
        variables: &[(&str, &str)],
        user: &[(&str, &str)],
        nodes: Vec<super::super::model::Node>,
    ) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::empty();
        let sink = Arc::new(RecordingSink::default());
        let mut flow = workflow(fields);
        flow.name = "Test flow".to_owned();
        flow.variables = variables
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        flow.connections = vec![wire("k", "c")];
        flow.nodes = nodes;
        assert!(flow.is_valid(), "{:?}", flow.validate());
        let runtime = Runtime::configured(
            "test-flow",
            dir.path().join("wf"),
            dir.path().join("data").join("test-flow"),
            dir.path().join("cache").join("test-flow"),
            user.iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            flow,
            platform.clone(),
            sink.clone(),
            &Config::default(),
        );
        Fixture {
            runtime,
            platform,
            sink,
            _dir: dir,
        }
    }

    fn text_field(variable: &str, default: &str, required: bool, label: &str) -> ConfigField {
        let mut f = field(variable, ConfigKind::Textfield);
        f.default = default.to_owned();
        f.required = required;
        f.label = label.to_owned();
        f
    }

    #[test]
    fn a_run_sees_user_value_then_default_then_workflow_variable() {
        let nodes = vec![keyword("k"), copy("c", "{var:a}|{var:b}|{var:c}|{var:d}")];
        let fields = vec![
            text_field("a", "default-a", false, ""),
            text_field("b", "default-b", false, ""),
            text_field("c", "", false, ""),
        ];
        let f = configured(
            fields,
            &[
                ("a", "var-a"),
                ("b", "var-b"),
                ("c", "var-c"),
                ("d", "var-d"),
            ],
            &[("a", "user-a")],
            nodes,
        );
        f.runtime.run_blocking("k", Ctx::with_arg(""));
        assert_eq!(clipboard(&f), ["user-a|default-b|var-c|var-d"]);
    }

    #[test]
    fn config_values_reach_scripts_as_environment_variables() {
        let f = configured(
            vec![text_field("api_host", "", false, "")],
            &[],
            &[("api_host", "example.test")],
            vec![keyword("k"), copy("c", "x")],
        );
        let env: std::collections::BTreeMap<_, _> =
            f.runtime.script_environment().into_iter().collect();
        assert_eq!(env["api_host"], "example.test");
        assert!(env["alfred_workflow_cache"].contains("cache"));
        assert_ne!(env["alfred_workflow_cache"], env["alfred_workflow_data"]);
        assert_eq!(env["alfred_version"], "5.5");
        assert_eq!(env["alfred_debug"], "0");
    }

    #[test]
    fn a_workflow_missing_required_settings_refuses_to_run() {
        let nodes = vec![keyword("k"), copy("c", "ran")];
        let f = configured(
            vec![
                text_field("host", "", true, "Server"),
                text_field("key", "", true, "API key"),
                text_field("opt", "", false, "Optional"),
            ],
            &[],
            &[],
            nodes.clone(),
        );
        let message = "Configure Test flow first: Server, API key";
        assert_eq!(f.runtime.configure_message().as_deref(), Some(message));
        // `start` says why at once and tells the user.
        assert_eq!(
            f.runtime.start("k", Ctx::with_arg("")).unwrap_err(),
            message
        );
        assert_eq!(
            f.sink.notes.lock().unwrap().as_slice(),
            [("Test flow".to_owned(), message.to_owned())]
        );
        // Nothing runs on any other path either.
        let report = f.runtime.run_blocking("k", Ctx::with_arg(""));
        assert!(report.executed.is_empty());
        assert_eq!(report.errors[0].message, message);
        assert!(clipboard(&f).is_empty());

        // Filled in, it runs.
        let ready = configured(
            vec![text_field("host", "", true, "Server")],
            &[],
            &[("host", "h")],
            nodes,
        );
        assert_eq!(ready.runtime.configure_message(), None);
        ready.runtime.run_blocking("k", Ctx::with_arg(""));
        assert_eq!(clipboard(&ready), ["ran"]);
    }

    #[test]
    fn the_trigger_row_says_it_needs_configuration() {
        use super::super::plugins::KeywordPlugin;
        use sevak_core::Plugin;
        let f = configured(
            vec![text_field("host", "", true, "Server")],
            &[],
            &[],
            vec![keyword("k"), copy("c", "x")],
        );
        let node = f.runtime.workflow.node("k").unwrap().clone();
        let plugin = KeywordPlugin::new(f.runtime.clone(), &node).unwrap();
        let rows = plugin.query("go");
        assert_eq!(rows[0].subtitle, "Configure Test flow first: Server");
        assert!(plugin.execute(&rows[0]).is_err());
    }

    #[test]
    fn a_config_value_in_a_terminal_command_is_one_quoted_word() {
        use super::super::template::{expand_command, Scope};
        use sevak_platform::ShellQuoting;
        let vars: BTreeMap<String, String> =
            [("host".to_owned(), "a; rm -rf ~ && $(id) `x`".to_owned())].into();
        let scope = Scope {
            query: "",
            vars: &vars,
        };
        let command = expand_command("ping {var:host}", &scope, ShellQuoting::Posix).unwrap();
        assert_eq!(command, "ping 'a; rm -rf ~ && $(id) `x`'");
    }
}
