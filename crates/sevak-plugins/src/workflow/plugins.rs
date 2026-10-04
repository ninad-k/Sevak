//! The `Plugin`s a workflow becomes.
//!
//! | Plugin | Id | Does |
//! |---|---|---|
//! | [`KeywordPlugin`] | `workflow:<folder>:<node>` | one per keyword trigger: a row for `<keyword> <text>` |
//! | [`FilterPlugin`] | `workflow:<folder>:<node>` | one per script filter: the script's rows (a [`ScriptPlugin`] underneath) |
//! | [`TriggersPlugin`] | `workflow:<folder>` | one per workflow with Universal Actions, hotkey or external triggers |
//!
//! Rows carry what to run in a `Custom` action; `execute` hands it to the
//! workflow's [`Runtime`], which runs on a thread of its own, so `execute`
//! returns at once and the launcher can hide before a paste node types.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sevak_core::model::score;
use sevak_core::{
    Action, IconSource, Plugin, PluginError, PluginResult, ResultItem, ResultsNotifier, Selection,
    SelectionKind,
};
use sevak_platform::PlatformProvider;

use super::exec::{Ctx, Runtime};
use super::model::{Accepts, Argument, Node, NodeKind, FAMILY};
use super::template::{expand, Scope, Target};
use crate::script::{Format, Launch, Manifest, Mode, RawPick, ScriptPlugin, Spec};

/// The id of the plugin for the keyword or script filter node `node`.
pub fn node_plugin_id(folder: &str, node: &str) -> String {
    format!("{FAMILY}:{folder}:{node}")
}

/// The result id that runs the hotkey or external trigger `node` (what a
/// `[[hotkey]] run = "..."` entry names).
pub fn run_id(folder: &str, node: &str) -> String {
    format!("{FAMILY}:{folder}:run:{node}")
}

fn icon() -> IconSource {
    IconSource::builtin("plugin")
}

fn plugin_error(message: String) -> PluginError {
    PluginError::Message(message)
}

/// What a keyword row asks `execute` to start.
#[derive(Debug, Serialize, Deserialize)]
struct Pick {
    arg: String,
    /// The keyword needs text and none was typed.
    #[serde(default)]
    missing: bool,
}

/// What a Universal Actions, hotkey or external row asks `execute` to start.
#[derive(Serialize, Deserialize)]
struct TriggerPick {
    node: String,
    #[serde(default)]
    arg: String,
    /// `text`, `url` or `file` for Universal Actions.
    #[serde(default)]
    kind: Option<String>,
}

fn custom(payload: &impl Serialize) -> Action {
    Action::Custom {
        payload: serde_json::to_string(payload).unwrap_or_default(),
    }
}

/// The label of a node on a result row: its title, else the workflow's name.
fn row_title(node: &Node, runtime: &Runtime) -> String {
    let title = node.title.trim();
    if title.is_empty() {
        runtime.workflow.name.trim().to_owned()
    } else {
        title.to_owned()
    }
}

// ---- keyword -------------------------------------------------------------

/// `<keyword> <text>` shows one row that runs the workflow with the text.
pub struct KeywordPlugin {
    id: String,
    keyword: String,
    node_id: String,
    argument: Argument,
    title: String,
    subtitle: String,
    runtime: Arc<Runtime>,
}

impl KeywordPlugin {
    /// `None` when `node` is not a keyword trigger.
    pub fn new(runtime: Arc<Runtime>, node: &Node) -> Option<Self> {
        let NodeKind::Keyword {
            keyword,
            argument,
            subtitle,
        } = &node.kind
        else {
            return None;
        };
        Some(Self {
            id: node_plugin_id(&runtime.folder, &node.id),
            keyword: keyword.trim().to_owned(),
            node_id: node.id.clone(),
            argument: *argument,
            title: row_title(node, &runtime),
            subtitle: subtitle.clone(),
            runtime,
        })
    }

    fn describe(&self, text: &str) -> (String, String) {
        let vars = &self.runtime.workflow.variables;
        let scope = Scope { query: text, vars };
        let title = expand(&self.title, &scope, Target::Plain);
        let subtitle = expand(&self.subtitle, &scope, Target::Plain);
        let subtitle = if subtitle.trim().is_empty() {
            self.runtime.workflow.description.trim().to_owned()
        } else {
            subtitle
        };
        (title, subtitle)
    }

    fn row(&self, text: &str) -> ResultItem {
        let missing = self.argument == Argument::Required && text.is_empty();
        let (title, subtitle) = self.describe(text);
        let subtitle = if missing {
            "Type some text after the keyword".to_owned()
        } else {
            subtitle
        };
        ResultItem::new(
            &self.id,
            "run",
            title,
            custom(&Pick {
                arg: text.to_owned(),
                missing,
            }),
        )
        .with_subtitle(subtitle)
        .with_icon(icon())
        .with_score(score::KEYWORD)
    }
}

impl Plugin for KeywordPlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.runtime.workflow.name
    }

    fn description(&self) -> &str {
        &self.runtime.workflow.description
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn keyword_row(&self) -> Option<ResultItem> {
        // Tab turns the bare keyword into `keyword `, ready for the text.
        Some(self.row("").with_autocomplete(format!("{} ", self.keyword)))
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let text = input.trim();
        if self.argument == Argument::None && !text.is_empty() {
            return Vec::new();
        }
        vec![self.row(text)]
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return Err(PluginError::Unsupported(item.id.clone()));
        };
        let pick: Pick =
            serde_json::from_str(payload).map_err(|_| PluginError::Unsupported(item.id.clone()))?;
        if pick.missing {
            return Err(plugin_error(
                "Type some text after the keyword first.".to_owned(),
            ));
        }
        self.runtime
            .start(&self.node_id, Ctx::with_arg(pick.arg))
            .map_err(plugin_error)
    }

    fn shutdown(&self) {
        self.runtime.shutdown();
    }
}

// ---- script filter -------------------------------------------------------

/// A keyword whose rows come from a script; picking a row starts the nodes
/// after the script filter with that row's `arg` and variables.
pub struct FilterPlugin {
    inner: ScriptPlugin,
    node_id: String,
    runtime: Arc<Runtime>,
}

impl FilterPlugin {
    /// `None` when `node` is not a script filter.
    pub fn new(
        runtime: Arc<Runtime>,
        node: &Node,
        platform: Arc<dyn PlatformProvider>,
    ) -> Option<Self> {
        let NodeKind::ScriptFilter {
            keyword,
            command,
            script,
            args,
            timeout_ms,
            hard_timeout_ms,
        } = &node.kind
        else {
            return None;
        };
        let mut launch_args = args.clone();
        let launch = match (command.is_empty(), script) {
            (false, _) => {
                let mut argv = command.clone();
                argv.append(&mut launch_args);
                Launch::Command(argv)
            }
            (true, Some(script)) => Launch::Script(script.clone()),
            (true, None) => return None,
        };
        // A script is started by the interpreter for its extension, and its
        // extra arguments cannot be added to `Launch::Script`: put the script
        // in front of them instead.
        let launch = match launch {
            Launch::Script(script) if !launch_args.is_empty() => {
                match crate::script::resolve_launch(&Launch::Script(script), &runtime.dir) {
                    Ok(mut argv) => {
                        argv.append(&mut launch_args);
                        Launch::Command(argv)
                    }
                    Err(err) => {
                        tracing::warn!(
                            workflow = runtime.folder,
                            node = node.id,
                            "this script filter cannot start yet: {err}"
                        );
                        return None;
                    }
                }
            }
            other => other,
        };
        let id = node_plugin_id(&runtime.folder, &node.id);
        let manifest = Manifest {
            id: id.clone(),
            name: runtime.workflow.name.clone(),
            description: runtime.workflow.description.clone(),
            keyword: keyword.trim().to_owned(),
            launch,
            files: Vec::new(),
            inherit_env: Vec::new(),
            capabilities: Default::default(),
            mode: Mode::Oneshot,
            format: Format::AlfredWorkflow,
            timeout: std::time::Duration::from_millis(timeout_ms.unwrap_or(50).clamp(10, 1_000)),
            hard_timeout: std::time::Duration::from_millis(
                hard_timeout_ms.unwrap_or(3_000).clamp(500, 60_000),
            ),
            idle_timeout: None,
            warnings: Vec::new(),
        };
        let spec = Spec {
            manifest,
            dir: runtime.dir.clone(),
            data_dir: runtime.data_dir.clone(),
            env: runtime.script_environment(),
            expected_key: None,
        };
        Some(Self {
            inner: ScriptPlugin::new(spec, platform),
            node_id: node.id.clone(),
            runtime,
        })
    }
}

impl Plugin for FilterPlugin {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn name(&self) -> &str {
        &self.runtime.workflow.name
    }

    fn description(&self) -> &str {
        &self.runtime.workflow.description
    }

    fn keyword(&self) -> Option<&str> {
        self.inner.keyword()
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        self.inner.query(input)
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return Err(PluginError::Unsupported(item.id.clone()));
        };
        let pick = RawPick::from_payload(payload)
            .ok_or_else(|| PluginError::Unsupported(item.id.clone()))?;
        if !pick.valid {
            return Err(plugin_error("That row is only for information.".to_owned()));
        }
        let mut ctx = Ctx::with_arg(pick.arg);
        ctx.vars = pick.variables;
        if let Some(modifier) = pick.modifier {
            ctx.vars.insert("mod".to_owned(), modifier);
        }
        self.runtime.start(&self.node_id, ctx).map_err(plugin_error)
    }

    fn attach_notifier(&self, notifier: ResultsNotifier) {
        self.inner.attach_notifier(notifier);
    }

    fn shutdown(&self) {
        self.inner.shutdown();
        self.runtime.shutdown();
    }
}

// ---- Universal Actions, hotkeys, external triggers -----------------------

/// Everything of a workflow that is not typed: Universal Actions rows, and the
/// results a `[[hotkey]] run` entry or `--trigger` starts.
pub struct TriggersPlugin {
    id: String,
    runtime: Arc<Runtime>,
}

impl TriggersPlugin {
    /// `None` when the workflow has none of those triggers.
    pub fn new(runtime: Arc<Runtime>) -> Option<Self> {
        let any = runtime.workflow.nodes.iter().any(|node| {
            matches!(
                node.kind,
                NodeKind::Selection { .. } | NodeKind::Hotkey { .. } | NodeKind::External {}
            )
        });
        any.then(|| Self {
            id: format!("{FAMILY}:{}", runtime.folder),
            runtime,
        })
    }

    fn run_row(&self, node: &Node) -> ResultItem {
        ResultItem::new(
            &self.id,
            format!("run:{}", node.id),
            row_title(node, &self.runtime),
            custom(&TriggerPick {
                node: node.id.clone(),
                arg: String::new(),
                kind: None,
            }),
        )
        .with_subtitle(self.runtime.workflow.description.trim())
        .with_icon(icon())
    }
}

/// Which kind of selection a trigger sees (`text`, `url`, `file`).
fn selection_kind(selection: &Selection) -> &'static str {
    match selection.kind() {
        SelectionKind::Files => "file",
        SelectionKind::Urls => "url",
        SelectionKind::Text => "text",
    }
}

/// Whether a trigger that `accepts` these kinds is offered for `selection`:
/// text takes any text, URLs included; `url` only selections of links.
fn accepts_selection(accepts: &[Accepts], selection: &Selection) -> bool {
    match selection.kind() {
        SelectionKind::Files => accepts.contains(&Accepts::File),
        SelectionKind::Urls => accepts.contains(&Accepts::Url) || accepts.contains(&Accepts::Text),
        SelectionKind::Text => accepts.contains(&Accepts::Text),
    }
}

/// The argument a Universal Actions trigger starts with: the text, or one URL
/// or path per line.
fn selection_arg(selection: &Selection) -> String {
    match selection.kind() {
        SelectionKind::Files => selection
            .files()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n"),
        SelectionKind::Urls => selection.urls().unwrap_or_default().join("\n"),
        SelectionKind::Text => selection.text().unwrap_or_default().to_owned(),
    }
}

impl Plugin for TriggersPlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.runtime.workflow.name
    }

    fn description(&self) -> &str {
        &self.runtime.workflow.description
    }

    fn keyword(&self) -> Option<&str> {
        None
    }

    /// Never answers typed queries.
    fn global(&self) -> bool {
        false
    }

    fn query(&self, _input: &str) -> Vec<ResultItem> {
        Vec::new()
    }

    /// The selection is private: running a row records nothing.
    fn tracks_usage(&self) -> bool {
        false
    }

    fn selection_actions(&self, selection: &Selection) -> Vec<ResultItem> {
        let kind = selection_kind(selection);
        let mut rows = Vec::new();
        for node in &self.runtime.workflow.nodes {
            let NodeKind::Selection { accepts } = &node.kind else {
                continue;
            };
            if !accepts_selection(accepts, selection) {
                continue;
            }
            // The id names the node, never the selection.
            rows.push(
                ResultItem::new(
                    &self.id,
                    format!("select:{}", node.id),
                    row_title(node, &self.runtime),
                    custom(&TriggerPick {
                        node: node.id.clone(),
                        arg: selection_arg(selection),
                        kind: Some(kind.to_owned()),
                    }),
                )
                .with_subtitle(self.runtime.workflow.description.trim())
                .with_icon(icon()),
            );
        }
        rows
    }

    fn resolve(&self, id: &str) -> Option<ResultItem> {
        let node_id = id.strip_prefix(&format!("{}:run:", self.id))?;
        let node = self.runtime.workflow.node(node_id)?;
        matches!(node.kind, NodeKind::Hotkey { .. } | NodeKind::External {})
            .then(|| self.run_row(node))
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return Err(PluginError::Unsupported(item.id.clone()));
        };
        let pick: TriggerPick =
            serde_json::from_str(payload).map_err(|_| PluginError::Unsupported(item.id.clone()))?;
        let node = self
            .runtime
            .workflow
            .node(&pick.node)
            .filter(|node| {
                matches!(
                    node.kind,
                    NodeKind::Selection { .. } | NodeKind::Hotkey { .. } | NodeKind::External {}
                )
            })
            .ok_or_else(|| PluginError::Unsupported(item.id.clone()))?;
        let mut ctx = Ctx::with_arg(pick.arg);
        if let Some(kind) = pick.kind {
            ctx.vars.insert("selection_kind".to_owned(), kind);
        }
        self.runtime.start(&node.id, ctx).map_err(plugin_error)
    }

    fn shutdown(&self) {
        self.runtime.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::testing::{clipboard, copy, fixture, keyword_with, node, wire, Fixture};

    fn keyword_plugin(f: &Fixture, id: &str) -> KeywordPlugin {
        let node = f.runtime.workflow.node(id).unwrap().clone();
        KeywordPlugin::new(f.runtime.clone(), &node).unwrap()
    }

    fn wait_for_clipboard(f: &Fixture, expected: &[&str]) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if clipboard(f) == expected {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "clipboard is {:?}, wanted {expected:?}",
                clipboard(f)
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_keyword_row_runs_the_workflow_with_the_typed_text() {
        let f = fixture(
            vec![
                keyword_with("k", "ds", Argument::Optional),
                copy("c", "got {query}"),
            ],
            vec![wire("k", "c")],
        );
        let plugin = keyword_plugin(&f, "k");
        assert_eq!(plugin.id(), "workflow:test-flow:k");
        assert_eq!(plugin.keyword(), Some("ds"));
        assert!(!plugin.global());
        assert!(plugin.tracks_usage());

        let rows = plugin.query("  rust traits ");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "workflow:test-flow:k:run");
        assert_eq!(rows[0].title, "Test flow");
        assert_eq!(rows[0].plugin_id, plugin.id());
        assert_eq!(rows[0].score, score::KEYWORD);
        plugin.execute(&rows[0]).unwrap();
        wait_for_clipboard(&f, &["got rust traits"]);
    }

    #[test]
    fn the_argument_mode_decides_when_the_row_shows() {
        let build = |argument| {
            let f = fixture(
                vec![keyword_with("k", "ds", argument), copy("c", "x")],
                vec![wire("k", "c")],
            );
            let plugin = keyword_plugin(&f, "k");
            (f, plugin)
        };
        let (_f, none) = build(Argument::None);
        assert_eq!(none.query("").len(), 1);
        assert!(none.query("text").is_empty());

        let (f, required) = build(Argument::Required);
        let rows = required.query("");
        assert_eq!(rows[0].subtitle, "Type some text after the keyword");
        let err = required.execute(&rows[0]).unwrap_err();
        assert!(err.to_string().contains("Type some text"), "{err}");
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(clipboard(&f).is_empty(), "nothing may run without text");
        assert_eq!(required.query("x")[0].subtitle, "");

        let (_f, optional) = build(Argument::Optional);
        assert_eq!(optional.query("").len(), 1);
        assert_eq!(optional.query("a").len(), 1);
    }

    #[test]
    fn titles_and_subtitles_can_use_the_query() {
        let mut k = keyword_with("k", "ds", Argument::Optional);
        k.title = "Search for {query}".into();
        if let NodeKind::Keyword { subtitle, .. } = &mut k.kind {
            *subtitle = "in the docs".into();
        }
        let f = fixture(vec![k, copy("c", "x")], vec![wire("k", "c")]);
        let plugin = keyword_plugin(&f, "k");
        let row = &plugin.query("rust")[0];
        assert_eq!(row.title, "Search for rust");
        assert_eq!(row.subtitle, "in the docs");
    }

    #[test]
    fn the_bare_keyword_offers_tab_completion() {
        let f = fixture(
            vec![keyword_with("k", "ds", Argument::Required), copy("c", "x")],
            vec![wire("k", "c")],
        );
        let plugin = keyword_plugin(&f, "k");
        let hint = plugin.keyword_row().unwrap();
        assert_eq!(hint.autocomplete.as_deref(), Some("ds "));
    }

    #[test]
    fn only_keyword_nodes_make_keyword_plugins() {
        let f = fixture(
            vec![keyword_with("k", "ds", Argument::Optional), copy("c", "x")],
            vec![wire("k", "c")],
        );
        let c = f.runtime.workflow.node("c").unwrap().clone();
        assert!(KeywordPlugin::new(f.runtime.clone(), &c).is_none());
        assert!(TriggersPlugin::new(f.runtime.clone()).is_none());
    }

    fn triggers_fixture() -> Fixture {
        fixture(
            vec![
                node(
                    "sel",
                    NodeKind::Selection {
                        accepts: vec![Accepts::Text],
                    },
                ),
                node(
                    "links",
                    NodeKind::Selection {
                        accepts: vec![Accepts::Url],
                    },
                ),
                node(
                    "files",
                    NodeKind::Selection {
                        accepts: vec![Accepts::File],
                    },
                ),
                node(
                    "hk",
                    NodeKind::Hotkey {
                        key: "Ctrl+Alt+H".into(),
                    },
                ),
                node("ext", NodeKind::External {}),
                copy("c", "{var:selection_kind}:{query}"),
            ],
            vec![
                wire("sel", "c"),
                wire("links", "c"),
                wire("files", "c"),
                wire("hk", "c"),
                wire("ext", "c"),
            ],
        )
    }

    fn rows_for(plugin: &TriggersPlugin, selection: &Selection) -> Vec<String> {
        plugin
            .selection_actions(selection)
            .into_iter()
            .map(|row| row.id)
            .collect()
    }

    #[test]
    fn universal_actions_rows_follow_what_each_trigger_accepts() {
        let f = triggers_fixture();
        let plugin = TriggersPlugin::new(f.runtime.clone()).unwrap();
        assert_eq!(plugin.id(), "workflow:test-flow");
        assert!(!plugin.tracks_usage());
        assert!(!plugin.global());
        assert!(plugin.query("anything").is_empty());

        let text = Selection::from_text("hello there").unwrap();
        assert_eq!(rows_for(&plugin, &text), ["workflow:test-flow:select:sel"]);
        // A link is text too, and also what `url` triggers want.
        let link = Selection::from_text("https://example.com").unwrap();
        assert_eq!(
            rows_for(&plugin, &link),
            [
                "workflow:test-flow:select:sel",
                "workflow:test-flow:select:links"
            ]
        );
        let files = Selection::from_files(vec!["/tmp/a.txt".into()]).unwrap();
        assert_eq!(
            rows_for(&plugin, &files),
            ["workflow:test-flow:select:files"]
        );
    }

    #[test]
    fn row_ids_and_debug_output_never_contain_the_selection() {
        let f = triggers_fixture();
        let plugin = TriggersPlugin::new(f.runtime.clone()).unwrap();
        let secret = Selection::from_text("my secret passphrase").unwrap();
        for row in plugin.selection_actions(&secret) {
            assert!(!row.id.contains("secret"));
            assert!(!row.title.contains("secret"));
            assert!(!row.subtitle.contains("secret"));
            assert!(row.copy_text().is_none(), "Ctrl+C must not copy it");
        }
    }

    #[test]
    fn a_selection_row_starts_the_workflow_with_the_selection() {
        let f = triggers_fixture();
        let plugin = TriggersPlugin::new(f.runtime.clone()).unwrap();
        let text = Selection::from_text("some words").unwrap();
        let row = plugin.selection_actions(&text).remove(0);
        plugin.execute(&row).unwrap();
        wait_for_clipboard(&f, &["text:some words"]);

        let files = Selection::from_files(vec!["/a.txt".into(), "/b.txt".into()]).unwrap();
        let row = plugin.selection_actions(&files).remove(0);
        plugin.execute(&row).unwrap();
        let joined = format!("file:{}\n{}", "/a.txt", "/b.txt");
        wait_for_clipboard(&f, &["text:some words", &joined]);
    }

    #[test]
    fn urls_arrive_one_per_line() {
        let selection = Selection::from_text("https://a.example\nwww.b.example").unwrap();
        assert_eq!(
            selection_arg(&selection),
            "https://a.example\nhttps://www.b.example"
        );
        assert_eq!(selection_kind(&selection), "url");
    }

    #[test]
    fn hotkey_and_external_triggers_resolve_by_id() {
        let f = triggers_fixture();
        let plugin = TriggersPlugin::new(f.runtime.clone()).unwrap();
        assert_eq!(run_id("test-flow", "hk"), "workflow:test-flow:run:hk");
        let row = plugin.resolve(&run_id("test-flow", "hk")).unwrap();
        assert_eq!(row.plugin_id, "workflow:test-flow");
        plugin.execute(&row).unwrap();
        wait_for_clipboard(&f, &[":"]);
        let ext = plugin.resolve(&run_id("test-flow", "ext")).unwrap();
        assert_eq!(ext.id, "workflow:test-flow:run:ext");
        // Selection and plain action nodes cannot be started by id.
        assert!(plugin.resolve(&run_id("test-flow", "sel")).is_none());
        assert!(plugin.resolve(&run_id("test-flow", "c")).is_none());
        assert!(plugin.resolve("workflow:test-flow:run:nope").is_none());
        assert!(plugin.resolve("workflow:other:run:hk").is_none());
    }

    #[test]
    fn a_forged_payload_cannot_start_an_action_node() {
        let f = triggers_fixture();
        let plugin = TriggersPlugin::new(f.runtime.clone()).unwrap();
        let mut row = plugin.resolve(&run_id("test-flow", "hk")).unwrap();
        row.action = custom(&TriggerPick {
            node: "c".into(),
            arg: "x".into(),
            kind: None,
        });
        assert!(plugin.execute(&row).is_err());
        row.action = Action::CopyText { text: "x".into() };
        assert!(plugin.execute(&row).is_err());
    }
}
