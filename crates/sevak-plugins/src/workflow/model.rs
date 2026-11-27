//! The workflow file: nodes, connections and how they are stored.
//!
//! A workflow is `<config dir>/workflows/<folder>/workflow.toml`:
//!
//! ```toml
//! format = 1
//! name = "Search docs"
//!
//! [[node]]
//! id = "trigger"
//! type = "keyword"
//! keyword = "docs"
//! argument = "required"
//!
//! [[node]]
//! id = "open"
//! type = "open_url"
//! url = "https://example.com/search?q={query}"
//!
//! [[connection]]
//! from = "trigger"
//! to = "open"
//! ```
//!
//! The same types are what the settings builder edits (as JSON), so every
//! field is plain data with serde defaults: a hand-written file may leave out
//! anything but `id`, `type` and the fields a node cannot work without.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ui::{UiScript, UiTarget};

/// The workflow file format this Sevak reads and writes.
pub const FORMAT: u32 = 1;
/// Name of the workflow file inside a workflow folder.
pub const FILE: &str = "workflow.toml";
/// Every workflow plugin id starts with this; it is also the family id, so
/// `[plugins] disabled = ["workflow"]` switches all workflows off.
pub const FAMILY: &str = "workflow";
/// The output port of every node except [`NodeKind::Conditional`] and
/// [`NodeKind::DialogConditional`].
pub const OUT: &str = "out";
/// The branch of a conditional taken when its test holds.
pub const THEN: &str = "then";
/// The branch of a conditional taken when its test fails.
pub const ELSE: &str = "else";

/// Most nodes and connections one workflow may have.
pub const MAX_NODES: usize = 200;
pub const MAX_CONNECTIONS: usize = 500;
/// Longest pause one `delay` node may take.
pub const MAX_DELAY_MS: u64 = 60_000;
/// Time limits of a script node, in milliseconds.
pub const DEFAULT_SCRIPT_TIMEOUT_MS: u64 = 10_000;
pub const MAX_SCRIPT_TIMEOUT_MS: u64 = 300_000;

/// A whole workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Workflow {
    /// The file format version ([`FORMAT`]).
    pub format: u32,
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    /// `false` keeps the workflow installed but inactive.
    pub enabled: bool,
    /// Values every run starts with (`{var:name}`; also environment variables
    /// of the script nodes).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
    #[serde(rename = "node", skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<Node>,
    #[serde(rename = "connection", skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<Connection>,
}

impl Default for Workflow {
    fn default() -> Self {
        Self {
            format: FORMAT,
            name: String::new(),
            description: String::new(),
            author: String::new(),
            version: String::new(),
            enabled: true,
            variables: BTreeMap::new(),
            nodes: Vec::new(),
            connections: Vec::new(),
        }
    }
}

/// One box of the graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Unique within the workflow; `sevak --trigger <folder>/<id>` names an
    /// external trigger by it.
    pub id: String,
    /// What the node is called on the canvas and, for keyword triggers, in the
    /// result list. May use `{query}`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Position on the canvas in pixels. Layout only: it never affects what
    /// runs or what has to be approved.
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(flatten)]
    pub kind: NodeKind,
}

/// A wire from the output port `port` of `from` to the input of `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub from: String,
    #[serde(default = "out", skip_serializing_if = "is_out")]
    pub port: String,
    pub to: String,
}

fn out() -> String {
    OUT.to_owned()
}

fn is_out(port: &str) -> bool {
    port == OUT
}

/// Whether a keyword trigger takes text after the keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Argument {
    /// Only the keyword itself.
    None,
    /// The keyword, with or without text.
    #[default]
    Optional,
    /// The keyword followed by some text.
    Required,
}

/// The kinds of selection a Universal Actions trigger accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accepts {
    /// Any selected text (a URL is text too).
    Text,
    /// A selection that is only URLs.
    Url,
    /// Selected files and folders.
    File,
}

/// What a `transform` node does with its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformOp {
    /// The input as it is: sets the argument from a template.
    #[default]
    Set,
    Upper,
    Lower,
    Title,
    Trim,
    UrlEncode,
    UrlDecode,
    Base64Encode,
    Base64Decode,
    /// Every `find` becomes `replace`.
    Replace,
    /// Regular-expression `find` becomes `replace` (`$1` refers to a group).
    RegexReplace,
    FirstLine,
    /// Splits at `find` and takes the part number `index` (negative counts
    /// from the end).
    Split,
}

/// What a `conditional` node tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Test {
    #[default]
    Equals,
    NotEquals,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    /// The regular expression `right` matches somewhere in `left`.
    Matches,
    IsEmpty,
    NotEmpty,
}

/// Where a `split_arg` node cuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delimiter {
    #[default]
    Newline,
    Comma,
    Tab,
    Space,
    /// The text in the node's `custom` field.
    Custom,
}

/// What a `random` node produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RandomMode {
    /// A whole number from `min` to `max`, both included.
    #[default]
    Number,
    /// One line of `lines`.
    Line,
    /// A version 4 UUID.
    Uuid,
}

/// How a `write_file` node treats a file that already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    #[default]
    Overwrite,
    Append,
    /// The new text goes in front of what the file holds.
    Prepend,
}

fn yes() -> bool {
    true
}

fn split_prefix() -> String {
    "split_".to_owned()
}

fn join_separator() -> String {
    ", ".to_owned()
}

fn default_min() -> i64 {
    1
}

fn default_max() -> i64 {
    100
}
/// Which entries a `file_filter` node lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileScope {
    /// Files and folders.
    #[default]
    Any,
    Files,
    Folders,
}

/// What a `running_apps` node passes on for the app that was picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppOutput {
    #[default]
    Name,
    /// The application's path, when it can be found (else its name).
    Path,
    /// The process id of its first process.
    Pid,
}

/// One row of a `list_filter` node.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ListItem {
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subtitle: String,
    /// What the nodes after the filter receive; the title when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub arg: String,
}

/// Most buttons a `dialog_conditional` node shows.
pub const MAX_DIALOG_BUTTONS: usize = 3;
/// The output port taken when the dialog is dismissed (or cannot be shown).
pub const CANCEL: &str = "cancel";
/// The output ports of a `dialog_conditional`, one per button, then `cancel`.
pub const DIALOG_PORTS: [&str; 4] = ["button1", "button2", "button3", CANCEL];
/// The output port of a `show_ui` node taken when the UI is dismissed.
pub const UI_CLOSED: &str = super::ui::CLOSED;
/// Most rows of a `list_filter`.
pub const MAX_LIST_ITEMS: usize = 500;
/// File extensions a `play_sound` node accepts.
pub const SOUND_EXTENSIONS: [&str; 7] = ["wav", "mp3", "aiff", "aif", "m4a", "ogg", "flac"];

fn query_template() -> String {
    "{query}".to_owned()
}

fn is_default_timeout(timeout: &Option<u64>) -> bool {
    timeout.is_none()
}

/// The behavior of a node. Serialized with a `type` tag next to the node's own
/// fields; field names avoid `id`, `type`, `title`, `x` and `y`, which belong
/// to [`Node`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeKind {
    // ---- triggers ---------------------------------------------------------
    /// Typing `<keyword> <text>` offers the workflow as a result.
    Keyword {
        keyword: String,
        #[serde(default)]
        argument: Argument,
        /// The row's second line; may use `{query}`.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        subtitle: String,
    },
    /// A global shortcut (the same syntax as `[[hotkey]]` entries).
    Hotkey { key: String },
    /// An entry in the Universal Actions panel for the selected text, links or
    /// files.
    Selection {
        #[serde(default = "default_accepts")]
        accepts: Vec<Accepts>,
    },
    /// Started from the command line: `sevak --trigger <folder>/<id> [text]`.
    External {},

    // ---- inputs -----------------------------------------------------------
    /// A keyword whose results come from a script (Alfred Script Filter JSON);
    /// the row you pick starts the nodes after it.
    ScriptFilter {
        keyword: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        command: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        script: Option<String>,
        /// Arguments before the typed text (used with `script`, or added to
        /// `command`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        /// How long a query waits for the script before the list is shown
        /// without it (10 to 1000 ms; default 50).
        #[serde(default, skip_serializing_if = "is_default_timeout")]
        timeout_ms: Option<u64>,
        /// How long one run of the script may take before it is stopped (500
        /// to 60 000 ms; default 3000).
        #[serde(default, skip_serializing_if = "is_default_timeout")]
        hard_timeout_ms: Option<u64>,
    },

    // ---- actions ----------------------------------------------------------
    /// Runs a program; its output becomes the argument.
    RunScript {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        command: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        script: Option<String>,
        /// Arguments after the script or command. May use `{query}`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        /// Text written to the program's standard input. May use `{query}`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stdin: Option<String>,
        /// Extra environment variables. Values may use `{query}`.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        env: BTreeMap<String, String>,
        /// Stop the program after this long (100 to 300 000 ms; default 10 000).
        #[serde(default, skip_serializing_if = "is_default_timeout")]
        timeout_ms: Option<u64>,
        /// Write the start of the program's error output to Sevak's log. Off
        /// by default because it can contain your data.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        log_stderr: bool,
    },
    /// Opens a web or mail link. Placeholders are URL-encoded.
    OpenUrl { url: String },
    /// Opens a file or folder with its default program.
    OpenFile { path: String },
    /// Starts an application by name or path.
    LaunchApp {
        app: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
    },
    /// Runs a system command: `lock`, `sleep`, `hibernate`, `restart`,
    /// `shutdown`, `logout` or `empty_trash`.
    SystemCommand { command: String },
    /// Runs a command line in a terminal window.
    TerminalCommand { command: String },
    /// Copies text to the clipboard.
    Copy {
        #[serde(default = "query_template")]
        text: String,
    },
    /// Pastes text into the app that was in front.
    Paste {
        #[serde(default = "query_template")]
        text: String,
        /// Put the clipboard's earlier text back (default: the `[paste]` setting).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_clipboard: Option<bool>,
    },

    // ---- utilities --------------------------------------------------------
    /// Sets a variable for the nodes after it.
    SetVariable { name: String, value: String },
    /// Changes the argument (or stores the result in a variable).
    Transform {
        #[serde(default)]
        op: TransformOp,
        #[serde(default = "query_template")]
        input: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        find: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        replace: String,
        /// For `split`: which part to keep.
        #[serde(default, skip_serializing_if = "is_zero")]
        index: i64,
        /// Store the result in this variable instead of replacing the argument.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        into: Option<String>,
    },
    /// Continues along `then` or `else`.
    Conditional {
        #[serde(default = "query_template")]
        left: String,
        #[serde(default)]
        test: Test,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        right: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ignore_case: bool,
    },
    /// Waits (at most [`MAX_DELAY_MS`]).
    Delay { ms: u64 },

    // ---- outputs ----------------------------------------------------------
    /// A system notification.
    Notification {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        heading: String,
        #[serde(default = "query_template")]
        body: String,
    },
    /// Shows text huge on screen.
    LargeType {
        #[serde(default = "query_template")]
        text: String,
    },
    /// Shows text in the launcher window.
    TextView {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        heading: String,
        #[serde(default = "query_template")]
        text: String,
        /// Draw the text as Markdown (headings, lists, tables, links...).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        markdown: bool,
    },

    // ---- more utilities and actions (Alfred parity) -----------------------
    /// Cuts the argument at a delimiter into a list (one item per line) and
    /// sets `<prefix>1..N` and `<prefix>count`.
    SplitArg {
        #[serde(default)]
        delimiter: Delimiter,
        /// The delimiter when `delimiter` is `custom`.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        custom: String,
        #[serde(default = "yes", skip_serializing_if = "is_true")]
        discard_empty: bool,
        #[serde(default = "yes", skip_serializing_if = "is_true")]
        trim: bool,
        #[serde(default = "split_prefix")]
        prefix: String,
    },
    /// Joins a list (lines, or a JSON array) with a separator (`\n` and `\t`
    /// escapes work).
    JoinArg {
        #[serde(default = "join_separator")]
        separator: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        discard_empty: bool,
    },
    /// A random number, line or UUID: the argument, or the variable `variable`.
    Random {
        #[serde(default)]
        mode: RandomMode,
        #[serde(default = "default_min")]
        min: i64,
        #[serde(default = "default_max")]
        max: i64,
        /// For `line`: the candidates, one per line. May use placeholders.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        lines: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        variable: Option<String>,
    },
    /// Calculates a math expression (placeholders are filled in first).
    Expression {
        #[serde(default = "query_template")]
        expression: String,
        /// The variable that receives the result.
        variable: String,
        /// Also make the result the argument.
        #[serde(default = "yes", skip_serializing_if = "is_true")]
        set_arg: bool,
    },
    /// Writes text to a file. Relative paths are inside the workflow's data
    /// folder.
    WriteFile {
        path: String,
        #[serde(default = "query_template")]
        content: String,
        #[serde(default)]
        mode: WriteMode,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        create_dirs: bool,
    },
    /// Shows the argument and variables in the launcher window.
    Debug {
        /// `false` empties the argument for the nodes after it.
        #[serde(default = "yes", skip_serializing_if = "is_true")]
        passthrough: bool,
    },
    /// Shows each file or folder (one per line) in the file manager.
    RevealFile {
        #[serde(default = "query_template")]
        path: String,
    },
    /// Runs the `external` trigger `trigger` of another workflow.
    CallExternal {
        /// The other workflow's folder.
        workflow: String,
        trigger: String,
        #[serde(default = "query_template")]
        arg: String,
        /// Hand the current variables to the other workflow.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        pass_variables: bool,
    },

    // ---- Alfred parity: filters, dialog, speech and sound -------------------
    /// A keyword that lists fixed rows (edited in the builder), filtered by the
    /// typed text; the row you pick starts the nodes after it.
    ListFilter {
        keyword: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        items: Vec<ListItem>,
    },
    /// A keyword that lists files and folders by name; the path you pick
    /// starts the nodes after it.
    FileFilter {
        keyword: String,
        /// Folders to look in (`~` is the home folder); the `[files]
        /// directories` when empty.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        folders: Vec<String>,
        /// Only names with one of these extensions (`pdf`, `.png`); any when
        /// empty.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        extensions: Vec<String>,
        #[serde(default, skip_serializing_if = "is_any_scope")]
        scope: FileScope,
    },
    /// A keyword that lists the running apps; the one you pick starts the
    /// nodes after it.
    RunningApps {
        keyword: String,
        #[serde(default, skip_serializing_if = "is_default_output")]
        output: AppOutput,
    },
    /// Shows a dialog and continues along the port of the button pressed
    /// (`button1` to `button3`), or `cancel`.
    DialogConditional {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        heading: String,
        #[serde(default = "query_template")]
        message: String,
        /// One to three button labels. May use `{query}`.
        #[serde(default)]
        buttons: Vec<String>,
    },
    /// Speaks text with the system voice.
    Speak {
        #[serde(default = "query_template")]
        text: String,
        /// The voice's name; the system's own when empty.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        voice: Option<String>,
    },
    /// Plays a sound file (`wav`, `mp3`, `aiff`, `aif`, `m4a`, `ogg`, `flac`).
    PlaySound { path: String },
    /// Opens a Sevak UI (see `ui.rs`) and waits for it. Continues along the
    /// port an `output` action names, or `closed` when it is dismissed.
    ShowUi {
        #[serde(default)]
        target: UiTarget,
        /// The UI as inline JSON.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition: Option<String>,
        /// Or a JSON file in the workflow folder.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        /// Scripts the UI's `run` actions may start. Part of what the user
        /// approves.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        scripts: BTreeMap<String, UiScript>,
        /// Output ports a `file` definition uses (an inline definition is
        /// read for them).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        ports: Vec<String>,
        /// A window's title bar; off for a bare window.
        #[serde(default = "yes", skip_serializing_if = "is_true")]
        titlebar: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        always_on_top: bool,
    },
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_any_scope(scope: &FileScope) -> bool {
    *scope == FileScope::Any
}

fn is_default_output(output: &AppOutput) -> bool {
    *output == AppOutput::Name
}

fn default_accepts() -> Vec<Accepts> {
    vec![Accepts::Text, Accepts::Url, Accepts::File]
}

fn is_zero(value: &i64) -> bool {
    *value == 0
}

/// How a node takes part in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Trigger,
    Input,
    Action,
    Utility,
    Output,
}

impl NodeKind {
    /// The `type` value in the file.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Keyword { .. } => "keyword",
            Self::Hotkey { .. } => "hotkey",
            Self::Selection { .. } => "selection",
            Self::External {} => "external",
            Self::ScriptFilter { .. } => "script_filter",
            Self::RunScript { .. } => "run_script",
            Self::OpenUrl { .. } => "open_url",
            Self::OpenFile { .. } => "open_file",
            Self::LaunchApp { .. } => "launch_app",
            Self::SystemCommand { .. } => "system_command",
            Self::TerminalCommand { .. } => "terminal_command",
            Self::Copy { .. } => "copy",
            Self::Paste { .. } => "paste",
            Self::SetVariable { .. } => "set_variable",
            Self::Transform { .. } => "transform",
            Self::Conditional { .. } => "conditional",
            Self::Delay { .. } => "delay",
            Self::Notification { .. } => "notification",
            Self::LargeType { .. } => "large_type",
            Self::TextView { .. } => "text_view",
            Self::SplitArg { .. } => "split_arg",
            Self::JoinArg { .. } => "join_arg",
            Self::Random { .. } => "random",
            Self::Expression { .. } => "expression",
            Self::WriteFile { .. } => "write_file",
            Self::Debug { .. } => "debug",
            Self::RevealFile { .. } => "reveal_file",
            Self::CallExternal { .. } => "call_external",
            Self::ListFilter { .. } => "list_filter",
            Self::FileFilter { .. } => "file_filter",
            Self::RunningApps { .. } => "running_apps",
            Self::DialogConditional { .. } => "dialog_conditional",
            Self::Speak { .. } => "speak",
            Self::PlaySound { .. } => "play_sound",
            Self::ShowUi { .. } => "show_ui",
        }
    }

    pub fn category(&self) -> Category {
        match self {
            Self::Keyword { .. }
            | Self::Hotkey { .. }
            | Self::Selection { .. }
            | Self::External {} => Category::Trigger,
            Self::ScriptFilter { .. }
            | Self::ListFilter { .. }
            | Self::FileFilter { .. }
            | Self::RunningApps { .. } => Category::Input,
            Self::RunScript { .. }
            | Self::OpenUrl { .. }
            | Self::OpenFile { .. }
            | Self::LaunchApp { .. }
            | Self::SystemCommand { .. }
            | Self::TerminalCommand { .. }
            | Self::Copy { .. }
            | Self::Paste { .. }
            | Self::WriteFile { .. }
            | Self::RevealFile { .. }
            | Self::CallExternal { .. } => Category::Action,
            Self::SetVariable { .. }
            | Self::Transform { .. }
            | Self::Conditional { .. }
            | Self::Delay { .. }
            | Self::DialogConditional { .. }
            | Self::SplitArg { .. }
            | Self::JoinArg { .. }
            | Self::Random { .. }
            | Self::Expression { .. }
            | Self::Debug { .. } => Category::Utility,
            Self::Notification { .. }
            | Self::LargeType { .. }
            | Self::TextView { .. }
            | Self::Speak { .. }
            | Self::PlaySound { .. }
            | Self::ShowUi { .. } => Category::Output,
        }
    }

    /// Whether a run can start at this node.
    pub fn is_start(&self) -> bool {
        matches!(self.category(), Category::Trigger | Category::Input)
    }

    /// The output ports, in the order the builder draws them.
    pub fn output_ports(&self) -> &'static [&'static str] {
        match self {
            Self::Conditional { .. } => &[THEN, ELSE],
            Self::DialogConditional { .. } => &DIALOG_PORTS,
            Self::ShowUi { .. } => &[UI_CLOSED],
            _ => &[OUT],
        }
    }

    /// The output ports, including those that depend on the node's own
    /// settings (a `show_ui` node has the ones its definition names).
    pub fn ports(&self) -> Vec<String> {
        match self {
            Self::ShowUi {
                definition, ports, ..
            } => super::ui::node_ports(definition.as_deref(), ports),
            other => other
                .output_ports()
                .iter()
                .map(|p| (*p).to_owned())
                .collect(),
        }
    }

    /// Whether the node runs code or commands, or acts in other apps, so a
    /// workflow containing it has to be allowed by the user first. `OpenFile`
    /// counts: it hands the file to the system's default handler, which runs a
    /// program, script or shortcut, and a relative path reaches files shipped
    /// inside the workflow's folder. `Paste` counts: it types text into
    /// whatever app was in front (a terminal, a chat, a form), which is the
    /// same as the user typing it. `WriteFile` counts: it changes files on the
    /// disk and can replace one the user cares about. `CallExternal` counts:
    /// it runs another workflow, so the target, trigger and argument are part
    /// of what the user approves.
    pub fn needs_approval(&self) -> bool {
        matches!(
            self,
            Self::ScriptFilter { .. }
                | Self::RunScript { .. }
                | Self::LaunchApp { .. }
                | Self::SystemCommand { .. }
                | Self::TerminalCommand { .. }
                | Self::OpenFile { .. }
                | Self::Paste { .. }
                | Self::WriteFile { .. }
                | Self::CallExternal { .. }
        ) || matches!(self, Self::ShowUi { scripts, .. } if !scripts.is_empty())
    }
}

impl Workflow {
    /// Parses a `workflow.toml`.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let workflow: Self = toml::from_str(text).map_err(|err| err.to_string())?;
        if workflow.format != FORMAT {
            return Err(format!(
                "needs workflow format {}, but this Sevak reads format {FORMAT}",
                workflow.format
            ));
        }
        Ok(workflow)
    }

    /// The file contents for this workflow.
    pub fn to_toml(&self) -> Result<String, String> {
        let text = toml::to_string_pretty(self).map_err(|err| err.to_string())?;
        Ok(format!(
            "# Written by Sevak's workflow builder (Settings > Workflows).\n\
             # Edit it there, or by hand: https://github.com/ninad-k/Sevak/blob/main/docs/workflows.md\n{text}"
        ))
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// The nodes that answer a typed keyword (keyword triggers and script
    /// filters) with that keyword, trimmed.
    pub fn keyword_nodes(&self) -> impl Iterator<Item = (&Node, &str)> {
        self.nodes.iter().filter_map(|node| match &node.kind {
            NodeKind::Keyword { keyword, .. }
            | NodeKind::ScriptFilter { keyword, .. }
            | NodeKind::ListFilter { keyword, .. }
            | NodeKind::FileFilter { keyword, .. }
            | NodeKind::RunningApps { keyword, .. } => Some((node, keyword.trim())),
            _ => None,
        })
    }

    /// The keywords of [`Workflow::keyword_nodes`].
    pub fn keywords(&self) -> impl Iterator<Item = &str> {
        self.keyword_nodes().map(|(_, keyword)| keyword)
    }

    /// The connections leaving port `port` of node `from`, in file order (the
    /// order the nodes after it run in).
    pub fn outgoing<'a>(
        &'a self,
        from: &'a str,
        port: &'a str,
    ) -> impl Iterator<Item = &'a Connection> + 'a {
        self.connections
            .iter()
            .filter(move |conn| conn.from == from && conn.port == port)
    }

    /// Whether any node needs the user's approval before the workflow runs.
    pub fn needs_approval(&self) -> bool {
        self.nodes.iter().any(|node| node.kind.needs_approval())
    }
}

/// A folder name for a workflow called `name`: lower case letters, digits and
/// dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    let trimmed = out.trim_end_matches('-');
    let mut slug: String = trimmed.chars().take(48).collect();
    if slug.is_empty() {
        slug.push_str("workflow");
    }
    slug
}

/// Whether `folder` is a safe single folder name below the workflows folder.
pub fn valid_folder_name(folder: &str) -> bool {
    !folder.is_empty()
        && folder.len() <= 64
        && !folder.starts_with('.')
        && folder
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
        && !folder.ends_with(' ')
        && !folder.ends_with('.')
        && !sevak_core::safe_names::is_reserved_device_name(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        format = 1
        name = "Docs"
        description = "Search the docs"

        [variables]
        site = "https://example.com"

        [[node]]
        id = "t"
        type = "keyword"
        keyword = "docs"
        argument = "required"
        x = 40
        y = 60.5

        [[node]]
        id = "c"
        type = "conditional"
        left = "{query}"
        test = "matches"
        right = "^[0-9]+$"

        [[node]]
        id = "open"
        type = "open_url"
        url = "{var:site}/{query}"

        [[connection]]
        from = "t"
        to = "c"

        [[connection]]
        from = "c"
        port = "then"
        to = "open"
    "#;

    #[test]
    fn parses_a_hand_written_file() {
        let workflow = Workflow::from_toml(SAMPLE).unwrap();
        assert_eq!(workflow.name, "Docs");
        assert!(workflow.enabled);
        assert_eq!(workflow.variables["site"], "https://example.com");
        assert_eq!(workflow.nodes.len(), 3);
        assert_eq!(workflow.nodes[0].x, 40.0);
        assert_eq!(workflow.nodes[0].y, 60.5);
        assert!(matches!(
            &workflow.nodes[0].kind,
            NodeKind::Keyword { keyword, argument: Argument::Required, .. } if keyword == "docs"
        ));
        assert!(matches!(
            &workflow.nodes[1].kind,
            NodeKind::Conditional {
                test: Test::Matches,
                ignore_case: false,
                ..
            }
        ));
        assert_eq!(workflow.connections[0].port, OUT);
        assert_eq!(workflow.connections[1].port, THEN);
        let next: Vec<_> = workflow
            .outgoing("c", THEN)
            .map(|c| c.to.as_str())
            .collect();
        assert_eq!(next, ["open"]);
        assert_eq!(workflow.outgoing("c", ELSE).count(), 0);
    }

    #[test]
    fn writes_what_it_reads() {
        let workflow = Workflow::from_toml(SAMPLE).unwrap();
        let text = workflow.to_toml().unwrap();
        assert!(text.starts_with("# Written by Sevak"));
        let again = Workflow::from_toml(&text).unwrap();
        assert_eq!(again, workflow);
        // Defaults are not spelled out.
        assert!(!text.contains("port = \"out\""), "{text}");
        assert!(!text.contains("ignore_case"), "{text}");
    }

    #[test]
    fn every_node_kind_survives_toml_and_json() {
        let kinds = vec![
            NodeKind::Keyword {
                keyword: "k".into(),
                argument: Argument::None,
                subtitle: "s".into(),
            },
            NodeKind::Hotkey {
                key: "Ctrl+Alt+K".into(),
            },
            NodeKind::Selection {
                accepts: vec![Accepts::File],
            },
            NodeKind::External {},
            NodeKind::ScriptFilter {
                keyword: "f".into(),
                command: vec!["python3".into(), "f.py".into()],
                script: None,
                args: vec!["--x".into()],
                timeout_ms: Some(80),
                hard_timeout_ms: Some(2000),
            },
            NodeKind::RunScript {
                command: Vec::new(),
                script: Some("run.py".into()),
                args: vec!["{query}".into()],
                stdin: Some("{query}".into()),
                env: [("A".to_owned(), "b".to_owned())].into_iter().collect(),
                timeout_ms: Some(500),
                log_stderr: true,
            },
            NodeKind::OpenUrl {
                url: "https://a.example/{query}".into(),
            },
            NodeKind::OpenFile {
                path: "~/notes.txt".into(),
            },
            NodeKind::LaunchApp {
                app: "Firefox".into(),
                args: vec!["--private".into()],
            },
            NodeKind::SystemCommand {
                command: "lock".into(),
            },
            NodeKind::TerminalCommand {
                command: "ls -la".into(),
            },
            NodeKind::Copy { text: "x".into() },
            NodeKind::Paste {
                text: "y".into(),
                restore_clipboard: Some(false),
            },
            NodeKind::SetVariable {
                name: "n".into(),
                value: "v".into(),
            },
            NodeKind::Transform {
                op: TransformOp::Split,
                input: "{query}".into(),
                find: ",".into(),
                replace: String::new(),
                index: -1,
                into: Some("last".into()),
            },
            NodeKind::Conditional {
                left: "{query}".into(),
                test: Test::StartsWith,
                right: "a".into(),
                ignore_case: true,
            },
            NodeKind::Delay { ms: 250 },
            NodeKind::Notification {
                heading: "h".into(),
                body: "b".into(),
            },
            NodeKind::LargeType { text: "big".into() },
            NodeKind::TextView {
                heading: "t".into(),
                text: "words".into(),
                markdown: false,
            },
            NodeKind::SplitArg {
                delimiter: Delimiter::Custom,
                custom: "::".into(),
                discard_empty: false,
                trim: false,
                prefix: "p_".into(),
            },
            NodeKind::JoinArg {
                separator: r"\n".into(),
                discard_empty: true,
            },
            NodeKind::Random {
                mode: RandomMode::Line,
                min: -3,
                max: 9,
                lines: "a\nb".into(),
                variable: Some("pick".into()),
            },
            NodeKind::Expression {
                expression: "{var:n} + 1".into(),
                variable: "next".into(),
                set_arg: false,
            },
            NodeKind::WriteFile {
                path: "~/log.txt".into(),
                content: "{query}\n".into(),
                mode: WriteMode::Prepend,
                create_dirs: true,
            },
            NodeKind::Debug { passthrough: false },
            NodeKind::RevealFile {
                path: "{query}".into(),
            },
            NodeKind::CallExternal {
                workflow: "other".into(),
                trigger: "go".into(),
                arg: "{query}".into(),
                pass_variables: true,
            },
        ];
        let names: std::collections::HashSet<_> = kinds.iter().map(|k| k.type_name()).collect();
        assert_eq!(names.len(), kinds.len(), "a type name is used twice");

        let workflow = Workflow {
            name: "all".into(),
            nodes: kinds
                .into_iter()
                .enumerate()
                .map(|(i, kind)| Node {
                    id: format!("n{i}"),
                    title: format!("Node {i}"),
                    x: i as f64 * 10.0,
                    y: 5.0,
                    kind,
                })
                .collect(),
            ..Workflow::default()
        };
        let text = workflow.to_toml().unwrap();
        assert_eq!(Workflow::from_toml(&text).unwrap(), workflow, "{text}");
        // The builder talks JSON with the same shape.
        let json = serde_json::to_string(&workflow).unwrap();
        assert_eq!(serde_json::from_str::<Workflow>(&json).unwrap(), workflow);
        assert!(json.contains(r#""type":"script_filter""#), "{json}");
    }

    #[test]
    fn leaving_things_out_uses_the_defaults() {
        let workflow = Workflow::from_toml(
            r#"
            [[node]]
            id = "k"
            type = "keyword"
            keyword = "x"
            [[node]]
            id = "c"
            type = "copy"
            [[node]]
            id = "s"
            type = "selection"
            "#,
        )
        .unwrap();
        assert_eq!(workflow.format, FORMAT);
        assert!(workflow.enabled);
        assert!(matches!(
            &workflow.nodes[0].kind,
            NodeKind::Keyword {
                argument: Argument::Optional,
                ..
            }
        ));
        assert!(matches!(&workflow.nodes[1].kind, NodeKind::Copy { text } if text == "{query}"));
        assert!(
            matches!(&workflow.nodes[2].kind, NodeKind::Selection { accepts } if accepts.len() == 3)
        );
    }

    #[test]
    fn unknown_types_and_formats_are_reported() {
        let err = Workflow::from_toml("[[node]]\nid = \"a\"\ntype = \"teleport\"\n").unwrap_err();
        assert!(err.contains("teleport"), "{err}");
        let err = Workflow::from_toml("format = 2\n").unwrap_err();
        assert!(err.contains("format 2"), "{err}");
        assert!(Workflow::from_toml("not toml at all [").is_err());
        // A keyword node without a keyword is not a node.
        assert!(Workflow::from_toml("[[node]]\nid = \"a\"\ntype = \"keyword\"\n").is_err());
    }

    #[test]
    fn categories_ports_and_approval() {
        let copy = NodeKind::Copy { text: "x".into() };
        assert_eq!(copy.category(), Category::Action);
        assert_eq!(copy.output_ports(), [OUT]);
        assert!(!copy.needs_approval());
        let cond = NodeKind::Conditional {
            left: String::new(),
            test: Test::IsEmpty,
            right: String::new(),
            ignore_case: false,
        };
        assert_eq!(cond.output_ports(), [THEN, ELSE]);
        assert!(NodeKind::External {}.is_start());
        assert!(!copy.is_start());
        for kind in [
            NodeKind::RunScript {
                command: vec!["x".into()],
                script: None,
                args: Vec::new(),
                stdin: None,
                env: BTreeMap::new(),
                timeout_ms: None,
                log_stderr: false,
            },
            NodeKind::TerminalCommand {
                command: "x".into(),
            },
            NodeKind::SystemCommand {
                command: "lock".into(),
            },
            NodeKind::LaunchApp {
                app: "x".into(),
                args: Vec::new(),
            },
            // Opening a file runs it when it is a program or script.
            NodeKind::OpenFile { path: "x".into() },
            // Pasting types into whatever app was in front.
            NodeKind::Paste {
                text: "x".into(),
                restore_clipboard: None,
            },
            // Calling another workflow runs whatever that one does.
            NodeKind::CallExternal {
                workflow: "x".into(),
                trigger: "go".into(),
                arg: String::new(),
                pass_variables: false,
            },
        ] {
            assert!(kind.needs_approval(), "{}", kind.type_name());
        }
        for kind in [
            NodeKind::OpenUrl { url: "x".into() },
            NodeKind::Notification {
                heading: String::new(),
                body: "x".into(),
            },
        ] {
            assert!(!kind.needs_approval(), "{}", kind.type_name());
        }
    }

    #[test]
    fn the_utility_nodes_have_defaults_categories_and_approval() {
        let workflow = Workflow::from_toml(
            r#"
            [[node]]
            id = "s"
            type = "split_arg"
            [[node]]
            id = "j"
            type = "join_arg"
            [[node]]
            id = "r"
            type = "random"
            [[node]]
            id = "e"
            type = "expression"
            variable = "v"
            [[node]]
            id = "w"
            type = "write_file"
            path = "a.txt"
            [[node]]
            id = "d"
            type = "debug"
            [[node]]
            id = "f"
            type = "reveal_file"
            [[node]]
            id = "c"
            type = "call_external"
            workflow = "w"
            trigger = "t"
            "#,
        )
        .unwrap();
        let kinds: Vec<&NodeKind> = workflow.nodes.iter().map(|n| &n.kind).collect();
        assert!(matches!(
            kinds[0],
            NodeKind::SplitArg { delimiter: Delimiter::Newline, discard_empty: true, trim: true, prefix, .. }
                if prefix == "split_"
        ));
        assert!(
            matches!(kinds[1], NodeKind::JoinArg { separator, discard_empty: false } if separator == ", ")
        );
        assert!(matches!(
            kinds[2],
            NodeKind::Random {
                mode: RandomMode::Number,
                min: 1,
                max: 100,
                variable: None,
                ..
            }
        ));
        assert!(matches!(
            kinds[3],
            NodeKind::Expression { expression, set_arg: true, .. } if expression == "{query}"
        ));
        assert!(matches!(
            kinds[4],
            NodeKind::WriteFile { content, mode: WriteMode::Overwrite, create_dirs: false, .. }
                if content == "{query}"
        ));
        assert!(matches!(kinds[5], NodeKind::Debug { passthrough: true }));
        assert!(matches!(kinds[6], NodeKind::RevealFile { path } if path == "{query}"));
        assert!(matches!(
            kinds[7],
            NodeKind::CallExternal { arg, pass_variables: false, .. } if arg == "{query}"
        ));
        for kind in &kinds {
            let utility = matches!(
                kind.type_name(),
                "split_arg" | "join_arg" | "random" | "expression" | "debug"
            );
            let expected = if utility {
                Category::Utility
            } else {
                Category::Action
            };
            assert_eq!(kind.category(), expected, "{}", kind.type_name());
            assert_eq!(kind.output_ports(), [OUT]);
            // Only writing files and calling another workflow need the user's yes.
            assert_eq!(
                kind.needs_approval(),
                matches!(kind.type_name(), "write_file" | "call_external"),
                "{}",
                kind.type_name()
            );
        }
        assert!(workflow.needs_approval());
        // Defaults are not spelled out.
        let text = workflow.to_toml().unwrap();
        assert!(!text.contains("discard_empty"), "{text}");
        assert!(!text.contains("passthrough"), "{text}");
        assert_eq!(Workflow::from_toml(&text).unwrap(), workflow);
        // `variable` has no default.
        assert!(Workflow::from_toml("[[node]]\nid = \"e\"\ntype = \"expression\"\n").is_err());
    }

    #[test]
    fn folder_names() {
        assert_eq!(slug("Search the Docs!"), "search-the-docs");
        assert_eq!(slug("  --  "), "workflow");
        assert_eq!(slug("Ünïcode only"), "n-code-only");
        assert!(slug(&"a".repeat(200)).len() <= 48);
        for good in ["docs", "my-flow_2", "A.B", "two words"] {
            assert!(valid_folder_name(good), "{good}");
        }
        for bad in [
            "",
            ".hidden",
            "a/b",
            "a\\b",
            "..",
            "x:",
            "trailing ",
            "dot.",
            "é",
        ] {
            assert!(!valid_folder_name(bad), "{bad}");
        }
    }

    const FILTERS: &str = r#"
        name = "Filters"

        [[node]]
        id = "list"
        type = "list_filter"
        keyword = "pick"
        [[node.items]]
        title = "Apple"
        arg = "a-1"

        [[node]]
        id = "files"
        type = "file_filter"
        keyword = "ff"
        folders = ["~/Documents"]
        extensions = ["pdf"]
        scope = "files"

        [[node]]
        id = "apps"
        type = "running_apps"
        keyword = "apps"
        output = "pid"

        [[node]]
        id = "ask"
        type = "dialog_conditional"
        message = "Sure?"
        buttons = ["Yes", "No"]

        [[node]]
        id = "say"
        type = "speak"

        [[node]]
        id = "ding"
        type = "play_sound"
        path = "ding.wav"
    "#;

    #[test]
    fn filter_dialog_speech_and_sound_nodes_read_and_write_back_the_same() {
        let w = Workflow::from_toml(FILTERS).unwrap();
        let kinds: Vec<(&str, Category)> = w
            .nodes
            .iter()
            .map(|n| (n.kind.type_name(), n.kind.category()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("list_filter", Category::Input),
                ("file_filter", Category::Input),
                ("running_apps", Category::Input),
                ("dialog_conditional", Category::Utility),
                ("speak", Category::Output),
                ("play_sound", Category::Output),
            ]
        );
        // Defaults fill in what a hand-written file leaves out.
        assert!(matches!(
            &w.nodes[4].kind,
            NodeKind::Speak { text, voice: None } if text == "{query}"
        ));
        assert!(matches!(
            &w.nodes[2].kind,
            NodeKind::RunningApps {
                output: AppOutput::Pid,
                ..
            }
        ));
        // None of them runs code, so none needs the user's Allow.
        assert!(!w.needs_approval());
        // The input nodes answer to a keyword.
        let keywords: Vec<&str> = w.keywords().collect();
        assert_eq!(keywords, ["pick", "ff", "apps"]);
        assert!(w.nodes[..3].iter().all(|n| n.kind.is_start()));

        let again = Workflow::from_toml(&w.to_toml().unwrap()).unwrap();
        assert_eq!(again, w);
    }

    #[test]
    fn a_dialog_has_a_port_per_button_and_cancel() {
        let w = Workflow::from_toml(FILTERS).unwrap();
        assert_eq!(
            w.nodes[3].kind.output_ports(),
            ["button1", "button2", "button3", CANCEL]
        );
        assert_eq!(w.nodes[4].kind.output_ports(), [OUT]);
    }
}
