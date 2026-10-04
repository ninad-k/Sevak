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

/// The workflow file format this Sevak reads and writes.
pub const FORMAT: u32 = 1;
/// Name of the workflow file inside a workflow folder.
pub const FILE: &str = "workflow.toml";
/// Every workflow plugin id starts with this; it is also the family id, so
/// `[plugins] disabled = ["workflow"]` switches all workflows off.
pub const FAMILY: &str = "workflow";
/// The output port of every node except [`NodeKind::Conditional`].
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
    },
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
        }
    }

    pub fn category(&self) -> Category {
        match self {
            Self::Keyword { .. }
            | Self::Hotkey { .. }
            | Self::Selection { .. }
            | Self::External {} => Category::Trigger,
            Self::ScriptFilter { .. } => Category::Input,
            Self::RunScript { .. }
            | Self::OpenUrl { .. }
            | Self::OpenFile { .. }
            | Self::LaunchApp { .. }
            | Self::SystemCommand { .. }
            | Self::TerminalCommand { .. }
            | Self::Copy { .. }
            | Self::Paste { .. } => Category::Action,
            Self::SetVariable { .. }
            | Self::Transform { .. }
            | Self::Conditional { .. }
            | Self::Delay { .. } => Category::Utility,
            Self::Notification { .. } | Self::LargeType { .. } | Self::TextView { .. } => {
                Category::Output
            }
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
            _ => &[OUT],
        }
    }

    /// Whether the node runs code or commands, or acts in other apps, so a
    /// workflow containing it has to be allowed by the user first. `OpenFile`
    /// counts: it hands the file to the system's default handler, which runs a
    /// program, script or shortcut, and a relative path reaches files shipped
    /// inside the workflow's folder. `Paste` counts: it types text into
    /// whatever app was in front (a terminal, a chat, a form), which is the
    /// same as the user typing it.
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
        )
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
            NodeKind::Keyword { keyword, .. } | NodeKind::ScriptFilter { keyword, .. } => {
                Some((node, keyword.trim()))
            }
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
}
