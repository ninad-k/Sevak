//! Sevak UI: a small, JSON-defined user interface for the `show_ui` node.
//!
//! A definition is one JSON document (inline in the node, or a file in the
//! workflow folder, which may pull in sub-trees with `{file:other.json}`):
//!
//! ```json
//! { "title": "...", "width": 420, "height": 300,
//!   "state": { "name": "" }, "templates": { "row": { ... } },
//!   "root": { "type": "column", "children": [ ... ] } }
//! ```
//!
//! [`load`] runs the whole pipeline: size limit, includes, template (`use`)
//! expansion, typed parsing, validation. The result is plain data the shell
//! hands to the renderer; there is no code in it. Text placeholders
//! (`{state:id}`, `{var:name}`, `{query}`) are filled in by the renderer as the
//! state changes; `{param:x}` is filled in here, when a template is used.
//!
//! Web views are intentionally not a component: the UI never loads remote
//! content and never navigates.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::script::relative_inside;
use crate::selection::transform::base64_encode;

/// Largest definition, counting every included file.
pub const MAX_DEFINITION_BYTES: usize = 1024 * 1024;
/// Deepest nesting of components.
pub const MAX_DEPTH: usize = 32;
/// Most components in one UI (after templates are expanded).
pub const MAX_COMPONENTS: usize = 2000;
/// Largest image the host loads.
pub const MAX_IMAGE_BYTES: u64 = 4 * 1024 * 1024;
/// Most bytes template expansion may copy in all.
const MAX_EXPANDED_BYTES: usize = MAX_DEFINITION_BYTES * 4;
/// Most files [`include_files`] follows.
const MAX_INCLUDE_FILES: usize = 64;
/// Most `{file:...}` includes nested inside each other.
const MAX_INCLUDE_DEPTH: usize = 8;
/// Most images one UI may show.
const MAX_IMAGES: usize = 32;
/// The output port taken when the UI is dismissed.
pub const CLOSED: &str = "closed";
/// Most scripts one node declares.
pub const MAX_UI_SCRIPTS: usize = 16;
/// Most rows of a `list` component.
const MAX_LIST_ITEMS: usize = 2000;

/// Where a UI opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiTarget {
    /// In the launcher panel, replacing the results.
    #[default]
    Launcher,
    /// In a window of its own.
    Window,
}

/// A script a UI event may run (`{"run": "<id>"}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiScript {
    /// `shell` (`sh`), `python`, `javascript` (`node`) or `powershell`.
    pub language: String,
    pub script: String,
}

/// File extension of a script language, as [`sevak_platform::process::script_runner`]
/// knows it.
pub fn language_extension(language: &str) -> Option<&'static str> {
    match language.trim().to_ascii_lowercase().as_str() {
        "sh" | "shell" | "bash" => Some("sh"),
        "py" | "python" | "python3" => Some("py"),
        "js" | "javascript" | "node" => Some("js"),
        "ps1" | "powershell" | "pwsh" => Some("ps1"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelStyle {
    Heading,
    #[default]
    Body,
    Caption,
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonStyle {
    Primary,
    #[default]
    Secondary,
    Danger,
}

/// What a component does when something happens to it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    /// Sets state values; the values are text templates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<BTreeMap<String, Value>>,
    /// Closes the UI (the workflow continues from `closed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close: Option<bool>,
    /// Continues the workflow from this output port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// With `output`: the argument (a template); the JSON of the state when
    /// absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arg: Option<String>,
    /// Runs one of the node's scripts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    /// Shows a notification (a template).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify: Option<String>,
    /// Copies text to the clipboard (a template).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<String>,
}

/// One action or a list of them.
fn actions<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Action>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(Action),
        Many(Vec<Action>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(one) => vec![one],
        OneOrMany::Many(many) => many,
    })
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Events {
    #[serde(
        default,
        deserialize_with = "actions",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub click: Vec<Action>,
    #[serde(
        default,
        deserialize_with = "actions",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub change: Vec<Action>,
    #[serde(
        default,
        deserialize_with = "actions",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub submit: Vec<Action>,
    #[serde(
        default,
        deserialize_with = "actions",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub select: Vec<Action>,
}

impl Events {
    fn all(&self) -> impl Iterator<Item = &Action> {
        self.click
            .iter()
            .chain(&self.change)
            .chain(&self.submit)
            .chain(&self.select)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickerOption {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListItem {
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subtitle: String,
    /// The value selected; the title when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arg: String,
}

/// A container's layout.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    /// A grid's column count (1 to 12; 1 when absent). Ignored elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<u32>,
    #[serde(default)]
    pub children: Vec<Component>,
}

/// One element of the UI. `use` never appears here: templates are expanded
/// before the tree is typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Component {
    Column(Layout),
    Row(Layout),
    Grid(Layout),
    Label {
        text: String,
        #[serde(default)]
        style: LabelStyle,
    },
    Markdown {
        text: String,
    },
    Button {
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default)]
        style: ButtonStyle,
        #[serde(default)]
        disabled: bool,
        #[serde(default)]
        events: Events,
    },
    Checkbox {
        id: String,
        label: String,
        #[serde(default)]
        events: Events,
    },
    Textfield {
        id: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default)]
        events: Events,
    },
    Passwordfield {
        id: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default)]
        events: Events,
    },
    Textarea {
        id: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rows: Option<u32>,
        #[serde(default)]
        events: Events,
    },
    Picker {
        id: String,
        options: Vec<PickerOption>,
        #[serde(default)]
        events: Events,
    },
    Slider {
        id: String,
        min: f64,
        max: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<f64>,
        #[serde(default)]
        events: Events,
    },
    Progress {
        /// A number from 0 to 1, or a template that gives one (`{state:p}`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
        #[serde(default)]
        indeterminate: bool,
    },
    Image {
        path: String,
        #[serde(default)]
        alt: String,
    },
    List {
        id: String,
        items: Vec<ListItem>,
        /// The id of a textfield whose text filters the rows.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter_by: Option<String>,
        #[serde(default)]
        multi: bool,
        #[serde(default)]
        events: Events,
    },
    Separator,
    Spacer,
}

/// A parsed, validated UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiDefinition {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default)]
    pub state: BTreeMap<String, Value>,
    /// Kept for the builder; already expanded inside `root`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub templates: BTreeMap<String, Value>,
    pub root: Component,
}

// ---- loading -------------------------------------------------------------

/// What the node's definition source is.
pub struct Source<'a> {
    pub inline: Option<&'a str>,
    pub file: Option<&'a str>,
    /// The workflow folder: `file`, includes and relative images live in it.
    pub dir: &'a Path,
}

/// Reads, expands and validates a definition. `scripts` are the ids a `run`
/// action may name. Errors are one readable sentence.
pub fn load(source: &Source, scripts: &BTreeSet<String>) -> Result<UiDefinition, String> {
    let mut budget = MAX_DEFINITION_BYTES;
    let text = match (source.inline.map(str::trim), source.file) {
        (Some(inline), _) if !inline.is_empty() => {
            take(&mut budget, inline.len())?;
            inline.to_owned()
        }
        (_, Some(file)) if !file.trim().is_empty() => read_inside(source.dir, file, &mut budget)?,
        _ => return Err("the UI has no definition (write one, or name a file)".to_owned()),
    };
    let value: Value = serde_json::from_str(&text)
        .map_err(|err| format!("the UI definition is not valid JSON: {err}"))?;
    let value = resolve_includes(value, source.dir, 0, &mut budget)?;
    parse_value(value, scripts)
}

/// Parses an already-resolved JSON value (also used for a script's live
/// `root` replacement, which has no includes).
pub fn parse_value(value: Value, scripts: &BTreeSet<String>) -> Result<UiDefinition, String> {
    let Value::Object(mut object) = value else {
        return Err("the UI definition must be a JSON object".to_owned());
    };
    let templates: BTreeMap<String, Value> = match object.remove("templates") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(Value::Object(map)) => map.into_iter().collect(),
        Some(_) => return Err("`templates` must be an object of components".to_owned()),
    };
    let root = object
        .remove("root")
        .ok_or("the UI needs a `root` component")?;
    let mut count = 0;
    let mut bytes = 0;
    let root = expand_templates(root, &templates, 0, &mut count, &mut bytes)?;
    object.insert("root".to_owned(), root);
    object.insert(
        "templates".to_owned(),
        Value::Object(templates.clone().into_iter().collect()),
    );
    let definition: UiDefinition = serde_json::from_value(Value::Object(object))
        .map_err(|err| format!("the UI definition is not valid: {err}"))?;
    validate(&definition, scripts)?;
    Ok(definition)
}

fn take(budget: &mut usize, bytes: usize) -> Result<(), String> {
    *budget = budget.checked_sub(bytes).ok_or_else(|| {
        format!(
            "the UI definition is larger than {} KB",
            MAX_DEFINITION_BYTES / 1024
        )
    })?;
    Ok(())
}

/// Reads `relative` inside `dir`, refusing anything that leaves it (a `..`, an
/// absolute path, or a link that points elsewhere).
fn read_inside(dir: &Path, relative: &str, budget: &mut usize) -> Result<String, String> {
    let path = relative_inside(dir, relative.trim())
        .ok_or_else(|| format!("\"{relative}\" must be a path inside the workflow folder"))?;
    let real = path
        .canonicalize()
        .map_err(|_| format!("the file \"{relative}\" was not found"))?;
    let root = dir
        .canonicalize()
        .map_err(|err| format!("the workflow folder cannot be read: {err}"))?;
    if !real.starts_with(&root) {
        return Err(format!("\"{relative}\" points outside the workflow folder"));
    }
    let size = std::fs::metadata(&real)
        .map_err(|err| format!("could not read \"{relative}\": {err}"))?
        .len();
    if size as usize > *budget {
        return Err(format!(
            "the UI definition is larger than {} KB",
            MAX_DEFINITION_BYTES / 1024
        ));
    }
    let text = std::fs::read_to_string(&real)
        .map_err(|err| format!("could not read \"{relative}\": {err}"))?;
    take(budget, text.len())?;
    Ok(text)
}

/// Every file the definition pulls in with `{file:...}`, directly or through
/// other included files, for the approval hash: the top-level `file` first,
/// then the includes of the inline text and of each file read. Bounded;
/// anything outside the workflow folder or unreadable is listed by name only
/// (the hash then records it as missing).
pub fn include_files(inline: Option<&str>, file: Option<&str>, dir: &Path) -> Vec<String> {
    fn scan(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(text) => {
                if let Some(rel) = text
                    .trim()
                    .strip_prefix("{file:")
                    .and_then(|rest| rest.strip_suffix('}'))
                {
                    out.push(rel.trim().to_owned());
                }
            }
            Value::Array(items) => items.iter().for_each(|item| scan(item, out)),
            Value::Object(map) => map.values().for_each(|item| scan(item, out)),
            _ => {}
        }
    }
    let mut found: Vec<String> = Vec::new();
    let mut queue: Vec<String> = Vec::new();
    if let Some(file) = file.map(str::trim).filter(|f| !f.is_empty()) {
        queue.push(file.to_owned());
    }
    if let Some(text) = inline {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            scan(&value, &mut queue);
        }
    }
    while let Some(next) = queue.pop() {
        if found.contains(&next) {
            continue;
        }
        if found.len() >= MAX_INCLUDE_FILES {
            break;
        }
        found.push(next.clone());
        let mut budget = MAX_DEFINITION_BYTES;
        if let Ok(text) = read_inside(dir, &next, &mut budget) {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                scan(&value, &mut queue);
            }
        }
    }
    found.sort();
    found
}

/// Replaces every string that is exactly `{file:relative.json}` with the
/// parsed file.
fn resolve_includes(
    value: Value,
    dir: &Path,
    depth: usize,
    budget: &mut usize,
) -> Result<Value, String> {
    Ok(match value {
        Value::String(text) => match text
            .trim()
            .strip_prefix("{file:")
            .and_then(|rest| rest.strip_suffix('}'))
        {
            Some(relative) => {
                if depth >= MAX_INCLUDE_DEPTH {
                    return Err("files include each other too deeply".to_owned());
                }
                let body = read_inside(dir, relative, budget)?;
                let parsed: Value = serde_json::from_str(&body)
                    .map_err(|err| format!("\"{relative}\" is not valid JSON: {err}"))?;
                resolve_includes(parsed, dir, depth + 1, budget)?
            }
            None => Value::String(text),
        },
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| resolve_includes(item, dir, depth, budget))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, item)| Ok((key, resolve_includes(item, dir, depth, budget)?)))
                .collect::<Result<Map<_, _>, String>>()?,
        ),
        other => other,
    })
}

// ---- templates -----------------------------------------------------------

/// Expands every `{"type": "use", "template": name, "params": {...}}`.
fn expand_templates(
    value: Value,
    templates: &BTreeMap<String, Value>,
    depth: usize,
    count: &mut usize,
    bytes: &mut usize,
) -> Result<Value, String> {
    if depth > MAX_DEPTH {
        return Err(format!(
            "components nest more than {MAX_DEPTH} levels deep (does a template use itself?)"
        ));
    }
    *count += 1;
    if *count > MAX_COMPONENTS * 4 {
        return Err("the UI is too large once its templates are expanded".to_owned());
    }
    match value {
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("use") => {
            let name = map
                .get("template")
                .and_then(Value::as_str)
                .ok_or("a `use` component needs a `template` name")?;
            let body = templates
                .get(name)
                .ok_or_else(|| format!("the template \"{name}\" does not exist"))?;
            let params = match map.get("params") {
                None | Some(Value::Null) => Map::new(),
                Some(Value::Object(params)) => params.clone(),
                Some(_) => return Err("`params` must be an object".to_owned()),
            };
            let filled = substitute_params(body.clone(), &params);
            // Every expansion pays for the size of what it copies, so nested
            // templates with large parameters cannot grow without bound.
            *bytes = bytes.saturating_add(filled.to_string().len());
            if *bytes > MAX_EXPANDED_BYTES {
                return Err("the UI is too large once its templates are expanded".to_owned());
            }
            expand_templates(filled, templates, depth + 1, count, bytes)
        }
        Value::Object(map) => Ok(Value::Object(
            map.into_iter()
                .map(|(key, item)| {
                    Ok((key, expand_templates(item, templates, depth, count, bytes)?))
                })
                .collect::<Result<Map<_, _>, String>>()?,
        )),
        Value::Array(items) => Ok(Value::Array(
            items
                .into_iter()
                .map(|item| expand_templates(item, templates, depth, count, bytes))
                .collect::<Result<_, _>>()?,
        )),
        other => Ok(other),
    }
}

/// Fills `{param:x}` in every string. A string that is only the placeholder
/// takes the parameter's own JSON value (a number, a sub-tree...); inside a
/// longer string the value is written as text. An unknown parameter is left
/// as it is.
pub fn substitute_params(value: Value, params: &Map<String, Value>) -> Value {
    match value {
        Value::String(text) => {
            if let Some(name) = whole_param(&text) {
                if let Some(found) = params.get(name) {
                    return found.clone();
                }
                return Value::String(text);
            }
            Value::String(fill_params(&text, params))
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| substitute_params(item, params))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, item)| (key, substitute_params(item, params)))
                .collect(),
        ),
        other => other,
    }
}

fn whole_param(text: &str) -> Option<&str> {
    let name = text.strip_prefix("{param:")?.strip_suffix('}')?;
    (!name.contains(['{', '}'])).then_some(name)
}

fn fill_params(text: &str, params: &Map<String, Value>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{param:") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "{param:".len()..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        match params.get(name) {
            Some(Value::String(value)) => out.push_str(value),
            Some(Value::Null) => {}
            Some(other) => out.push_str(&other.to_string()),
            None => out.push_str(&rest[start..start + "{param:".len() + end + 1]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

// ---- validation ----------------------------------------------------------

/// Valid script ids: the same plain words as component ids.
pub fn valid_script_id(id: &str) -> bool {
    valid_id(id)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Valid port names: lower-case words, so they read well in the builder.
pub fn valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.len() <= 32
        && port
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

struct Checker<'a> {
    scripts: &'a BTreeSet<String>,
    ids: BTreeMap<String, &'static str>,
    textfields: BTreeSet<String>,
    filters: Vec<String>,
    count: usize,
    images: usize,
}

/// Checks a parsed UI. The first problem is the error.
pub fn validate(definition: &UiDefinition, scripts: &BTreeSet<String>) -> Result<(), String> {
    if definition.width.is_some_and(|w| !(200..=4000).contains(&w))
        || definition
            .height
            .is_some_and(|h| !(100..=4000).contains(&h))
    {
        return Err("`width` must be 200 to 4000 and `height` 100 to 4000".to_owned());
    }
    let mut checker = Checker {
        scripts,
        ids: BTreeMap::new(),
        textfields: BTreeSet::new(),
        filters: Vec::new(),
        count: 0,
        images: 0,
    };
    checker.component(&definition.root, 1)?;
    for filter in &checker.filters {
        if !checker.textfields.contains(filter) {
            return Err(format!(
                "`filter_by` names \"{filter}\", which is not a textfield of this UI"
            ));
        }
    }
    for name in definition.state.keys() {
        if !valid_id(name) {
            return Err(format!(
                "\"{name}\" is not a valid state id (letters, digits, - and _)"
            ));
        }
    }
    Ok(())
}

impl Checker<'_> {
    fn id(&mut self, kind: &'static str, id: &str) -> Result<(), String> {
        if !valid_id(id) {
            return Err(format!(
                "the {kind} id \"{id}\" is not valid (use letters, digits, - and _; at most 64)"
            ));
        }
        if self.ids.insert(id.to_owned(), kind).is_some() {
            return Err(format!("the id \"{id}\" is used twice; ids must be unique"));
        }
        Ok(())
    }

    fn events(&self, events: &Events) -> Result<(), String> {
        for action in events.all() {
            let verbs = [
                action.set.is_some(),
                action.close.is_some(),
                action.output.is_some(),
                action.run.is_some(),
                action.notify.is_some(),
                action.copy.is_some(),
            ]
            .iter()
            .filter(|set| **set)
            .count();
            if verbs != 1 {
                return Err(
                    "every action does exactly one thing: set, close, output, run, notify or copy"
                        .to_owned(),
                );
            }
            if action.arg.is_some() && action.output.is_none() {
                return Err("`arg` only goes with an `output` action".to_owned());
            }
            if let Some(port) = &action.output {
                if !valid_port(port) {
                    return Err(format!(
                        "\"{port}\" is not a valid output name (lower-case letters, digits, - and _)"
                    ));
                }
            }
            if let Some(script) = &action.run {
                if !self.scripts.contains(script) {
                    return Err(format!(
                        "the action runs the script \"{script}\", which this node does not declare"
                    ));
                }
            }
            if let Some(set) = &action.set {
                for name in set.keys() {
                    if !valid_id(name) {
                        return Err(format!("\"{name}\" is not a valid state id"));
                    }
                }
            }
        }
        Ok(())
    }

    fn component(&mut self, component: &Component, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(format!("components nest more than {MAX_DEPTH} levels deep"));
        }
        self.count += 1;
        if self.count > MAX_COMPONENTS {
            return Err(format!("a UI may have at most {MAX_COMPONENTS} components"));
        }
        match component {
            Component::Column(layout) | Component::Row(layout) => {
                for child in &layout.children {
                    self.component(child, depth + 1)?;
                }
            }
            Component::Grid(layout) => {
                if !(1..=12).contains(&layout.columns.unwrap_or(1)) {
                    return Err("a grid needs 1 to 12 columns".to_owned());
                }
                for child in &layout.children {
                    self.component(child, depth + 1)?;
                }
            }
            Component::Label { .. }
            | Component::Markdown { .. }
            | Component::Separator
            | Component::Spacer => {}
            Component::Button {
                label, id, events, ..
            } => {
                if label.trim().is_empty() {
                    return Err("a button needs a label".to_owned());
                }
                if let Some(id) = id {
                    self.id("button", id)?;
                }
                self.events(events)?;
            }
            Component::Checkbox { id, label, events } => {
                self.id("checkbox", id)?;
                if label.trim().is_empty() {
                    return Err(format!("the checkbox \"{id}\" needs a label"));
                }
                self.events(events)?;
            }
            Component::Textfield { id, events, .. }
            | Component::Passwordfield { id, events, .. } => {
                self.id("textfield", id)?;
                self.textfields.insert(id.clone());
                self.events(events)?;
            }
            Component::Textarea {
                id, rows, events, ..
            } => {
                self.id("textarea", id)?;
                if rows.is_some_and(|rows| !(1..=50).contains(&rows)) {
                    return Err("a textarea has 1 to 50 rows".to_owned());
                }
                self.events(events)?;
            }
            Component::Picker {
                id,
                options,
                events,
            } => {
                self.id("picker", id)?;
                if options.is_empty() || options.len() > MAX_LIST_ITEMS {
                    return Err(format!("the picker \"{id}\" needs at least one option"));
                }
                self.events(events)?;
            }
            Component::Slider {
                id,
                min,
                max,
                step,
                events,
            } => {
                self.id("slider", id)?;
                if !(min.is_finite() && max.is_finite()) || min >= max {
                    return Err(format!("the slider \"{id}\" needs min below max"));
                }
                if step.is_some_and(|step| !step.is_finite() || step <= 0.0) {
                    return Err(format!("the slider \"{id}\" needs a step above 0"));
                }
                self.events(events)?;
            }
            Component::Progress {
                value,
                indeterminate,
            } => match value {
                None => {
                    if !indeterminate {
                        return Err(
                            "a progress bar needs a `value` from 0 to 1, or `indeterminate`"
                                .to_owned(),
                        );
                    }
                }
                Some(Value::Number(number)) => {
                    if !number.as_f64().is_some_and(|n| (0.0..=1.0).contains(&n)) {
                        return Err("a progress `value` is between 0 and 1".to_owned());
                    }
                }
                Some(Value::String(_)) => {}
                Some(_) => return Err("a progress `value` is a number or a template".to_owned()),
            },
            Component::Image { path, .. } => {
                if path.trim().is_empty() {
                    return Err("an image needs a `path`".to_owned());
                }
                let lower = path.trim().to_ascii_lowercase();
                if lower.contains("://") || lower.starts_with("data:") || lower.starts_with("//") {
                    return Err(
                        "images are loaded from files only, never from the network".to_owned()
                    );
                }
                if !has_image_extension(path) {
                    return Err(format!("\"{path}\" is not a png, jpeg, gif or webp image"));
                }
                self.images += 1;
                if self.images > MAX_IMAGES {
                    return Err(format!("a UI may show at most {MAX_IMAGES} images"));
                }
            }
            Component::List {
                id,
                items,
                filter_by,
                events,
                ..
            } => {
                self.id("list", id)?;
                if items.len() > MAX_LIST_ITEMS {
                    return Err(format!(
                        "the list \"{id}\" has more than {MAX_LIST_ITEMS} rows"
                    ));
                }
                if let Some(filter) = filter_by {
                    self.filters.push(filter.clone());
                }
                self.events(events)?;
            }
        }
        Ok(())
    }
}

// ---- ports ----------------------------------------------------------------

fn collect_ports(component: &Component, out: &mut Vec<String>) {
    let mut events = |events: &Events| {
        for action in events.all() {
            if let Some(port) = &action.output {
                if !out.contains(port) {
                    out.push(port.clone());
                }
            }
        }
    };
    match component {
        Component::Button { events: e, .. }
        | Component::Checkbox { events: e, .. }
        | Component::Textfield { events: e, .. }
        | Component::Passwordfield { events: e, .. }
        | Component::Textarea { events: e, .. }
        | Component::Picker { events: e, .. }
        | Component::Slider { events: e, .. }
        | Component::List { events: e, .. } => events(e),
        _ => {}
    }
    let children = match component {
        Component::Column(layout) | Component::Row(layout) => &layout.children,
        Component::Grid(layout) => &layout.children,
        _ => return,
    };
    for child in children {
        collect_ports(child, out);
    }
}

/// The output ports a definition uses: those its `output` actions name, in
/// order of appearance, then `closed`.
pub fn definition_ports(definition: &UiDefinition) -> Vec<String> {
    let mut ports = Vec::new();
    collect_ports(&definition.root, &mut ports);
    ports.retain(|port| port != CLOSED);
    ports.push(CLOSED.to_owned());
    ports
}

/// The output ports of the node, without reading any file: those named by the
/// inline `definition` (so long as it parses; unparsable text names none), the
/// `declared` ones (for a `file` definition), then `closed`.
pub fn node_ports(inline: Option<&str>, declared: &[String]) -> Vec<String> {
    let mut ports: Vec<String> = Vec::new();
    if let Some(text) = inline {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            if let Ok(definition) = parse_value(value, &script_wildcard(text)) {
                ports = definition_ports(&definition);
            }
        }
    }
    ports.retain(|port| port != CLOSED);
    for port in declared {
        if valid_port(port) && port != CLOSED && !ports.contains(port) {
            ports.push(port.clone());
        }
    }
    ports.push(CLOSED.to_owned());
    ports
}

/// Every `run` target named in `text`, so port discovery does not fail on
/// scripts it does not know about.
fn script_wildcard(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        fn walk(value: &Value, found: &mut BTreeSet<String>) {
            match value {
                Value::Object(map) => {
                    if let Some(Value::String(run)) = map.get("run") {
                        found.insert(run.clone());
                    }
                    map.values().for_each(|item| walk(item, found));
                }
                Value::Array(items) => items.iter().for_each(|item| walk(item, found)),
                _ => {}
            }
        }
        walk(&value, &mut found);
    }
    found
}

/// How many components `root` holds (itself included).
pub fn count_components(root: &Component) -> usize {
    1 + match root {
        Component::Column(layout) | Component::Row(layout) | Component::Grid(layout) => {
            layout.children.iter().map(count_components).sum()
        }
        _ => 0,
    }
}

// ---- images ---------------------------------------------------------------

fn has_image_extension(path: &str) -> bool {
    mime_for(path).is_some()
}

fn mime_for(path: &str) -> Option<&'static str> {
    let extension = Path::new(path.trim())
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn collect_images<'a>(component: &'a Component, out: &mut Vec<&'a str>) {
    if let Component::Image { path, .. } = component {
        out.push(path);
    }
    let children = match component {
        Component::Column(layout) | Component::Row(layout) => &layout.children,
        Component::Grid(layout) => &layout.children,
        _ => return,
    };
    for child in children {
        collect_images(child, out);
    }
}

/// Loads every image of `root` as a `data:` URL, keyed by the path written in
/// the definition. Relative paths are inside `dir`. An image that cannot be
/// loaded is left out (the renderer shows its alt text) and reported.
pub fn load_images(root: &Component, dir: &Path) -> (BTreeMap<String, String>, Vec<String>) {
    let mut paths = Vec::new();
    collect_images(root, &mut paths);
    let mut images = BTreeMap::new();
    let mut problems = Vec::new();
    for path in paths {
        if images.contains_key(path) {
            continue;
        }
        match load_image(dir, path) {
            Ok(url) => {
                images.insert(path.to_owned(), url);
            }
            Err(err) => problems.push(format!("image \"{path}\": {err}")),
        }
    }
    (images, problems)
}

fn load_image(dir: &Path, path: &str) -> Result<String, String> {
    let mime = mime_for(path).ok_or("not a png, jpeg, gif or webp image")?;
    let trimmed = path.trim();
    let file = if Path::new(trimmed).is_absolute() {
        Path::new(trimmed).to_path_buf()
    } else {
        let inside = relative_inside(dir, trimmed).ok_or("must be inside the workflow folder")?;
        let real = inside.canonicalize().map_err(|_| "not found".to_owned())?;
        let root = dir.canonicalize().map_err(|err| err.to_string())?;
        if !real.starts_with(root) {
            return Err("points outside the workflow folder".to_owned());
        }
        real
    };
    let meta = std::fs::metadata(&file).map_err(|_| "not found".to_owned())?;
    if !meta.is_file() {
        return Err("not a file".to_owned());
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err("larger than 4 MB".to_owned());
    }
    let bytes = std::fs::read(&file).map_err(|err| err.to_string())?;
    if !looks_like(mime, &bytes) {
        return Err("the file is not really that kind of image".to_owned());
    }
    Ok(format!("data:{mime};base64,{}", base64_encode(&bytes)))
}

fn looks_like(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "image/gif" => bytes.starts_with(b"GIF8"),
        "image/webp" => bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

// ---- what a script says ----------------------------------------------------

/// What a UI script printed (JSON on standard output).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptReply {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub set: BTreeMap<String, Value>,
    /// A component that replaces the whole UI (templates still apply).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub close: bool,
}

/// Parses a script's standard output. Nothing printed is an empty reply.
pub fn parse_reply(stdout: &str) -> Result<ScriptReply, String> {
    let text = stdout.trim();
    if text.is_empty() {
        return Ok(ScriptReply::default());
    }
    serde_json::from_str(text)
        .map_err(|err| format!("the script must print JSON like {{\"set\": {{...}}}}: {err}"))
}

/// A live `root` from a script, checked against the UI's own templates and
/// scripts. Returns the replacement component.
pub fn replacement_root(
    root: Value,
    current: &UiDefinition,
    scripts: &BTreeSet<String>,
) -> Result<Component, String> {
    let mut object = Map::new();
    object.insert("root".to_owned(), root);
    object.insert(
        "templates".to_owned(),
        Value::Object(current.templates.clone().into_iter().collect()),
    );
    object.insert(
        "state".to_owned(),
        Value::Object(current.state.clone().into_iter().collect()),
    );
    Ok(parse_value(Value::Object(object), scripts)?.root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scripts(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    fn parse(value: Value) -> Result<UiDefinition, String> {
        parse_value(value, &scripts(&["go"]))
    }

    fn column(children: Value) -> Value {
        json!({"type": "column", "children": children})
    }

    #[test]
    fn a_small_ui_parses_with_defaults() {
        let ui = parse(json!({
            "title": "Hi", "width": 400, "state": {"name": "x"},
            "root": column(json!([
                {"type": "label", "text": "Hello {state:name}", "style": "heading"},
                {"type": "textfield", "id": "name", "events": {"submit": {"output": "done"}}},
                {"type": "button", "label": "Go", "style": "primary",
                 "events": {"click": [{"run": "go"}, {"notify": "ok"}]}}
            ]))
        }))
        .unwrap();
        assert_eq!(ui.title, "Hi");
        assert_eq!(ui.width, Some(400));
        assert_eq!(definition_ports(&ui), ["done", CLOSED]);
    }

    #[test]
    fn unknown_types_and_fields_are_errors() {
        assert!(parse(json!({"root": {"type": "webview", "url": "http://x"}})).is_err());
        assert!(parse(json!({"root": {"type": "label", "text": "a", "color": "red"}})).is_err());
        assert!(parse(json!({"title": "no root"})).is_err());
        assert!(parse(json!([1, 2])).is_err());
    }

    #[test]
    fn required_properties_are_checked() {
        for bad in [
            json!({"type": "button", "label": " "}),
            json!({"type": "textfield"}),
            json!({"type": "picker", "id": "p", "options": []}),
            json!({"type": "slider", "id": "s", "min": 5, "max": 1}),
            json!({"type": "progress"}),
            json!({"type": "progress", "value": 2}),
            json!({"type": "grid", "columns": 0}),
            json!({"type": "checkbox", "id": "c", "label": ""}),
            json!({"type": "textfield", "id": "bad id"}),
        ] {
            assert!(parse(json!({"root": bad.clone()})).is_err(), "{bad}");
        }
        assert!(parse(json!({"root": {"type": "progress", "indeterminate": true}})).is_ok());
    }

    #[test]
    fn ids_must_be_unique() {
        let err = parse(json!({"root": column(json!([
            {"type": "textfield", "id": "a"},
            {"type": "checkbox", "id": "a", "label": "x"}
        ]))}))
        .unwrap_err();
        assert!(err.contains("used twice"), "{err}");
    }

    #[test]
    fn actions_do_one_thing_and_name_real_scripts() {
        let button =
            |events: Value| json!({"root": {"type": "button", "label": "b", "events": events}});
        assert!(parse(button(json!({"click": {"run": "go"}}))).is_ok());
        assert!(parse(button(json!({"click": {"run": "missing"}}))).is_err());
        assert!(parse(button(json!({"click": {"set": {"a": "1"}, "close": true}}))).is_err());
        assert!(parse(button(json!({"click": {}}))).is_err());
        assert!(parse(button(json!({"click": {"output": "Bad Port"}}))).is_err());
        assert!(parse(button(json!({"click": {"arg": "x"}}))).is_err());
        assert!(parse(button(json!({"hover": {"close": true}}))).is_err());
    }

    #[test]
    fn filter_by_must_name_a_textfield() {
        let list = |filter: &str| {
            json!({"root": column(json!([
                {"type": "textfield", "id": "q"},
                {"type": "checkbox", "id": "c", "label": "c"},
                {"type": "list", "id": "l", "items": [{"title": "a"}], "filter_by": filter}
            ]))})
        };
        assert!(parse(list("q")).is_ok());
        assert!(parse(list("c")).is_err());
        assert!(parse(list("nope")).is_err());
    }

    #[test]
    fn depth_and_count_are_limited() {
        let mut deep = json!({"type": "spacer"});
        for _ in 0..MAX_DEPTH + 1 {
            deep = column(json!([deep]));
        }
        assert!(parse(json!({"root": deep})).unwrap_err().contains("levels"));
        let mut deep = json!({"type": "spacer"});
        for _ in 0..MAX_DEPTH - 1 {
            deep = column(json!([deep]));
        }
        assert!(parse(json!({"root": deep})).is_ok());

        let many: Vec<Value> = (0..MAX_COMPONENTS)
            .map(|_| json!({"type": "spacer"}))
            .collect();
        assert!(parse(json!({"root": column(Value::Array(many))}))
            .unwrap_err()
            .contains("at most"));
    }

    #[test]
    fn images_never_come_from_the_network() {
        for path in [
            "http://x/a.png",
            "data:image/png;base64,AA",
            "//x/a.png",
            "a.svg",
        ] {
            assert!(
                parse(json!({"root": {"type": "image", "path": path}})).is_err(),
                "{path}"
            );
        }
        assert!(parse(json!({"root": {"type": "image", "path": "img/a.PNG"}})).is_ok());
    }

    #[test]
    fn templates_expand_with_params() {
        let ui = parse(json!({
            "templates": {"field": {"type": "column", "children": [
                {"type": "label", "text": "{param:title}: {state:{param:id}} ({param:n})"},
                {"type": "textfield", "id": "{param:id}"}
            ]}},
            "root": column(json!([
                {"type": "use", "template": "field", "params": {"title": "Name", "id": "name", "n": 3}},
                {"type": "use", "template": "field", "params": {"title": "City", "id": "city", "n": true}}
            ]))
        }))
        .unwrap();
        let Component::Column(layout) = &ui.root else {
            panic!()
        };
        let Component::Column(first) = &layout.children[0] else {
            panic!()
        };
        assert_eq!(
            first.children[0],
            Component::Label {
                text: "Name: {state:name} (3)".into(),
                style: LabelStyle::Body
            }
        );
        let Component::Column(second) = &layout.children[1] else {
            panic!()
        };
        let Component::Label { text, .. } = &second.children[0] else {
            panic!()
        };
        assert_eq!(text, "City: {state:city} (true)");
    }

    #[test]
    fn a_whole_placeholder_keeps_its_json_type_and_unknown_ones_stay() {
        let params: Map<String, Value> = serde_json::from_value(json!({"n": 5, "s": "x"})).unwrap();
        assert_eq!(substitute_params(json!("{param:n}"), &params), json!(5));
        assert_eq!(
            substitute_params(json!("a{param:n}b{param:s}"), &params),
            json!("a5bx")
        );
        assert_eq!(
            substitute_params(json!("{param:zz}"), &params),
            json!("{param:zz}")
        );
        assert_eq!(
            substitute_params(json!("{param:zz} {param:s}"), &params),
            json!("{param:zz} x")
        );
        assert_eq!(
            substitute_params(json!("{param:s"), &params),
            json!("{param:s")
        );
        assert_eq!(
            substitute_params(json!({"a": ["{param:n}"]}), &params),
            json!({"a": [5]})
        );
    }

    #[test]
    fn templates_cannot_recurse_forever_or_explode() {
        let err = parse(json!({
            "templates": {"a": {"type": "use", "template": "a"}},
            "root": {"type": "use", "template": "a"}
        }))
        .unwrap_err();
        assert!(err.contains("levels"), "{err}");
        assert!(parse(json!({"root": {"type": "use", "template": "nope"}}))
            .unwrap_err()
            .contains("does not exist"));
        // Ten levels of ten uses each would be 10^10 components.
        let mut templates = Map::new();
        for level in 0..10 {
            let next = format!("t{}", level + 1);
            let uses: Vec<Value> = (0..10)
                .map(|_| json!({"type": "use", "template": next}))
                .collect();
            templates.insert(format!("t{level}"), column(Value::Array(uses)));
        }
        templates.insert("t10".into(), json!({"type": "spacer"}));
        assert!(
            parse(json!({"templates": templates, "root": {"type": "use", "template": "t0"}}))
                .unwrap_err()
                .contains("too large")
        );
    }

    #[test]
    fn includes_stay_inside_the_workflow_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("part.json"),
            r#"{"type": "label", "text": "from file"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("main.json"),
            r#"{"root": "{file:part.json}"}"#,
        )
        .unwrap();
        let source = Source {
            inline: None,
            file: Some("main.json"),
            dir: dir.path(),
        };
        let ui = load(&source, &BTreeSet::new()).unwrap();
        assert_eq!(
            ui.root,
            Component::Label {
                text: "from file".into(),
                style: LabelStyle::Body
            }
        );

        for bad in ["../x.json", "/etc/passwd", "sub/../../x.json"] {
            let inline = format!(r#"{{"root": "{{file:{bad}}}"}}"#);
            let source = Source {
                inline: Some(&inline),
                file: None,
                dir: dir.path(),
            };
            let err = load(&source, &BTreeSet::new()).unwrap_err();
            assert!(err.contains("inside the workflow folder"), "{bad}: {err}");
        }
        let source = Source {
            inline: None,
            file: Some("../main.json"),
            dir: dir.path(),
        };
        assert!(load(&source, &BTreeSet::new()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.json"), r#"{"type": "spacer"}"#).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.json"),
            dir.path().join("link.json"),
        )
        .unwrap();
        let source = Source {
            inline: Some(r#"{"root": "{file:link.json}"}"#),
            file: None,
            dir: dir.path(),
        };
        assert!(load(&source, &BTreeSet::new())
            .unwrap_err()
            .contains("outside"));
    }

    #[test]
    fn include_files_lists_every_file_the_definition_pulls_in() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.json"), r#"{"x": "{file:b.json}"}"#).unwrap();
        std::fs::write(dir.path().join("b.json"), r#"{"x": "{file:a.json}"}"#).unwrap();
        std::fs::write(dir.path().join("top.json"), r#"{"root": "{file:a.json}"}"#).unwrap();
        let inline = r#"{"root": {"type": "column", "children": ["{file:c.json}"]}}"#;
        assert_eq!(
            include_files(Some(inline), Some("top.json"), dir.path()),
            ["a.json", "b.json", "c.json", "top.json"]
        );
        assert!(include_files(None, None, dir.path()).is_empty());
    }

    #[test]
    fn nested_templates_with_large_params_hit_the_byte_budget() {
        let big = "x".repeat(200_000);
        let mut templates = Map::new();
        for level in 0..3 {
            let uses: Vec<Value> = (0..10)
                .map(|_| {
                    json!({"type": "use", "template": format!("t{}", level + 1),
                           "params": {"p": "{param:p}"}})
                })
                .collect();
            let mut children = uses;
            children.push(json!({"type": "label", "text": "{param:p}"}));
            templates.insert(format!("t{level}"), column(Value::Array(children)));
        }
        templates.insert("t3".into(), json!({"type": "label", "text": "{param:p}"}));
        let err = parse(json!({
            "templates": templates,
            "root": {"type": "use", "template": "t0", "params": {"p": big}}
        }))
        .unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn includes_that_loop_or_grow_too_large_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.json"), r#""{file:a.json}""#).unwrap();
        let source = Source {
            inline: Some(r#"{"root": "{file:a.json}"}"#),
            file: None,
            dir: dir.path(),
        };
        assert!(load(&source, &BTreeSet::new())
            .unwrap_err()
            .contains("too deeply"));

        let big = format!(
            r#"{{"root": {{"type": "label", "text": "{}"}}}}"#,
            "x".repeat(MAX_DEFINITION_BYTES)
        );
        let source = Source {
            inline: Some(&big),
            file: None,
            dir: dir.path(),
        };
        assert!(load(&source, &BTreeSet::new())
            .unwrap_err()
            .contains("larger"));
    }

    #[test]
    fn no_source_and_bad_json_are_clear_errors() {
        let dir = Path::new(".");
        let none = Source {
            inline: Some("  "),
            file: None,
            dir,
        };
        assert!(load(&none, &BTreeSet::new())
            .unwrap_err()
            .contains("no definition"));
        let bad = Source {
            inline: Some("{oops"),
            file: None,
            dir,
        };
        assert!(load(&bad, &BTreeSet::new())
            .unwrap_err()
            .contains("not valid JSON"));
    }

    #[test]
    fn ports_are_the_outputs_the_definition_names() {
        let inline = serde_json::to_string(&json!({"root": column(json!([
            {"type": "button", "label": "a", "events": {"click": [{"output": "save"}, {"output": "closed"}]}},
            {"type": "button", "label": "b", "events": {"click": {"output": "skip"}}},
            {"type": "button", "label": "c", "events": {"click": {"output": "save"}}},
            {"type": "button", "label": "d", "events": {"click": {"run": "anything"}}}
        ]))}))
        .unwrap();
        assert_eq!(node_ports(Some(&inline), &[]), ["save", "skip", CLOSED]);
        // A file-based UI declares what it uses; junk is ignored.
        assert_eq!(
            node_ports(
                None,
                &[
                    "next".into(),
                    "Bad Port".into(),
                    "closed".into(),
                    "next".into()
                ]
            ),
            ["next", CLOSED]
        );
        assert_eq!(node_ports(Some("{oops"), &[]), [CLOSED]);
    }

    #[test]
    fn script_replies_parse_strictly() {
        assert_eq!(parse_reply("  ").unwrap(), ScriptReply::default());
        let reply =
            parse_reply(r#"{"set": {"out": "HI"}, "notify": "done", "close": true}"#).unwrap();
        assert_eq!(reply.set["out"], json!("HI"));
        assert_eq!(reply.notify.as_deref(), Some("done"));
        assert!(reply.close);
        assert!(parse_reply("hello").is_err());
        assert!(parse_reply(r#"{"unknown": 1}"#).is_err());
    }

    #[test]
    fn a_live_root_uses_the_templates_and_scripts_of_the_ui() {
        let ui = parse(json!({
            "templates": {"t": {"type": "label", "text": "{param:x}"}},
            "root": {"type": "spacer"}
        }))
        .unwrap();
        let root = replacement_root(
            json!({"type": "use", "template": "t", "params": {"x": "hi"}}),
            &ui,
            &scripts(&[]),
        )
        .unwrap();
        assert_eq!(
            root,
            Component::Label {
                text: "hi".into(),
                style: LabelStyle::Body
            }
        );
        assert!(replacement_root(
            json!({"type": "button", "label": "b", "events": {"click": {"run": "ghost"}}}),
            &ui,
            &scripts(&[])
        )
        .is_err());
    }

    #[test]
    fn images_load_as_data_urls_and_only_real_ones() {
        let dir = tempfile::tempdir().unwrap();
        let png = b"\x89PNG\r\n\x1a\n0000";
        std::fs::write(dir.path().join("a.png"), png).unwrap();
        std::fs::write(dir.path().join("fake.png"), b"not an image").unwrap();
        std::fs::write(
            dir.path().join("big.png"),
            vec![0u8; MAX_IMAGE_BYTES as usize + 1],
        )
        .unwrap();
        let root = column(json!([
            {"type": "image", "path": "a.png"},
            {"type": "image", "path": "fake.png"},
            {"type": "image", "path": "big.png"},
            {"type": "image", "path": "missing.png"},
            {"type": "image", "path": "../a.png"}
        ]));
        let ui = parse(json!({"root": root})).unwrap();
        let (images, problems) = load_images(&ui.root, dir.path());
        assert_eq!(images.len(), 1);
        assert!(images["a.png"].starts_with("data:image/png;base64,"));
        assert_eq!(problems.len(), 4, "{problems:?}");
    }

    #[test]
    fn languages_map_to_script_extensions() {
        assert_eq!(language_extension("Python"), Some("py"));
        assert_eq!(language_extension("node"), Some("js"));
        assert_eq!(language_extension("bash"), Some("sh"));
        assert_eq!(language_extension("ruby"), None);
    }
}

#[cfg(test)]
mod node_tests {
    use super::*;
    use crate::workflow::model::{NodeKind, Workflow};
    use crate::workflow::testing::{copy, keyword, node, wire, wire_port};
    use serde_json::json;

    fn kind(definition: Value, scripts: &[&str]) -> NodeKind {
        NodeKind::ShowUi {
            target: UiTarget::Window,
            definition: Some(definition.to_string()),
            file: None,
            scripts: scripts
                .iter()
                .map(|id| {
                    (
                        (*id).to_owned(),
                        UiScript {
                            language: "shell".into(),
                            script: "true".into(),
                        },
                    )
                })
                .collect(),
            ports: Vec::new(),
            titlebar: true,
            always_on_top: false,
        }
    }

    fn two_buttons() -> Value {
        json!({"root": {"type": "row", "children": [
            {"type": "button", "label": "A", "events": {"click": {"output": "yes"}}},
            {"type": "button", "label": "B", "events": {"click": {"output": "no"}}}
        ]}})
    }

    #[test]
    fn only_a_ui_with_scripts_needs_approval() {
        assert!(!kind(two_buttons(), &[]).needs_approval());
        assert!(kind(two_buttons(), &["go"]).needs_approval());
    }

    #[test]
    fn the_nodes_ports_follow_its_definition() {
        assert_eq!(kind(two_buttons(), &[]).ports(), ["yes", "no", CLOSED]);
        assert_eq!(kind(two_buttons(), &[]).output_ports(), [CLOSED]);
    }

    #[test]
    fn connections_can_only_leave_ports_the_definition_names() {
        let build = |port: &str| Workflow {
            name: "w".into(),
            nodes: vec![
                keyword("k"),
                node("ui", kind(two_buttons(), &[])),
                copy("c", "x"),
            ],
            connections: vec![wire("k", "ui"), wire_port("ui", port, "c")],
            ..Workflow::default()
        };
        assert!(build("yes").is_valid(), "{:?}", build("yes").validate());
        assert!(build("closed").is_valid());
        let problems = build("maybe").validate();
        assert!(
            problems
                .iter()
                .any(|p| p.message.contains("no output \"maybe\"")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_bad_definition_or_script_is_a_problem_of_the_node() {
        let check = |kind: NodeKind| {
            Workflow {
                name: "w".into(),
                nodes: vec![keyword("k"), node("ui", kind)],
                connections: vec![wire("k", "ui")],
                ..Workflow::default()
            }
            .validate()
            .into_iter()
            .filter(|p| p.is_error())
            .map(|p| p.message)
            .collect::<Vec<_>>()
        };
        assert!(check(kind(two_buttons(), &[])).is_empty());
        assert!(check(kind(
            json!({"root": {"type": "button", "label": "x",
            "events": {"click": {"run": "ghost"}}}}),
            &[]
        ))[0]
            .contains("ghost"));
        let mut bad_language = kind(two_buttons(), &["go"]);
        if let NodeKind::ShowUi { scripts, .. } = &mut bad_language {
            scripts.get_mut("go").unwrap().language = "cobol".into();
        }
        assert!(check(bad_language)[0].contains("unknown language"));
        let mut none = kind(two_buttons(), &[]);
        if let NodeKind::ShowUi { definition, .. } = &mut none {
            *definition = None;
        }
        assert!(check(none)[0].contains("write the UI"));
    }

    #[test]
    fn changing_an_included_file_changes_the_approval_key() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("top.json"),
            r#"{"root": "{file:deep.json}"}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("deep.json"), r#"{"type": "spacer"}"#).unwrap();
        let mut ui = kind(json!({}), &["go"]);
        if let NodeKind::ShowUi {
            definition, file, ..
        } = &mut ui
        {
            *definition = None;
            *file = Some("top.json".to_owned());
        }
        let workflow = Workflow {
            name: "w".into(),
            nodes: vec![keyword("k"), node("ui", ui)],
            connections: vec![wire("k", "ui")],
            ..Workflow::default()
        };
        let key = |dir: &std::path::Path| crate::workflow::approval_key(&workflow, dir);
        let before = key(dir.path());
        assert!(before.is_some());
        assert_eq!(before, key(dir.path()));
        std::fs::write(
            dir.path().join("deep.json"),
            r#"{"type": "label", "text": "x"}"#,
        )
        .unwrap();
        assert_ne!(before, key(dir.path()));
    }

    #[test]
    fn a_ui_node_round_trips_through_the_workflow_file() {
        let workflow = Workflow {
            name: "w".into(),
            nodes: vec![keyword("k"), node("ui", kind(two_buttons(), &["go"]))],
            connections: vec![wire("k", "ui")],
            ..Workflow::default()
        };
        let text = workflow.to_toml().unwrap();
        assert!(text.contains("type = \"show_ui\""), "{text}");
        assert_eq!(Workflow::from_toml(&text).unwrap().nodes, workflow.nodes);
    }
}
