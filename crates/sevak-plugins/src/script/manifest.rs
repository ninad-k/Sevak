//! `plugin.toml`: what a script plugin declares about itself.
//!
//! See `docs/plugins.md` ("External plugins") for the user-facing description
//! of every field. Parsing is strict about what Sevak needs (a keyword, a way
//! to run the script, a supported protocol) and lenient about everything else:
//! unknown keys are ignored so newer manifests still load on older Sevaks.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sevak_platform::process::{is_interpreter_variable, script_runner, ScriptRunner};

/// The protocol version this Sevak speaks (`protocol` in the manifest and in
/// the `initialize` message).
pub const PROTOCOL: u32 = 1;

/// Every script plugin id starts with this; it is also the family id, so
/// `[plugins] disabled = ["script"]` switches all script plugins off.
pub const ID_PREFIX: &str = "script:";

/// Name of the manifest file inside a plugin folder.
pub const MANIFEST_FILE: &str = "plugin.toml";

const DEFAULT_TIMEOUT_MS: u64 = 50;
const DEFAULT_HARD_TIMEOUT_MS: u64 = 3_000;
const DEFAULT_IDLE_SECS: u64 = 300;

/// How Sevak runs the script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// One long-lived process speaking newline-delimited JSON on stdin/stdout.
    #[default]
    Persistent,
    /// A fresh process per query, with the query as the last argument; stdout
    /// is one JSON document.
    Oneshot,
}

/// What a one-shot script prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// `{"items":[...]}` with the same items as the persistent protocol.
    #[default]
    Sevak,
    /// Alfred Script Filter JSON.
    Alfred,
    /// Alfred Script Filter JSON for a workflow's script filter node
    /// (`workflow/`): the items keep their raw `arg` and `variables` for the
    /// nodes that follow instead of becoming an action. Not a manifest value.
    #[serde(skip_deserializing)]
    AlfredWorkflow,
}

#[derive(Debug, Deserialize)]
struct Raw {
    protocol: Option<u32>,
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    keyword: Option<String>,
    global: Option<bool>,
    command: Option<Vec<String>>,
    script: Option<String>,
    /// Extra files in the plugin folder that are part of the approval.
    #[serde(default)]
    files: Vec<String>,
    /// Names of Sevak's own environment variables the script wants.
    #[serde(default)]
    inherit_env: Vec<String>,
    /// Extra things the script's results may do; see [`Capabilities`].
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    mode: Mode,
    #[serde(default)]
    format: Format,
    timeout_ms: Option<u64>,
    hard_timeout_ms: Option<u64>,
    idle_timeout_secs: Option<u64>,
}

/// Things a script's results may do only when the manifest asks for them with
/// `capabilities = [...]`. The Allow dialog lists them, and they are part of
/// what the approval covers (they are in `plugin.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// Results may start applications (`{"type":"launch"}`).
    pub launch: bool,
}

impl Capabilities {
    /// The capability names, for the Allow dialog.
    pub fn names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.launch {
            names.push("launch");
        }
        names
    }
}

/// How the manifest says to start the script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// `command = ["python", "main.py"]`: run exactly this, no shell.
    Command(Vec<String>),
    /// `script = "main.py"`: a file in the plugin folder; the interpreter is
    /// chosen from its extension.
    Script(String),
}

/// A validated `plugin.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub keyword: String,
    pub launch: Launch,
    /// Support files (modules the script imports, data it reads) that the
    /// approval also covers; plain paths inside the plugin folder.
    pub files: Vec<String>,
    /// Environment variables of Sevak's own that the script is given in
    /// addition to the small base set (shown in the Allow dialog).
    pub inherit_env: Vec<String>,
    /// What the script's results may do beyond the basic actions (shown in
    /// the Allow dialog).
    pub capabilities: Capabilities,
    pub mode: Mode,
    pub format: Format,
    /// How long a query waits for the script before the list is shown without
    /// it (the answer still arrives later).
    pub timeout: Duration,
    /// How long a request may stay unanswered before the script counts as hung.
    pub hard_timeout: Duration,
    /// Persistent mode: stop the process after this much inactivity.
    pub idle_timeout: Option<Duration>,
    /// Problems that do not stop the plugin from loading (ignored keys).
    pub warnings: Vec<String>,
}

impl Manifest {
    /// Reads and validates `<dir>/plugin.toml`. `dir` also names the plugin
    /// when the manifest gives no `id` or `name`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        Self::read(dir).map(|(manifest, _)| manifest)
    }

    /// [`Manifest::load`] together with the exact bytes of the file, which an
    /// approval is bound to.
    pub fn read(dir: &Path) -> Result<(Self, Vec<u8>), String> {
        let path = dir.join(MANIFEST_FILE);
        let bytes =
            std::fs::read(&path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let text = String::from_utf8(bytes.clone())
            .map_err(|_| format!("{} is not valid UTF-8 text", path.display()))?;
        let folder = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::parse(&text, &folder).map(|manifest| (manifest, bytes))
    }

    /// Validates manifest `text`; `folder` is the plugin folder's name.
    pub fn parse(text: &str, folder: &str) -> Result<Self, String> {
        let raw: Raw = toml::from_str(text).map_err(|err| err.to_string())?;
        let mut warnings = Vec::new();

        let protocol = raw
            .protocol
            .ok_or_else(|| format!("missing `protocol` (this Sevak speaks protocol {PROTOCOL})"))?;
        if protocol != PROTOCOL {
            return Err(format!(
                "needs protocol {protocol}, but this Sevak speaks protocol {PROTOCOL}"
            ));
        }

        let keyword = raw.keyword.unwrap_or_default().trim().to_owned();
        if keyword.is_empty() {
            return Err("missing `keyword`".to_owned());
        }
        if keyword.chars().any(char::is_whitespace) {
            return Err(format!("the keyword \"{keyword}\" cannot contain spaces"));
        }
        if raw.global == Some(true) {
            warnings.push(
                "`global = true` is ignored: script plugins only answer their keyword".to_owned(),
            );
        }

        let id = match raw.id.map(|id| id.trim().to_owned()) {
            Some(id) if !id.is_empty() => id,
            _ => format!("{ID_PREFIX}{}", folder.to_lowercase().replace(' ', "-")),
        };
        if !id.starts_with(ID_PREFIX) || id.len() == ID_PREFIX.len() {
            return Err(format!("the id \"{id}\" must start with \"{ID_PREFIX}\""));
        }
        if id.chars().any(char::is_whitespace) {
            return Err(format!("the id \"{id}\" cannot contain spaces"));
        }

        let launch = match (raw.command, raw.script) {
            (Some(_), Some(_)) => return Err("set `command` or `script`, not both".to_owned()),
            (None, None) => return Err("missing `command` (or `script`)".to_owned()),
            (Some(command), None) => {
                if command
                    .first()
                    .is_none_or(|program| program.trim().is_empty())
                {
                    return Err("`command` must start with a program".to_owned());
                }
                Launch::Command(command)
            }
            (None, Some(script)) => {
                if script.trim().is_empty() {
                    return Err("`script` is empty".to_owned());
                }
                Launch::Script(script)
            }
        };

        let mut files = Vec::new();
        for file in raw.files {
            let file = file.trim().to_owned();
            if relative_inside(Path::new(""), &file).is_none() {
                return Err(format!(
                    "`files` may only name files inside the plugin folder, not \"{file}\""
                ));
            }
            files.push(file);
        }

        let inherit_env = check_inherit_env(raw.inherit_env)?;
        let mut capabilities = Capabilities::default();
        for name in &raw.capabilities {
            match name.trim() {
                "launch" => capabilities.launch = true,
                other => warnings.push(format!(
                    "the capability \"{other}\" is not known to this Sevak and is ignored"
                )),
            }
        }

        if raw.mode == Mode::Persistent && raw.format == Format::Alfred {
            return Err("`format = \"alfred\"` needs `mode = \"oneshot\"`".to_owned());
        }

        let name = match raw.name.map(|name| name.trim().to_owned()) {
            Some(name) if !name.is_empty() => name,
            _ if !folder.is_empty() => folder.to_owned(),
            _ => id.clone(),
        };

        let idle_secs = raw.idle_timeout_secs.unwrap_or(DEFAULT_IDLE_SECS);
        Ok(Self {
            id,
            name,
            description: raw.description.unwrap_or_default().trim().to_owned(),
            keyword,
            launch,
            files,
            inherit_env,
            capabilities,
            mode: raw.mode,
            format: raw.format,
            timeout: Duration::from_millis(
                raw.timeout_ms
                    .unwrap_or(DEFAULT_TIMEOUT_MS)
                    .clamp(10, 1_000),
            ),
            hard_timeout: Duration::from_millis(
                raw.hard_timeout_ms
                    .unwrap_or(DEFAULT_HARD_TIMEOUT_MS)
                    .clamp(500, 60_000),
            ),
            idle_timeout: (idle_secs > 0).then(|| Duration::from_secs(idle_secs.min(86_400))),
            warnings,
        })
    }

    /// The declared command in a human-readable form. It is what the user
    /// approves, and changing it asks for approval again.
    pub fn command_line(&self) -> String {
        match &self.launch {
            Launch::Script(script) => format!("script {}", quote(script)),
            Launch::Command(argv) => argv
                .iter()
                .map(|arg| quote(arg))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }

    /// The files inside the plugin folder that the approval covers: the script
    /// the manifest names, every argument of the command that is a plain
    /// relative path (`["python", "main.py"]` names `main.py`), and the
    /// manifest's `files`. A name that is not a file (a program on `PATH`) is
    /// harmless: it counts as "missing" in the hash.
    pub fn support_files(&self) -> Vec<String> {
        let mut files: Vec<String> = match &self.launch {
            Launch::Script(script) => vec![script.clone()],
            Launch::Command(argv) => argv
                .iter()
                .filter(|arg| relative_inside(Path::new(""), arg).is_some())
                .cloned()
                .collect(),
        };
        files.extend(self.files.iter().cloned());
        files.sort();
        files.dedup();
        files
    }

    /// The program and arguments to start, with relative paths resolved against
    /// the plugin folder `dir`. One-shot queries are appended by the caller.
    ///
    /// Programs without a path separator are looked up on `PATH` by the OS.
    pub fn resolve_argv(&self, dir: &Path) -> Result<Vec<String>, String> {
        resolve_launch(&self.launch, dir)
    }
}

/// The program and arguments for `launch`, with relative paths resolved
/// against `dir` (a plugin or workflow folder). Shared by script plugins and
/// the script nodes of workflows.
///
/// Programs without a path separator are looked up on `PATH` by the OS.
pub fn resolve_launch(launch: &Launch, dir: &Path) -> Result<Vec<String>, String> {
    match launch {
        Launch::Command(argv) => {
            let mut argv = argv.clone();
            let program = argv
                .first()
                .ok_or_else(|| "`command` must start with a program".to_owned())?;
            let has_path = program.contains(['/', '\\']) || program.starts_with('.');
            if has_path && !Path::new(program).is_absolute() {
                let resolved = relative_inside(dir, program)
                    .ok_or_else(|| format!("`{program}` points outside the plugin folder"))?;
                argv[0] = resolved.to_string_lossy().into_owned();
            }
            Ok(argv)
        }
        Launch::Script(script) => {
            let path = relative_inside(dir, script)
                .ok_or_else(|| format!("`{script}` points outside the plugin folder"))?;
            if !path.is_file() {
                return Err(format!("the script {script} does not exist"));
            }
            let extension = path
                .extension()
                .map(|ext| ext.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut argv = match script_runner(&extension) {
                ScriptRunner::Interpreter(prefix) => prefix,
                ScriptRunner::Direct => Vec::new(),
                ScriptRunner::Missing(names) => {
                    return Err(format!(
                        "cannot run .{extension} scripts: none of {} is on PATH",
                        names.join(", ")
                    ))
                }
            };
            argv.push(path.to_string_lossy().into_owned());
            Ok(argv)
        }
    }
}

/// Most `inherit_env` names a manifest may list.
const MAX_INHERITED: usize = 32;

/// Validates `inherit_env`: plain variable names, no interpreter hooks.
fn check_inherit_env(names: Vec<String>) -> Result<Vec<String>, String> {
    if names.len() > MAX_INHERITED {
        return Err(format!(
            "`inherit_env` lists {} names; at most {MAX_INHERITED} are allowed",
            names.len()
        ));
    }
    let mut out = Vec::new();
    for name in names {
        let name = name.trim().to_owned();
        let mut chars = name.chars();
        let plain = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            && name.len() <= 64;
        if !plain {
            return Err(format!(
                "`inherit_env` may only list plain variable names, not \"{name}\""
            ));
        }
        if is_interpreter_variable(&name) {
            return Err(format!(
                "`inherit_env` cannot include {name}: it changes how programs start"
            ));
        }
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out.sort();
    Ok(out)
}

fn quote(arg: &str) -> String {
    if arg.is_empty() || arg.contains(char::is_whitespace) || arg.contains('"') {
        let mut out = String::from("\"");
        for ch in arg.chars() {
            if ch == '"' {
                out.push('\\');
            }
            let _ = out.write_char(ch);
        }
        out.push('"');
        out
    } else {
        arg.to_owned()
    }
}

/// `dir` joined with `relative`, provided `relative` is a plain relative path:
/// not absolute, no drive or root, and no `..` anywhere.
pub fn relative_inside(dir: &Path, relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    let plain = !relative.is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
    // A leading slash is not "absolute" to Windows' `Path::is_absolute`, but it
    // is a root component, which the check above already rejects.
    plain.then(|| dir.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str =
        "protocol = 1\nkeyword = \"hello\"\ncommand = [\"python\", \"main.py\"]\n";

    #[test]
    fn minimal_manifest_gets_defaults() {
        let m = Manifest::parse(MINIMAL, "My Hello").unwrap();
        assert_eq!(m.id, "script:my-hello");
        assert_eq!(m.name, "My Hello");
        assert_eq!(m.keyword, "hello");
        assert_eq!(m.mode, Mode::Persistent);
        assert_eq!(m.format, Format::Sevak);
        assert_eq!(m.timeout, Duration::from_millis(50));
        assert_eq!(m.hard_timeout, Duration::from_secs(3));
        assert_eq!(m.idle_timeout, Some(Duration::from_secs(300)));
        assert_eq!(m.command_line(), "python main.py");
        assert!(m.warnings.is_empty());
    }

    #[test]
    fn full_manifest_round_trips() {
        let text = r#"
            protocol = 1
            id = "script:weather"
            name = "Weather"
            description = "Shows the weather."
            keyword = "w"
            mode = "oneshot"
            format = "alfred"
            script = "run.py"
            timeout_ms = 120
            hard_timeout_ms = 8000
            idle_timeout_secs = 0
            some_future_key = true
        "#;
        let m = Manifest::parse(text, "ignored").unwrap();
        assert_eq!(m.id, "script:weather");
        assert_eq!(m.name, "Weather");
        assert_eq!(m.description, "Shows the weather.");
        assert_eq!(m.mode, Mode::Oneshot);
        assert_eq!(m.format, Format::Alfred);
        assert_eq!(m.launch, Launch::Script("run.py".into()));
        assert_eq!(m.timeout, Duration::from_millis(120));
        assert_eq!(m.hard_timeout, Duration::from_secs(8));
        assert_eq!(m.idle_timeout, None);
    }

    #[test]
    fn the_files_an_approval_covers() {
        let m = Manifest::parse(MINIMAL, "x").unwrap();
        assert_eq!(m.support_files(), ["main.py", "python"]);
        let text = format!("{MINIMAL}files = [\"lib/util.py\", \"main.py\"]\n");
        let m = Manifest::parse(&text, "x").unwrap();
        assert_eq!(m.support_files(), ["lib/util.py", "main.py", "python"]);
        let m =
            Manifest::parse("protocol = 1\nkeyword = \"a\"\nscript = \"run.py\"\n", "x").unwrap();
        assert_eq!(m.support_files(), ["run.py"]);
        // An absolute interpreter is not a file of the plugin.
        let m = Manifest::parse(
            "protocol = 1\nkeyword = \"a\"\ncommand = [\"/usr/bin/python3\", \"-u\", \"m.py\"]\n",
            "x",
        )
        .unwrap();
        assert_eq!(m.support_files(), ["-u", "m.py"]);
    }

    #[test]
    fn inherit_env_lists_plain_names_and_refuses_interpreter_hooks() {
        let text = format!(
            "{MINIMAL}inherit_env = [\"OPENAI_API_KEY\", \"HTTPS_PROXY\", \"OPENAI_API_KEY\"]\n"
        );
        let m = Manifest::parse(&text, "x").unwrap();
        assert_eq!(m.inherit_env, ["HTTPS_PROXY", "OPENAI_API_KEY"]);
        assert!(Manifest::parse(MINIMAL, "x")
            .unwrap()
            .inherit_env
            .is_empty());

        for bad in [
            "BASH_ENV",
            "node_options",
            "PYTHONPATH",
            "LD_PRELOAD",
            "PATH",
            "A=B",
            "a b",
            "1X",
        ] {
            let text = format!("{MINIMAL}inherit_env = [{bad:?}]\n");
            let err = Manifest::parse(&text, "x").expect_err(bad);
            assert!(err.contains("inherit_env"), "{err}");
        }
        let many: Vec<String> = (0..40).map(|n| format!("\"V{n}\"")).collect();
        let text = format!("{MINIMAL}inherit_env = [{}]\n", many.join(","));
        assert!(Manifest::parse(&text, "x").unwrap_err().contains("at most"));
    }

    #[test]
    fn capabilities_are_opt_in_and_unknown_ones_are_ignored_with_a_warning() {
        let m = Manifest::parse(MINIMAL, "x").unwrap();
        assert!(!m.capabilities.launch);
        assert!(m.capabilities.names().is_empty());

        let text = format!("{MINIMAL}capabilities = [\"launch\"]\n");
        let m = Manifest::parse(&text, "x").unwrap();
        assert!(m.capabilities.launch);
        assert_eq!(m.capabilities.names(), ["launch"]);

        let text = format!("{MINIMAL}capabilities = [\"teleport\"]\n");
        let m = Manifest::parse(&text, "x").unwrap();
        assert!(!m.capabilities.launch);
        assert!(m.warnings.iter().any(|w| w.contains("teleport")));
    }

    #[test]
    fn support_files_cannot_leave_the_plugin_folder() {
        for bad in ["../x.py", "/etc/passwd", "a/../../b", "C:\\\\x.py", ""] {
            let text = format!("{MINIMAL}files = [{bad:?}]\n");
            let err = Manifest::parse(&text, "x").expect_err(bad);
            assert!(err.contains("files"), "{err}");
        }
    }

    #[test]
    fn timeouts_are_clamped() {
        let text = format!("{MINIMAL}timeout_ms = 1\nhard_timeout_ms = 999999\n");
        let m = Manifest::parse(&text, "x").unwrap();
        assert_eq!(m.timeout, Duration::from_millis(10));
        assert_eq!(m.hard_timeout, Duration::from_secs(60));
        let text = format!("{MINIMAL}timeout_ms = 60000\n");
        assert_eq!(
            Manifest::parse(&text, "x").unwrap().timeout,
            Duration::from_secs(1)
        );
    }

    #[test]
    fn rejects_bad_manifests() {
        let cases = [
            ("keyword = \"a\"\ncommand = [\"x\"]\n", "protocol"),
            (
                "protocol = 2\nkeyword = \"a\"\ncommand = [\"x\"]\n",
                "protocol 2",
            ),
            ("protocol = 1\ncommand = [\"x\"]\n", "keyword"),
            (
                "protocol = 1\nkeyword = \"a b\"\ncommand = [\"x\"]\n",
                "spaces",
            ),
            ("protocol = 1\nkeyword = \"a\"\n", "command"),
            ("protocol = 1\nkeyword = \"a\"\ncommand = []\n", "program"),
            (
                "protocol = 1\nkeyword = \"a\"\ncommand = [\"x\"]\nscript = \"y\"\n",
                "not both",
            ),
            (
                "protocol = 1\nkeyword = \"a\"\ncommand = [\"x\"]\nid = \"hello\"\n",
                "script:",
            ),
            (
                "protocol = 1\nkeyword = \"a\"\ncommand = [\"x\"]\nformat = \"alfred\"\n",
                "oneshot",
            ),
            (
                "protocol = 1\nkeyword = \"a\"\ncommand = [\"x\"]\nmode = \"magic\"\n",
                "magic",
            ),
            ("this is not toml", "expected"),
        ];
        for (text, needle) in cases {
            let err = Manifest::parse(text, "x").expect_err(text);
            assert!(
                err.contains(needle),
                "{text:?} -> {err:?} (wanted {needle:?})"
            );
        }
    }

    #[test]
    fn global_is_ignored_with_a_warning() {
        let m = Manifest::parse(&format!("{MINIMAL}global = true\n"), "x").unwrap();
        assert_eq!(m.warnings.len(), 1);
        assert!(Manifest::parse(&format!("{MINIMAL}global = false\n"), "x")
            .unwrap()
            .warnings
            .is_empty());
    }

    #[test]
    fn command_line_quotes_arguments_with_spaces() {
        let m = Manifest::parse(
            "protocol = 1\nkeyword = \"a\"\ncommand = [\"node\", \"my file.js\"]\n",
            "x",
        )
        .unwrap();
        assert_eq!(m.command_line(), "node \"my file.js\"");
    }

    #[test]
    fn relative_inside_rejects_escapes() {
        let dir = Path::new("plugins").join("p");
        assert_eq!(
            relative_inside(&dir, "icons/a.png"),
            Some(dir.join("icons/a.png"))
        );
        assert_eq!(relative_inside(&dir, "./a.png"), Some(dir.join("./a.png")));
        for bad in ["", "../a.png", "a/../../b", "/etc/passwd"] {
            assert_eq!(relative_inside(&dir, bad), None, "{bad:?}");
        }
        if cfg!(windows) {
            assert_eq!(relative_inside(&dir, "\\windows"), None);
            assert_eq!(relative_inside(&dir, "..\\a.png"), None);
            assert_eq!(relative_inside(&dir, "C:\\a.png"), None);
        }
    }

    #[test]
    fn commands_resolve_against_the_plugin_folder() {
        let dir = tempfile::tempdir().unwrap();
        let parse = |command: &str| {
            Manifest::parse(
                &format!("protocol = 1\nkeyword = \"a\"\ncommand = {command}\n"),
                "x",
            )
            .unwrap()
        };
        // A bare program name is left for the OS to find on PATH.
        assert_eq!(
            parse("[\"python\", \"main.py\"]")
                .resolve_argv(dir.path())
                .unwrap(),
            ["python", "main.py"]
        );
        // A relative path is anchored in the plugin folder.
        let argv = parse("[\"./run\"]").resolve_argv(dir.path()).unwrap();
        assert_eq!(Path::new(&argv[0]), dir.path().join("./run"));
        assert!(parse("[\"../run\"]").resolve_argv(dir.path()).is_err());
    }

    #[test]
    fn script_shorthand_needs_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let m =
            Manifest::parse("protocol = 1\nkeyword = \"a\"\nscript = \"run.cmd\"\n", "x").unwrap();
        assert!(m
            .resolve_argv(dir.path())
            .unwrap_err()
            .contains("does not exist"));
        std::fs::write(dir.path().join("run.cmd"), "").unwrap();
        // `.cmd` needs no interpreter: the file itself is started.
        assert_eq!(
            m.resolve_argv(dir.path()).unwrap(),
            [dir.path().join("run.cmd").to_string_lossy()]
        );
        let escape =
            Manifest::parse("protocol = 1\nkeyword = \"a\"\nscript = \"../x.sh\"\n", "x").unwrap();
        assert!(escape.resolve_argv(dir.path()).is_err());
    }
}
