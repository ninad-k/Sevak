//! Checking a workflow before it is saved or run.
//!
//! The builder shows these problems next to the nodes they are about; the host
//! refuses to load a workflow that has any error. Warnings (a node nothing
//! reaches, a trigger that leads nowhere) do not stop anything.

use std::collections::{HashMap, HashSet};

use regex::RegexBuilder;
use serde::Serialize;
use sevak_platform::SystemCommand;

use crate::keywords::KeywordOwners;

#[cfg(test)]
use super::model::OUT;
use super::model::{
    Accepts, Node, NodeKind, Test, TransformOp, Workflow, FORMAT, MAX_CONNECTIONS, MAX_DELAY_MS,
    MAX_NODES,
};

/// Largest compiled regular expression a node may use.
pub const REGEX_SIZE_LIMIT: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// One thing wrong (or suspicious) about a workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub severity: Severity,
    /// The node it is about; `None` for the workflow as a whole.
    pub node: Option<String>,
    pub message: String,
}

impl Problem {
    fn error(node: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            node: node.map(str::to_owned),
            message: message.into(),
        }
    }

    fn warning(node: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            node: node.map(str::to_owned),
            message: message.into(),
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// The messages of the errors among `problems`, for one-line reports.
pub fn error_summary(problems: &[Problem]) -> Option<String> {
    let errors: Vec<String> = problems
        .iter()
        .filter(|problem| problem.is_error())
        .map(|problem| match &problem.node {
            Some(node) => format!("{node}: {}", problem.message),
            None => problem.message.clone(),
        })
        .collect();
    (!errors.is_empty()).then(|| errors.join("; "))
}

/// Whether `name` can be a variable or environment variable name.
pub fn valid_variable_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.len() <= 64
}

fn valid_node_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// The regular expression `pattern` as the nodes compile it, or why it is not
/// acceptable.
pub fn compile_regex(pattern: &str, ignore_case: bool) -> Result<regex::Regex, String> {
    RegexBuilder::new(pattern)
        .case_insensitive(ignore_case)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|err| {
            // The first line says what is wrong; the rest repeats the pattern.
            let text = err.to_string();
            let first = text.lines().last().unwrap_or("invalid regular expression");
            format!("invalid regular expression: {}", first.trim())
        })
}

impl Workflow {
    /// Every problem found, errors first.
    pub fn validate(&self) -> Vec<Problem> {
        let mut problems = Vec::new();
        self.check_workflow(&mut problems);
        let mut ids: HashSet<&str> = HashSet::new();
        for node in &self.nodes {
            if !valid_node_id(&node.id) {
                problems.push(Problem::error(
                    Some(&node.id),
                    "the id must be 1 to 40 letters, digits, dashes or underscores",
                ));
            } else if !ids.insert(&node.id) {
                problems.push(Problem::error(Some(&node.id), "this id is used twice"));
            }
            check_node(node, &mut problems);
        }
        self.check_graph(&mut problems);
        // Stable and readable: errors before warnings, otherwise in file order.
        problems.sort_by_key(|problem| problem.severity != Severity::Error);
        problems
    }

    /// A warning for each keyword trigger whose keyword something else in
    /// `owners` answers too. `own_key` is this workflow's own entry in
    /// `owners` (see [`crate::keywords::workflow_key`]), which is not "something
    /// else". Kept apart from [`Workflow::validate`], which needs no config.
    pub fn keyword_problems(&self, owners: &KeywordOwners, own_key: Option<&str>) -> Vec<Problem> {
        let mut problems = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for (node, keyword) in self.keyword_nodes() {
            if !seen.insert(keyword.to_lowercase()) {
                continue;
            }
            for (_, message) in owners.warnings_for(own_key, [keyword]) {
                problems.push(Problem::warning(Some(&node.id), message));
            }
        }
        problems
    }

    /// True when [`Workflow::validate`] finds no error.
    pub fn is_valid(&self) -> bool {
        !self.validate().iter().any(Problem::is_error)
    }

    fn check_workflow(&self, problems: &mut Vec<Problem>) {
        if self.format != FORMAT {
            problems.push(Problem::error(
                None,
                format!(
                    "format {} is not supported (this Sevak reads format {FORMAT})",
                    self.format
                ),
            ));
        }
        if self.name.trim().is_empty() {
            problems.push(Problem::error(None, "the workflow needs a name"));
        }
        if self.nodes.len() > MAX_NODES {
            problems.push(Problem::error(
                None,
                format!("a workflow may have at most {MAX_NODES} nodes"),
            ));
        }
        if self.connections.len() > MAX_CONNECTIONS {
            problems.push(Problem::error(
                None,
                format!("a workflow may have at most {MAX_CONNECTIONS} connections"),
            ));
        }
        for name in self.variables.keys() {
            if !valid_variable_name(name) {
                problems.push(Problem::error(
                    None,
                    format!(
                        "the variable name \"{name}\" may only use letters, digits, _, - and ."
                    ),
                ));
            }
        }
        if !self.nodes.iter().any(|node| node.kind.is_start()) {
            problems.push(Problem::error(
                None,
                "the workflow has no trigger: add a keyword, hotkey, Universal Actions, \
                 external trigger or script filter node",
            ));
        }
    }

    fn check_graph(&self, problems: &mut Vec<Problem>) {
        let by_id: HashMap<&str, &Node> = self
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect();
        let mut seen: HashSet<(&str, &str, &str)> = HashSet::new();
        // Only the connections that make sense take part in the checks below.
        let mut edges: Vec<(&str, &str)> = Vec::new();

        for conn in &self.connections {
            let label = format!("{}.{} -> {}", conn.from, conn.port, conn.to);
            let (Some(from), Some(to)) =
                (by_id.get(conn.from.as_str()), by_id.get(conn.to.as_str()))
            else {
                let missing = if by_id.contains_key(conn.from.as_str()) {
                    &conn.to
                } else {
                    &conn.from
                };
                problems.push(Problem::error(
                    None,
                    format!("the connection {label} points at the node \"{missing}\", which does not exist"),
                ));
                continue;
            };
            if !from.kind.output_ports().contains(&conn.port.as_str()) {
                problems.push(Problem::error(
                    Some(&conn.from),
                    format!(
                        "this node has no output \"{}\" (the connection {label})",
                        conn.port
                    ),
                ));
                continue;
            }
            if to.kind.is_start() {
                problems.push(Problem::error(
                    Some(&conn.to),
                    format!(
                        "a {} has no input to connect to ({label})",
                        to.kind.type_name()
                    ),
                ));
                continue;
            }
            if conn.from == conn.to {
                problems.push(Problem::error(
                    Some(&conn.from),
                    "a node cannot be connected to itself",
                ));
                continue;
            }
            if !seen.insert((&conn.from, &conn.port, &conn.to)) {
                problems.push(Problem::error(
                    Some(&conn.from),
                    format!("the connection {label} is listed twice"),
                ));
                continue;
            }
            edges.push((&conn.from, &conn.to));
        }

        if let Some(cycle) = find_cycle(&self.nodes, &edges) {
            problems.push(Problem::error(
                Some(&cycle[0]),
                format!("the connections form a loop: {}", cycle.join(" -> ")),
            ));
            // Reachability is meaningless in a loop.
            return;
        }

        // Warnings: dead ends and unreachable nodes.
        let mut reached: HashSet<&str> = HashSet::new();
        let mut stack: Vec<&str> = self
            .nodes
            .iter()
            .filter(|node| node.kind.is_start())
            .map(|node| node.id.as_str())
            .collect();
        while let Some(id) = stack.pop() {
            if !reached.insert(id) {
                continue;
            }
            stack.extend(
                edges
                    .iter()
                    .filter(|(from, _)| *from == id)
                    .map(|(_, to)| *to),
            );
        }
        for node in &self.nodes {
            if node.kind.is_start() {
                if !edges.iter().any(|(from, _)| *from == node.id) {
                    // A script filter or keyword with nothing after it only
                    // shows a row that does nothing.
                    problems.push(Problem::warning(
                        Some(&node.id),
                        "nothing is connected after this trigger, so it does nothing",
                    ));
                }
            } else if !reached.contains(node.id.as_str()) {
                problems.push(Problem::warning(
                    Some(&node.id),
                    "no trigger leads to this node, so it never runs",
                ));
            }
        }
    }
}

/// A loop in the graph as the list of node ids around it (first repeated at
/// the end), if there is one.
fn find_cycle(nodes: &[Node], edges: &[(&str, &str)]) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Visiting,
        Done,
    }
    fn visit<'a>(
        id: &'a str,
        edges: &[(&'a str, &'a str)],
        marks: &mut HashMap<&'a str, Mark>,
        path: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        match marks.get(id) {
            Some(Mark::Done) => return None,
            Some(Mark::Visiting) => {
                let start = path.iter().position(|seen| *seen == id).unwrap_or(0);
                let mut cycle: Vec<String> =
                    path[start..].iter().map(|s| (*s).to_owned()).collect();
                cycle.push(id.to_owned());
                return Some(cycle);
            }
            None => {}
        }
        marks.insert(id, Mark::Visiting);
        path.push(id);
        for (_, to) in edges.iter().filter(|(from, _)| *from == id) {
            if let Some(cycle) = visit(to, edges, marks, path) {
                return Some(cycle);
            }
        }
        path.pop();
        marks.insert(id, Mark::Done);
        None
    }

    let mut marks = HashMap::new();
    for node in nodes {
        let mut path = Vec::new();
        if let Some(cycle) = visit(&node.id, edges, &mut marks, &mut path) {
            return Some(cycle);
        }
    }
    None
}

fn check_node(node: &Node, problems: &mut Vec<Problem>) {
    let id = Some(node.id.as_str());
    let mut error = |message: &str| problems.push(Problem::error(id, message));
    match &node.kind {
        NodeKind::Keyword { keyword, .. } => check_keyword(keyword, &mut error),
        NodeKind::Hotkey { key } => {
            if key.trim().is_empty() {
                error("choose a shortcut");
            }
        }
        NodeKind::Selection { accepts } => {
            if accepts.is_empty() {
                error("accept at least one of text, url or file");
            }
            let mut seen: Vec<Accepts> = Vec::new();
            for kind in accepts {
                if seen.contains(kind) {
                    error("a kind is listed twice in `accepts`");
                    break;
                }
                seen.push(*kind);
            }
        }
        NodeKind::External {} => {}
        NodeKind::ScriptFilter {
            keyword,
            command,
            script,
            ..
        } => {
            check_keyword(keyword, &mut error);
            check_program(command, script.as_deref(), &mut error);
        }
        NodeKind::RunScript {
            command,
            script,
            env,
            timeout_ms,
            ..
        } => {
            check_program(command, script.as_deref(), &mut error);
            for name in env.keys() {
                if !valid_env_name(name) {
                    error(&format!(
                        "\"{name}\" is not a valid environment variable name"
                    ));
                }
            }
            if timeout_ms.is_some_and(|ms| ms == 0) {
                error("the timeout must be more than 0 ms");
            }
        }
        NodeKind::OpenUrl { url } => {
            let url = url.trim();
            if url.is_empty() {
                error("enter a link");
            } else if let Some(scheme) = literal_scheme(url) {
                if !matches!(scheme.as_str(), "http" | "https" | "mailto") {
                    error("only http, https and mailto links can be opened");
                }
            }
        }
        NodeKind::OpenFile { path } => {
            if path.trim().is_empty() {
                error("enter a file or folder");
            }
        }
        NodeKind::LaunchApp { app, .. } => {
            if app.trim().is_empty() {
                error("enter an application name or path");
            }
        }
        NodeKind::SystemCommand { command } => {
            if SystemCommand::from_key(command.trim()).is_none() {
                let keys: Vec<&str> = SystemCommand::ALL.iter().map(|c| c.key()).collect();
                error(&format!(
                    "unknown system command \"{}\" (use {})",
                    command.trim(),
                    keys.join(", ")
                ));
            }
        }
        NodeKind::TerminalCommand { command } => {
            if command.trim().is_empty() {
                error("enter a command");
            }
        }
        NodeKind::Copy { .. } | NodeKind::Paste { .. } => {}
        NodeKind::SetVariable { name, .. } => {
            if !valid_variable_name(name.trim()) {
                error("the variable name may only use letters, digits, _, - and .");
            }
        }
        NodeKind::Transform { op, find, into, .. } => {
            if let Some(into) = into {
                if !valid_variable_name(into.trim()) {
                    error("the variable name may only use letters, digits, _, - and .");
                }
            }
            match op {
                TransformOp::RegexReplace => {
                    if let Err(message) = compile_regex(find, false) {
                        error(&message);
                    }
                }
                TransformOp::Replace | TransformOp::Split if find.is_empty() => {
                    error("enter the text to look for");
                }
                _ => {}
            }
        }
        NodeKind::Conditional {
            test,
            right,
            ignore_case,
            ..
        } => {
            if *test == Test::Matches {
                if let Err(message) = compile_regex(right, *ignore_case) {
                    error(&message);
                }
            }
        }
        NodeKind::Delay { ms } => {
            if *ms > MAX_DELAY_MS {
                error(&format!("a delay may last at most {MAX_DELAY_MS} ms"));
            }
        }
        NodeKind::Notification { heading, body } => {
            if heading.trim().is_empty() && body.trim().is_empty() {
                error("a notification needs a heading or text");
            }
        }
        NodeKind::LargeType { .. } | NodeKind::TextView { .. } => {}
    }
}

fn check_keyword(keyword: &str, error: &mut impl FnMut(&str)) {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        error("choose a keyword");
    } else if keyword.chars().any(char::is_whitespace) {
        error("the keyword cannot contain spaces");
    } else if keyword.chars().count() > 32 {
        error("the keyword is too long");
    }
}

fn check_program(command: &[String], script: Option<&str>, error: &mut impl FnMut(&str)) {
    match (command.is_empty(), script.map(str::trim)) {
        (true, None | Some("")) => error("set `command` or `script`"),
        (false, Some(script)) if !script.is_empty() => error("set `command` or `script`, not both"),
        (false, _) if command[0].trim().is_empty() => error("`command` must start with a program"),
        _ => {}
    }
    if let Some(script) = script.filter(|s| !s.trim().is_empty()) {
        if crate::script::relative_inside(std::path::Path::new(""), script).is_none() {
            error("the script must be a file inside the workflow folder");
        }
    }
    if let Some(program) = command.first() {
        let has_path = program.contains(['/', '\\']) || program.starts_with('.');
        let absolute = std::path::Path::new(program).is_absolute();
        if has_path
            && !absolute
            && crate::script::relative_inside(std::path::Path::new(""), program).is_none()
        {
            error("the command must be a program on PATH or a file inside the workflow folder");
        }
    }
}

/// The scheme at the start of `url` when it is written out in full (no
/// placeholder before the colon), lower-cased.
fn literal_scheme(url: &str) -> Option<String> {
    let end = url.find(':')?;
    let scheme = &url[..end];
    (!scheme.is_empty()
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then(|| scheme.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::model::{Argument, Connection};

    fn node(id: &str, kind: NodeKind) -> Node {
        Node {
            id: id.into(),
            title: String::new(),
            x: 0.0,
            y: 0.0,
            kind,
        }
    }

    fn keyword(id: &str, kw: &str) -> Node {
        node(
            id,
            NodeKind::Keyword {
                keyword: kw.into(),
                argument: Argument::Optional,
                subtitle: String::new(),
            },
        )
    }

    fn copy(id: &str) -> Node {
        node(id, NodeKind::Copy { text: "x".into() })
    }

    fn wire(from: &str, to: &str) -> Connection {
        Connection {
            from: from.into(),
            port: OUT.into(),
            to: to.into(),
        }
    }

    fn workflow(nodes: Vec<Node>, connections: Vec<Connection>) -> Workflow {
        Workflow {
            name: "w".into(),
            nodes,
            connections,
            ..Workflow::default()
        }
    }

    fn errors(workflow: &Workflow) -> Vec<String> {
        workflow
            .validate()
            .into_iter()
            .filter(Problem::is_error)
            .map(|p| p.message)
            .collect()
    }

    fn has_error(workflow: &Workflow, needle: &str) -> bool {
        errors(workflow)
            .iter()
            .any(|message| message.contains(needle))
    }

    #[test]
    fn a_simple_chain_is_valid() {
        let w = workflow(vec![keyword("k", "go"), copy("c")], vec![wire("k", "c")]);
        assert_eq!(w.validate(), []);
        assert!(w.is_valid());
    }

    #[test]
    fn loops_are_errors_and_named() {
        let w = workflow(
            vec![keyword("k", "go"), copy("a"), copy("b"), copy("c")],
            vec![
                wire("k", "a"),
                wire("a", "b"),
                wire("b", "c"),
                wire("c", "a"),
            ],
        );
        let found = errors(&w);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("loop"), "{found:?}");
        assert!(found[0].contains("a -> b -> c -> a"), "{found:?}");
        // A node pointing at itself is its own, clearer error.
        let own = workflow(
            vec![keyword("k", "go"), copy("a")],
            vec![wire("k", "a"), wire("a", "a")],
        );
        assert!(has_error(&own, "itself"));
    }

    #[test]
    fn dangling_ports_and_nodes_are_errors() {
        let missing = workflow(
            vec![keyword("k", "go"), copy("c")],
            vec![wire("k", "c"), wire("k", "ghost"), wire("phantom", "c")],
        );
        let found = errors(&missing);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|m| m.contains("\"ghost\"")), "{found:?}");
        assert!(found.iter().any(|m| m.contains("\"phantom\"")), "{found:?}");

        // A port the node does not have.
        let bad_port = workflow(
            vec![keyword("k", "go"), copy("c")],
            vec![Connection {
                from: "k".into(),
                port: "then".into(),
                to: "c".into(),
            }],
        );
        assert!(has_error(&bad_port, "no output \"then\""));

        // Nothing feeds a trigger.
        let into_trigger = workflow(
            vec![keyword("k", "go"), copy("c"), keyword("k2", "other")],
            vec![wire("k", "c"), wire("c", "k2")],
        );
        assert!(has_error(&into_trigger, "no input"));

        let twice = workflow(
            vec![keyword("k", "go"), copy("c")],
            vec![wire("k", "c"), wire("k", "c")],
        );
        assert!(has_error(&twice, "listed twice"));
    }

    #[test]
    fn conditional_branches_are_ports() {
        let cond = node(
            "q",
            NodeKind::Conditional {
                left: "{query}".into(),
                test: Test::Contains,
                right: "a".into(),
                ignore_case: false,
            },
        );
        let w = workflow(
            vec![keyword("k", "go"), cond, copy("yes"), copy("no")],
            vec![
                wire("k", "q"),
                Connection {
                    from: "q".into(),
                    port: "then".into(),
                    to: "yes".into(),
                },
                Connection {
                    from: "q".into(),
                    port: "else".into(),
                    to: "no".into(),
                },
            ],
        );
        assert_eq!(w.validate(), []);
        // A conditional has no plain `out`.
        let wrong = workflow(
            vec![
                keyword("k", "go"),
                node(
                    "q",
                    NodeKind::Conditional {
                        left: String::new(),
                        test: Test::IsEmpty,
                        right: String::new(),
                        ignore_case: false,
                    },
                ),
                copy("c"),
            ],
            vec![wire("k", "q"), wire("q", "c")],
        );
        assert!(has_error(&wrong, "no output \"out\""));
    }

    #[test]
    fn warnings_do_not_make_a_workflow_invalid() {
        let w = workflow(vec![keyword("k", "go"), copy("orphan")], Vec::new());
        let problems = w.validate();
        assert!(w.is_valid());
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().all(|p| p.severity == Severity::Warning));
        assert!(problems.iter().any(|p| p.message.contains("does nothing")));
        assert!(problems.iter().any(|p| p.message.contains("never runs")));
    }

    #[test]
    fn a_workflow_needs_a_name_and_a_trigger() {
        let mut w = workflow(vec![copy("c")], Vec::new());
        w.name = "  ".into();
        let found = errors(&w);
        assert!(
            found.iter().any(|m| m.contains("needs a name")),
            "{found:?}"
        );
        assert!(found.iter().any(|m| m.contains("no trigger")), "{found:?}");
    }

    #[test]
    fn ids_are_checked() {
        let w = workflow(
            vec![
                keyword("k", "go"),
                copy("same"),
                copy("same"),
                copy("bad id!"),
            ],
            vec![wire("k", "same")],
        );
        let found = errors(&w);
        assert!(found.iter().any(|m| m.contains("used twice")), "{found:?}");
        assert!(
            found.iter().any(|m| m.contains("letters, digits")),
            "{found:?}"
        );
    }

    #[test]
    fn node_fields_are_checked() {
        let cases: Vec<(NodeKind, &str)> = vec![
            (
                NodeKind::Keyword {
                    keyword: "two words".into(),
                    argument: Argument::None,
                    subtitle: String::new(),
                },
                "spaces",
            ),
            (NodeKind::Hotkey { key: " ".into() }, "shortcut"),
            (NodeKind::Selection { accepts: vec![] }, "at least one"),
            (
                NodeKind::Selection {
                    accepts: vec![Accepts::Text, Accepts::Text],
                },
                "twice",
            ),
            (
                NodeKind::RunScript {
                    command: vec![],
                    script: None,
                    args: vec![],
                    stdin: None,
                    env: Default::default(),
                    timeout_ms: None,
                    log_stderr: false,
                },
                "`command` or `script`",
            ),
            (
                NodeKind::RunScript {
                    command: vec!["x".into()],
                    script: Some("a.py".into()),
                    args: vec![],
                    stdin: None,
                    env: Default::default(),
                    timeout_ms: None,
                    log_stderr: false,
                },
                "not both",
            ),
            (
                NodeKind::RunScript {
                    command: vec![],
                    script: Some("../escape.py".into()),
                    args: vec![],
                    stdin: None,
                    env: Default::default(),
                    timeout_ms: None,
                    log_stderr: false,
                },
                "inside the workflow folder",
            ),
            (
                NodeKind::RunScript {
                    command: vec!["x".into()],
                    script: None,
                    args: vec![],
                    stdin: None,
                    env: [("1BAD".to_owned(), "v".to_owned())].into_iter().collect(),
                    timeout_ms: None,
                    log_stderr: false,
                },
                "environment variable",
            ),
            (
                NodeKind::OpenUrl {
                    url: "file:///etc/passwd".into(),
                },
                "only http",
            ),
            (
                NodeKind::OpenUrl {
                    url: "javascript:alert(1)".into(),
                },
                "only http",
            ),
            (NodeKind::OpenUrl { url: " ".into() }, "enter a link"),
            (
                NodeKind::SystemCommand {
                    command: "format-c".into(),
                },
                "unknown system command",
            ),
            (
                NodeKind::SetVariable {
                    name: "bad name".into(),
                    value: String::new(),
                },
                "variable name",
            ),
            (
                NodeKind::Transform {
                    op: TransformOp::RegexReplace,
                    input: "{query}".into(),
                    find: "(unclosed".into(),
                    replace: String::new(),
                    index: 0,
                    into: None,
                },
                "regular expression",
            ),
            (
                NodeKind::Transform {
                    op: TransformOp::Split,
                    input: "{query}".into(),
                    find: String::new(),
                    replace: String::new(),
                    index: 0,
                    into: None,
                },
                "look for",
            ),
            (
                NodeKind::Conditional {
                    left: "{query}".into(),
                    test: Test::Matches,
                    right: "[".into(),
                    ignore_case: false,
                },
                "regular expression",
            ),
            (
                NodeKind::Delay {
                    ms: MAX_DELAY_MS + 1,
                },
                "at most",
            ),
            (
                NodeKind::Notification {
                    heading: String::new(),
                    body: " ".into(),
                },
                "heading or text",
            ),
        ];
        for (kind, needle) in cases {
            let name = kind.type_name();
            let w = workflow(
                vec![keyword("k", "go"), node("n", kind)],
                vec![wire("k", "n")],
            );
            assert!(has_error(&w, needle), "{name}: {:?}", errors(&w));
        }
    }

    #[test]
    fn good_urls_pass_even_with_a_placeholder_first() {
        for url in [
            "https://example.com/?q={query}",
            "HTTP://example.com",
            "mailto:a@b.example?subject={query}",
            "{var:site}/search",
        ] {
            let w = workflow(
                vec![
                    keyword("k", "go"),
                    node("n", NodeKind::OpenUrl { url: url.into() }),
                ],
                vec![wire("k", "n")],
            );
            assert_eq!(errors(&w), Vec::<String>::new(), "{url}");
        }
    }

    #[test]
    fn regexes_have_a_size_limit() {
        assert!(compile_regex("^[0-9]+$", false).is_ok());
        assert!(compile_regex("(a{1000}){1000}", false).is_err());
        let err = compile_regex("(", false).unwrap_err();
        assert!(err.starts_with("invalid regular expression"), "{err}");
    }

    #[test]
    fn variable_names() {
        for good in ["a", "my_var", "my-var.2"] {
            assert!(valid_variable_name(good), "{good}");
        }
        for bad in ["", "a b", "x=y", "é", &"a".repeat(65)] {
            assert!(!valid_variable_name(bad), "{bad}");
        }
    }

    #[test]
    fn error_summary_lists_only_errors() {
        let w = workflow(
            vec![keyword("k", "go"), copy("orphan"), copy("c")],
            vec![wire("k", "c"), wire("k", "ghost")],
        );
        let summary = error_summary(&w.validate()).unwrap();
        assert!(summary.contains("ghost"), "{summary}");
        assert!(!summary.contains("never runs"), "{summary}");
        assert_eq!(error_summary(&[]), None);
    }
}
