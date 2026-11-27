//! Running a workflow.
//!
//! A run starts at a trigger (or a picked script filter row) with an
//! *argument* and a set of *variables*, and follows the connections: each node
//! receives what the node before it produced, does its work and passes its
//! result on. A node with several connections runs them one after the other in
//! file order, each branch with its own copy of the argument and variables, so
//! the order is the same every time. A failing node ends its own branch; the
//! other branches carry on, and the user is told what failed.
//!
//! Everything runs on a thread of its own (never the UI thread, never the
//! typing path). Every step has a limit: script nodes a timeout that kills the
//! program, `delay` a maximum, the whole run a step count and a deadline.
//!
//! Logging names the workflow folder and node ids only. What the user typed,
//! selected or copied is never logged, and neither is a program's output
//! (unless the node asks for `log_stderr`).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use sevak_core::{Config, LaunchTarget, ShellConfig};
use sevak_platform::process::{configure_helper_command, is_reserved_variable, scrub_environment};
use sevak_platform::process_tree::ProcessTree;
use sevak_platform::{PasteOutcome, PlatformProvider, SystemCommand};

use super::model::{
    Delimiter, Node, NodeKind, RandomMode, Test, TransformOp, Workflow, WriteMode, CANCEL,
    DEFAULT_SCRIPT_TIMEOUT_MS, DIALOG_PORTS, ELSE, MAX_DELAY_MS, MAX_DIALOG_BUTTONS,
    MAX_SCRIPT_TIMEOUT_MS, OUT, SOUND_EXTENSIONS, THEN,
};
use super::template::{expand, expand_command, Scope, Target};
use super::ui::{self, Component, UiDefinition, UiScript, UiTarget};
use super::validate::{compile_regex, valid_variable_name};
use crate::files::{expand_home, home_dir};
use crate::script::{resolve_launch, Launch};
use crate::selection::transform::{base64_encode, title_case};
use crate::web_search::percent_encode;

/// Most nodes one run may execute (a node on two branches counts twice).
pub const MAX_STEPS: usize = 200;
/// A run is abandoned after this long, whatever it is doing.
pub const RUN_LIMIT: Duration = Duration::from_secs(15 * 60);
/// Workflows running at once, across all of them.
pub const MAX_CONCURRENT_RUNS: usize = 8;
/// How many `call_external` nodes may be nested inside each other.
pub const MAX_CALL_DEPTH: usize = 8;
/// Most `<prefix>N` variables one `split_arg` sets (`<prefix>count` and the
/// argument still carry every item).
const MAX_SPLIT_VARIABLES: usize = 200;
/// Largest existing file a `write_file` in `prepend` mode will rewrite.
const MAX_PREPEND_BYTES: u64 = 16 * 1024 * 1024;
/// Output kept from one script run.
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;
const MAX_STDERR_BYTES: u64 = 64 * 1024;
/// Whether `path` ends in one of the [`SOUND_EXTENSIONS`].
pub(super) fn has_sound_extension(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            SOUND_EXTENSIONS
                .iter()
                .any(|ok| ext.eq_ignore_ascii_case(ok))
        })
}

/// Environment variables larger than this are not exported (Windows allows
/// 32 767 characters per variable).
const MAX_ENV_BYTES: usize = 16 * 1024;
const POLL: Duration = Duration::from_millis(10);

/// Where a workflow's visible results go. The shell implements it with native
/// notifications and its launcher window; tests record the calls.
pub trait OutputSink: Send + Sync {
    /// A system notification.
    fn notify(&self, heading: &str, body: &str);
    /// Shows `text` huge on screen.
    fn large_type(&self, text: &str);
    /// Shows `text` in the launcher window.
    fn text_view(&self, heading: &str, text: &str);
    /// Like [`OutputSink::text_view`], told whether `text` is Markdown. Sinks
    /// that cannot draw Markdown keep the default, which shows it as text.
    fn text_view_markdown(&self, heading: &str, text: &str, markdown: bool) {
        let _ = markdown;
        self.text_view(heading, text);
    }
    /// Asks the user a yes/no question before something that cannot be undone
    /// (restart, shut down...). Anything but a clear yes is a no, which is what
    /// the default answers.
    fn confirm(&self, _question: &str) -> bool {
        false
    }
    /// Shows what a `debug` node found (the argument and variables). This is
    /// the user's own data, so an implementation shows it to the user and
    /// never writes it to a log. Dropped by default.
    fn debug(&self, _text: &str) {}
    /// Shows a dialog with one button per label and returns the index of the
    /// button pressed, or `None` when it was dismissed (or cannot be shown,
    /// which is what the default answers).
    fn ask(&self, _heading: &str, _message: &str, _buttons: &[String]) -> Option<usize> {
        None
    }
    /// Opens a Sevak UI and waits until it ends: the user pressed something
    /// that continues the workflow, or dismissed it (`closed`). It runs on
    /// the workflow's own thread, so blocking is fine; while it waits, the run's
    /// deadline is paused. `host` runs the node's scripts for the UI. `None`
    /// means the sink cannot show UIs (the default).
    fn show_ui(&self, _session: &UiSession, _host: &dyn UiHost) -> Option<UiOutcome> {
        None
    }
}

/// A Sevak UI to show: the validated definition and what the renderer needs
/// around it. Sinks that draw it never log any of it (it holds what the user
/// typed and selected).
#[derive(Clone)]
pub struct UiSession {
    /// The workflow's folder name and display name.
    pub folder: String,
    pub workflow: String,
    pub node: String,
    pub target: UiTarget,
    pub titlebar: bool,
    pub always_on_top: bool,
    pub definition: UiDefinition,
    /// `data:` URLs of the images, by the path written in the definition.
    pub images: BTreeMap<String, String>,
    pub vars: BTreeMap<String, String>,
    /// The argument the node received (`{query}`).
    pub query: String,
    /// Ids of the scripts `run` actions may name.
    pub scripts: BTreeSet<String>,
}

impl fmt::Debug for UiSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiSession")
            .field("folder", &self.folder)
            .field("node", &self.node)
            .field("target", &self.target)
            .finish()
    }
}

/// How a UI ended.
#[derive(Debug, Clone, PartialEq)]
pub struct UiOutcome {
    /// The output port to continue from (`closed` when dismissed).
    pub port: String,
    /// The argument for the next nodes; the JSON of `state` when `None`.
    pub arg: Option<String>,
    /// The state when the UI ended.
    pub state: serde_json::Value,
}

/// What running a UI script changes in the open UI.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct UiUpdate {
    pub set: BTreeMap<String, serde_json::Value>,
    /// A component that replaces the whole UI, already checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<Component>,
    pub images: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify: Option<String>,
    pub close: bool,
}

/// What a sink may ask of the workflow while its UI is open.
pub trait UiHost {
    /// Runs the node's script `id` (the node's approved scripts only) with
    /// `input` (`{event, component, state, vars}`) on standard input.
    fn run_script(&self, id: &str, input: &serde_json::Value) -> Result<UiUpdate, String>;
    /// Whether the workflow is being stopped (Sevak is closing or reloading):
    /// a sink that waits for the user gives up.
    fn cancelled(&self) -> bool;
}

/// Drops everything (workflows built without a shell).
pub struct NoSink;

impl OutputSink for NoSink {
    fn notify(&self, _heading: &str, _body: &str) {}
    fn large_type(&self, _text: &str) {}
    fn text_view(&self, _heading: &str, _text: &str) {}
}

/// What flows between nodes: the argument and the variables.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Ctx {
    pub arg: String,
    pub vars: BTreeMap<String, String>,
}

impl Ctx {
    pub fn with_arg(arg: impl Into<String>) -> Self {
        Self {
            arg: arg.into(),
            vars: BTreeMap::new(),
        }
    }
}

// By hand: the argument is what the user typed or selected.
impl fmt::Debug for Ctx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ctx")
            .field("arg_bytes", &self.arg.len())
            .field("vars", &self.vars.len())
            .finish()
    }
}

/// A node that could not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeFailure {
    pub node: String,
    pub message: String,
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    /// The ids of the nodes that ran, in the order they started.
    pub executed: Vec<String>,
    pub errors: Vec<NodeFailure>,
}

struct Step {
    ctx: Ctx,
    port: Cow<'static, str>,
}

/// What a whole call chain may spend: every `call_external` shares the
/// budget of the run that started it, so nesting cannot multiply the caps.
struct Budget {
    deadline: Instant,
    steps: AtomicUsize,
    /// Milliseconds spent waiting for the user in a `show_ui` node. The run
    /// limit is about a workflow doing work, not about a person reading.
    paused_ms: AtomicU64,
}

impl Budget {
    fn expired(&self) -> bool {
        Instant::now()
            > self.deadline + Duration::from_millis(self.paused_ms.load(Ordering::SeqCst))
    }
}

struct Run {
    report: RunReport,
    budget: Arc<Budget>,
    stopped: bool,
    /// How many `call_external` nodes this run is inside of.
    depth: usize,
}

/// The runtimes that may run now, by folder; what `call_external` looks in.
pub type Registry = Mutex<HashMap<String, Arc<Runtime>>>;

type RandomSource = Box<dyn FnMut() -> Option<u64> + Send>;

fn os_random() -> Option<u64> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// One workflow, ready to run.
pub struct Runtime {
    /// The workflow's folder name (its id in logs and `--trigger`).
    pub folder: String,
    pub dir: PathBuf,
    /// A folder the workflow's scripts may keep files in.
    pub data_dir: PathBuf,
    pub workflow: Workflow,
    platform: Arc<dyn PlatformProvider>,
    sink: Arc<dyn OutputSink>,
    shell: ShellConfig,
    restore_clipboard: bool,
    cancelled: AtomicBool,
    random: Mutex<RandomSource>,
    registry: Mutex<Option<Weak<Registry>>>,
}

impl Runtime {
    pub fn new(
        folder: &str,
        dir: PathBuf,
        data_dir: PathBuf,
        workflow: Workflow,
        platform: Arc<dyn PlatformProvider>,
        sink: Arc<dyn OutputSink>,
        config: &Config,
    ) -> Arc<Self> {
        Arc::new(Self {
            folder: folder.to_owned(),
            dir,
            data_dir,
            workflow,
            platform,
            sink,
            shell: config.shell.clone(),
            restore_clipboard: config.paste.restore_clipboard,
            cancelled: AtomicBool::new(false),
            random: Mutex::new(Box::new(os_random)),
            registry: Mutex::new(None),
        })
    }

    /// Replaces the source of random numbers (the operating system's by
    /// default), so tests can be deterministic.
    pub fn set_random_source(&self, source: impl FnMut() -> u64 + Send + 'static) {
        let mut source = source;
        *self.random.lock().unwrap_or_else(|p| p.into_inner()) = Box::new(move || Some(source()));
    }

    /// Tells the runtime where `call_external` finds the other workflows.
    pub fn set_registry(&self, registry: Weak<Registry>) {
        *self.registry.lock().unwrap_or_else(|p| p.into_inner()) = Some(registry);
    }

    fn next_random(&self) -> Result<u64, String> {
        (self.random.lock().unwrap_or_else(|p| p.into_inner()))()
            .ok_or_else(|| "no random numbers are available".to_owned())
    }

    /// The runtime of the workflow in `folder`, when it may run.
    fn find_runtime(&self, folder: &str) -> Result<Arc<Runtime>, String> {
        let registry = self
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .and_then(Weak::upgrade);
        registry
            .and_then(|map| {
                map.lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(folder)
                    .cloned()
            })
            .ok_or_else(|| {
                format!("no workflow \"{folder}\" is ready to run (is it installed, enabled and allowed?)")
            })
    }

    /// The plugin and approval id of this workflow.
    pub fn id(&self) -> String {
        format!("{}:{}", super::model::FAMILY, self.folder)
    }

    /// Stops running workflows of this instance at the next opportunity (their
    /// programs are killed).
    pub fn shutdown(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Runs the nodes after `start` on a thread of their own. Fails at once
    /// (nothing runs) when too many runs are already going.
    pub fn start(self: &Arc<Self>, start: &str, ctx: Ctx) -> Result<(), String> {
        let Some(guard) = RunGuard::acquire() else {
            return Err("too many workflows are running; try again in a moment".to_owned());
        };
        let runtime = Arc::clone(self);
        let start = start.to_owned();
        thread::Builder::new()
            .name("sevak-workflow".to_owned())
            .spawn(move || {
                let _guard = guard;
                let started = Instant::now();
                let report = runtime.run_blocking(&start, ctx);
                tracing::info!(
                    workflow = runtime.folder,
                    trigger = start,
                    steps = report.executed.len(),
                    failed = report.errors.len(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "workflow finished"
                );
                runtime.report_failures(&report);
            })
            .map(drop)
            .map_err(|err| format!("could not start the workflow: {err}"))
    }

    /// Tells the user about the nodes that failed (at most three).
    fn report_failures(&self, report: &RunReport) {
        for failure in report.errors.iter().take(3) {
            let label = self
                .workflow
                .node(&failure.node)
                .map(|node| {
                    if node.title.trim().is_empty() {
                        node.id.clone()
                    } else {
                        node.title.clone()
                    }
                })
                .unwrap_or_else(|| failure.node.clone());
            self.sink.notify(
                &self.display_name(),
                &format!("\"{label}\" failed: {}", failure.message),
            );
        }
    }

    fn display_name(&self) -> String {
        let name = self.workflow.name.trim();
        if name.is_empty() {
            self.folder.clone()
        } else {
            name.to_owned()
        }
    }

    /// Runs everything connected after the trigger or input node `start`, here
    /// and now. `ctx` carries the trigger's argument and variables; the
    /// workflow's own variables fill in whatever it does not set.
    pub fn run_blocking(&self, start: &str, ctx: Ctx) -> RunReport {
        let budget = Arc::new(Budget {
            deadline: Instant::now() + RUN_LIMIT,
            steps: AtomicUsize::new(0),
            paused_ms: AtomicU64::new(0),
        });
        self.run_blocking_at(start, ctx, 0, budget)
    }

    /// [`Runtime::run_blocking`] inside `depth` nested `call_external` nodes,
    /// which spend the `budget` of the run that called them.
    fn run_blocking_at(
        &self,
        start: &str,
        ctx: Ctx,
        depth: usize,
        budget: Arc<Budget>,
    ) -> RunReport {
        let mut vars = self.workflow.variables.clone();
        vars.extend(ctx.vars);
        let ctx = Ctx { arg: ctx.arg, vars };
        let mut run = Run {
            report: RunReport::default(),
            budget,
            stopped: false,
            depth,
        };
        self.follow(start, OUT, &ctx, &mut run);
        run.report
    }

    fn follow(&self, from: &str, port: &str, ctx: &Ctx, run: &mut Run) {
        let targets: Vec<String> = self
            .workflow
            .outgoing(from, port)
            .map(|conn| conn.to.clone())
            .collect();
        for target in targets {
            self.step(&target, ctx.clone(), run);
        }
    }

    fn step(&self, id: &str, ctx: Ctx, run: &mut Run) {
        if run.stopped {
            return;
        }
        let reason = if self.cancelled() {
            Some("Sevak is closing or reloading")
        } else if run.budget.steps.load(Ordering::SeqCst) >= MAX_STEPS {
            Some("the run reached the limit of steps")
        } else if run.budget.expired() {
            Some("the run took too long")
        } else {
            None
        };
        if let Some(reason) = reason {
            run.stopped = true;
            run.report.errors.push(NodeFailure {
                node: id.to_owned(),
                message: format!("stopped: {reason}"),
            });
            tracing::warn!(workflow = self.folder, node = id, "run stopped: {reason}");
            return;
        }
        let Some(node) = self.workflow.node(id) else {
            return;
        };
        run.report.executed.push(id.to_owned());
        run.budget.steps.fetch_add(1, Ordering::SeqCst);
        match self.execute(node, &ctx, run.depth, &run.budget) {
            Ok(step) => self.follow(id, &step.port, &step.ctx, run),
            Err(message) => {
                tracing::warn!(
                    workflow = self.folder,
                    node = id,
                    kind = node.kind.type_name(),
                    "node failed"
                );
                run.report.errors.push(NodeFailure {
                    node: id.to_owned(),
                    message,
                });
            }
        }
    }

    fn execute(
        &self,
        node: &Node,
        ctx: &Ctx,
        depth: usize,
        budget: &Arc<Budget>,
    ) -> Result<Step, String> {
        let scope = Scope {
            query: &ctx.arg,
            vars: &ctx.vars,
        };
        let plain = |text: &str| expand(text, &scope, Target::Plain);
        let mut next = ctx.clone();
        let mut port = OUT;
        let mut dynamic_port: Option<String> = None;
        match &node.kind {
            NodeKind::Keyword { .. }
            | NodeKind::Hotkey { .. }
            | NodeKind::Selection { .. }
            | NodeKind::External {}
            | NodeKind::ScriptFilter { .. }
            | NodeKind::ListFilter { .. }
            | NodeKind::FileFilter { .. }
            | NodeKind::RunningApps { .. } => {
                return Err("a trigger cannot be run as a step".to_owned());
            }
            NodeKind::RunScript {
                command,
                script,
                args,
                stdin,
                env,
                timeout_ms,
                log_stderr,
            } => {
                let launch = match (command.is_empty(), script) {
                    (false, _) => Launch::Command(command.clone()),
                    (true, Some(script)) => Launch::Script(script.clone()),
                    (true, None) => return Err("no program is set".to_owned()),
                };
                let mut argv = resolve_launch(&launch, &self.dir)?;
                // The program itself is never templated; its arguments are.
                let fixed = match &launch {
                    Launch::Command(_) => 1,
                    Launch::Script(_) => argv.len(),
                };
                for arg in argv.iter_mut().skip(fixed) {
                    *arg = plain(arg);
                }
                argv.extend(args.iter().map(|arg| plain(arg)));
                let mut environment = self.environment(ctx);
                for (name, value) in env {
                    // Checked when the workflow is saved too; a hand-edited
                    // file that gets here anyway does not get to set these.
                    if is_reserved_variable(name) {
                        tracing::warn!(
                            workflow = self.folder,
                            node = node.id,
                            "not setting a reserved environment variable"
                        );
                        continue;
                    }
                    environment.insert(name.clone(), plain(value));
                }
                let limits = Limits {
                    timeout: Duration::from_millis(
                        timeout_ms
                            .unwrap_or(DEFAULT_SCRIPT_TIMEOUT_MS)
                            .clamp(100, MAX_SCRIPT_TIMEOUT_MS),
                    ),
                    log_stderr: *log_stderr,
                };
                let output = self.run_program(
                    &node.id,
                    &argv,
                    stdin.as_deref().map(plain),
                    &environment,
                    &limits,
                )?;
                let (arg, vars) = script_output(&output);
                next.arg = arg;
                next.vars.extend(vars);
            }
            NodeKind::OpenUrl { url } => {
                let url = expand(url, &scope, Target::Url);
                self.platform
                    .open_url(url.trim())
                    .map_err(|err| format!("could not open the link: {err}"))?;
            }
            NodeKind::OpenFile { path } => {
                let path = self.resolve_path(&plain(path))?;
                self.platform
                    .open_path(&path)
                    .map_err(|err| format!("could not open the file: {err}"))?;
            }
            NodeKind::LaunchApp { app, args } => {
                let args: Vec<String> = args.iter().map(|arg| plain(arg)).collect();
                self.launch_app(plain(app).trim(), &args)?;
            }
            NodeKind::SystemCommand { command } => self.system_command(command.trim())?,
            NodeKind::TerminalCommand { command } => {
                // Every placeholder is one quoted word for the shell that will
                // read the line, unless the author wrote `|raw`.
                let quoting = self.platform.shell_quoting(&self.shell);
                let line = expand_command(command, &scope, quoting)?;
                self.platform
                    .run_in_terminal(&line, &self.shell)
                    .map_err(|err| format!("could not open a terminal: {err}"))?;
            }
            NodeKind::Copy { text } => {
                self.platform
                    .set_clipboard_text(&plain(text))
                    .map_err(|err| format!("could not copy: {err}"))?;
            }
            NodeKind::Paste {
                text,
                restore_clipboard,
            } => {
                let outcome = self
                    .platform
                    .paste_text(
                        &plain(text),
                        restore_clipboard.unwrap_or(self.restore_clipboard),
                    )
                    .map_err(|err| format!("could not paste: {err}"))?;
                // Copying instead is not a failure: the text is on the clipboard.
                if let PasteOutcome::CopiedOnly(reason) = outcome {
                    tracing::warn!(
                        workflow = self.folder,
                        node = node.id,
                        reason,
                        "copied to the clipboard instead of pasting"
                    );
                }
            }
            NodeKind::SetVariable { name, value } => {
                let name = name.trim();
                if !valid_variable_name(name) {
                    return Err("the variable name is not valid".to_owned());
                }
                next.vars.insert(name.to_owned(), plain(value));
            }
            NodeKind::Transform {
                op,
                input,
                find,
                replace,
                index,
                into,
            } => {
                let result = transform(*op, &plain(input), &plain(find), &plain(replace), *index)?;
                match into
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                {
                    Some(name) => {
                        next.vars.insert(name.to_owned(), result);
                    }
                    None => next.arg = result,
                }
            }
            NodeKind::Conditional {
                left,
                test,
                right,
                ignore_case,
            } => {
                let holds = evaluate(*test, &plain(left), &plain(right), *ignore_case)?;
                port = if holds { THEN } else { ELSE };
            }
            NodeKind::Delay { ms } => {
                if !self.pause(Duration::from_millis((*ms).min(MAX_DELAY_MS))) {
                    return Err("stopped while waiting".to_owned());
                }
            }
            NodeKind::Notification { heading, body } => {
                let heading = plain(heading);
                let heading = if heading.trim().is_empty() {
                    self.display_name()
                } else {
                    heading
                };
                self.sink.notify(&heading, &plain(body));
            }
            NodeKind::LargeType { text } => self.sink.large_type(&plain(text)),
            NodeKind::TextView {
                heading,
                text,
                markdown,
            } => {
                let heading = plain(heading);
                let heading = if heading.trim().is_empty() {
                    self.display_name()
                } else {
                    heading
                };
                self.sink
                    .text_view_markdown(&heading, &plain(text), *markdown);
            }
            NodeKind::SplitArg {
                delimiter,
                custom,
                discard_empty,
                trim,
                prefix,
            } => {
                let items = split_items(&ctx.arg, *delimiter, custom, *trim, *discard_empty)?;
                let prefix = prefix.trim();
                // The numbered variables of an earlier split must not linger.
                next.vars.retain(|name, _| {
                    name.strip_prefix(prefix).is_none_or(|rest| {
                        rest != "count"
                            && (rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()))
                    })
                });
                for (i, item) in items.iter().take(MAX_SPLIT_VARIABLES).enumerate() {
                    next.vars.insert(format!("{prefix}{}", i + 1), item.clone());
                }
                next.vars
                    .insert(format!("{prefix}count"), items.len().to_string());
                next.arg = items.join("\n");
            }
            NodeKind::JoinArg {
                separator,
                discard_empty,
            } => {
                let mut items = list_items(&ctx.arg);
                if *discard_empty {
                    items.retain(|item| !item.trim().is_empty());
                }
                next.arg = items.join(&unescape(&plain(separator)));
            }
            NodeKind::Random {
                mode,
                min,
                max,
                lines,
                variable,
            } => {
                let result = match mode {
                    RandomMode::Number => {
                        if min > max {
                            return Err("the smallest number is above the largest".to_owned());
                        }
                        let span = (i128::from(*max) - i128::from(*min) + 1) as u128;
                        let offset = u128::from(self.next_random()?) % span;
                        (i128::from(*min) + offset as i128).to_string()
                    }
                    RandomMode::Line => {
                        let text = plain(lines);
                        let candidates: Vec<&str> = text
                            .split('\n')
                            .map(|line| line.trim_end_matches('\r'))
                            .filter(|line| !line.trim().is_empty())
                            .collect();
                        if candidates.is_empty() {
                            return Err("there is no line to pick".to_owned());
                        }
                        let at = self.next_random()? % candidates.len() as u64;
                        candidates[at as usize].to_owned()
                    }
                    RandomMode::Uuid => {
                        let mut bytes = [0u8; 16];
                        bytes[..8].copy_from_slice(&self.next_random()?.to_le_bytes());
                        bytes[8..].copy_from_slice(&self.next_random()?.to_le_bytes());
                        crate::example_uuid::format_v4(bytes)
                    }
                };
                match variable
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                {
                    Some(name) => {
                        next.vars.insert(name.to_owned(), result);
                    }
                    None => next.arg = result,
                }
            }
            NodeKind::Expression {
                expression,
                variable,
                set_arg,
            } => {
                let name = variable.trim();
                if !valid_variable_name(name) {
                    return Err("the variable name is not valid".to_owned());
                }
                let value = crate::calculator::evaluate(&plain(expression))
                    .map_err(|err| format!("could not calculate the expression: {err}"))?;
                let result = crate::calculator::format_number(value);
                next.vars.insert(name.to_owned(), result.clone());
                if *set_arg {
                    next.arg = result;
                }
            }
            NodeKind::WriteFile {
                path,
                content,
                mode,
                create_dirs,
            } => {
                let path = self.resolve_data_path(&plain(path))?;
                write_text(&path, &plain(content), *mode, *create_dirs)?;
                next.arg = path.display().to_string();
            }
            NodeKind::Debug { passthrough } => {
                let mut text = format!("argument:\n{}\n\nvariables ({}):", ctx.arg, ctx.vars.len());
                for (name, value) in &ctx.vars {
                    text.push_str(&format!("\n{name} = {value}"));
                }
                self.sink.debug(&text);
                if !*passthrough {
                    next.arg.clear();
                }
            }
            NodeKind::RevealFile { path } => {
                let text = plain(path);
                let paths: Vec<&str> = text
                    .split('\n')
                    .map(|line| line.trim_end_matches('\r'))
                    .filter(|line| !line.trim().is_empty())
                    .collect();
                if paths.is_empty() {
                    return Err("there is no file or folder to show".to_owned());
                }
                for line in paths {
                    let path = self.resolve_path(line)?;
                    self.platform
                        .reveal_path(&path)
                        .map_err(|err| format!("could not show the file: {err}"))?;
                }
            }
            NodeKind::CallExternal {
                workflow,
                trigger,
                arg,
                pass_variables,
            } => {
                if depth >= MAX_CALL_DEPTH {
                    return Err(format!(
                        "workflows are calling each other more than {MAX_CALL_DEPTH} levels deep"
                    ));
                }
                let (workflow, trigger) = (workflow.trim(), trigger.trim());
                let target = self.find_runtime(workflow)?;
                match target.workflow.node(trigger).map(|n| &n.kind) {
                    Some(NodeKind::External {}) => {}
                    Some(_) => {
                        return Err(format!(
                            "\"{trigger}\" in \"{workflow}\" is not an external trigger"
                        ))
                    }
                    None => {
                        return Err(format!("\"{workflow}\" has no node \"{trigger}\""));
                    }
                }
                let called = Ctx {
                    arg: plain(arg),
                    // The target's own (approved) variables win over passed ones.
                    vars: if *pass_variables {
                        ctx.vars
                            .iter()
                            .filter(|(name, _)| !target.workflow.variables.contains_key(*name))
                            .map(|(name, value)| (name.clone(), value.clone()))
                            .collect()
                    } else {
                        BTreeMap::new()
                    },
                };
                let report = target.run_blocking_at(trigger, called, depth + 1, Arc::clone(budget));
                if let Some(failure) = report.errors.first() {
                    return Err(format!(
                        "\"{}\" in \"{workflow}\" failed: {}",
                        failure.node, failure.message
                    ));
                }
            }
            NodeKind::DialogConditional {
                heading,
                message,
                buttons,
            } => {
                let heading = plain(heading);
                let heading = if heading.trim().is_empty() {
                    self.display_name()
                } else {
                    heading
                };
                let labels: Vec<String> = buttons
                    .iter()
                    .take(MAX_DIALOG_BUTTONS)
                    .map(|label| plain(label))
                    .collect();
                // Dismissing the dialog, or a shell that cannot show one, takes
                // the `cancel` branch.
                port = match self.sink.ask(&heading, &plain(message), &labels) {
                    Some(index) if index < labels.len() => DIALOG_PORTS[index],
                    _ => CANCEL,
                };
            }
            NodeKind::ShowUi {
                target,
                definition,
                file,
                scripts,
                titlebar,
                always_on_top,
                ..
            } => {
                let ids: BTreeSet<String> = scripts.keys().cloned().collect();
                let loaded = ui::load(
                    &ui::Source {
                        inline: definition.as_deref(),
                        file: file.as_deref(),
                        dir: &self.dir,
                    },
                    &ids,
                )?;
                let (images, problems) = ui::load_images(&loaded.root, &self.dir);
                if let Some(problem) = problems.first() {
                    self.sink.notify(&self.display_name(), problem);
                }
                let session = UiSession {
                    folder: self.folder.clone(),
                    workflow: self.display_name(),
                    node: node.id.clone(),
                    target: *target,
                    titlebar: *titlebar,
                    always_on_top: *always_on_top,
                    definition: loaded,
                    images,
                    vars: ctx.vars.clone(),
                    query: ctx.arg.clone(),
                    scripts: ids,
                };
                let host = NodeUiHost {
                    runtime: self,
                    node,
                    ctx,
                    session: &session,
                    scripts,
                };
                let started = Instant::now();
                let outcome = self.sink.show_ui(&session, &host);
                budget
                    .paused_ms
                    .fetch_add(started.elapsed().as_millis() as u64, Ordering::SeqCst);
                let outcome = outcome.ok_or("this Sevak cannot show the UI here")?;
                next.arg = outcome.arg.unwrap_or_else(|| outcome.state.to_string());
                dynamic_port = Some(outcome.port);
            }
            NodeKind::Speak { text, voice } => {
                let text = plain(text);
                let voice = voice.as_deref().map(plain);
                let voice = voice.as_deref().map(str::trim).filter(|v| !v.is_empty());
                if !text.trim().is_empty() {
                    self.platform
                        .speak(&text, voice)
                        .map_err(|err| format!("could not speak: {err}"))?;
                }
            }
            NodeKind::PlaySound { path } => {
                let path = self.resolve_path(&plain(path))?;
                if !has_sound_extension(&path) {
                    return Err("that is not a sound file Sevak can play".to_owned());
                }
                if !path.is_file() {
                    return Err("the sound file was not found".to_owned());
                }
                self.platform
                    .play_sound(&path)
                    .map_err(|err| format!("could not play the sound: {err}"))?;
            }
        }
        Ok(Step {
            ctx: next,
            port: dynamic_port.map_or(Cow::Borrowed(port), Cow::Owned),
        })
    }

    /// Sleeps for `total`, waking early when the runtime shuts down. False
    /// when it did.
    fn pause(&self, total: Duration) -> bool {
        let end = Instant::now() + total;
        loop {
            if self.cancelled() {
                return false;
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            thread::sleep(left.min(Duration::from_millis(50)));
        }
    }

    /// A path from a node: `~` is the home folder, a relative path is inside
    /// the workflow's folder.
    /// The absolute path `text` names (relative ones are inside the workflow's
    /// folder). A network path is refused here, before anything asks the file
    /// system about it, unless `[files] allow_network_paths` is on.
    fn resolve_path(&self, text: &str) -> Result<PathBuf, String> {
        let path = expand_home(text.trim(), home_dir().as_deref());
        let path = if path.is_absolute() {
            path
        } else {
            self.dir.join(path)
        };
        sevak_platform::netpath::guard(&path).map_err(|err| err.to_string())?;
        Ok(path)
    }

    /// Where `write_file` writes: `~` is the home folder, a relative path is
    /// inside the workflow's data folder (made when it is missing).
    fn resolve_data_path(&self, text: &str) -> Result<PathBuf, String> {
        if text.trim().is_empty() {
            return Err("no file is set".to_owned());
        }
        let path = expand_home(text.trim(), home_dir().as_deref());
        let path = if path.is_absolute() {
            path
        } else {
            // Best effort: the write reports its own error.
            let _ = std::fs::create_dir_all(&self.data_dir);
            self.data_dir.join(path)
        };
        sevak_platform::netpath::guard(&path).map_err(|err| err.to_string())?;
        Ok(path)
    }

    fn launch_app(&self, app: &str, args: &[String]) -> Result<(), String> {
        if app.is_empty() {
            return Err("no application is set".to_owned());
        }
        let wanted = app.to_lowercase();
        let by_name = self
            .platform
            .list_applications()
            .map_err(|err| format!("could not list the applications: {err}"))?
            .into_iter()
            .find(|entry| entry.name.to_lowercase() == wanted);
        let target = match by_name {
            Some(entry) => entry.target,
            None => {
                let path = self.resolve_path(app)?;
                if !path.exists() {
                    return Err("no application with that name or path was found".to_owned());
                }
                LaunchTarget::Executable {
                    path,
                    args: args.to_vec(),
                    working_dir: None,
                }
            }
        };
        self.platform
            .launch(&target)
            .map_err(|err| format!("could not start it: {err}"))
    }

    fn system_command(&self, key: &str) -> Result<(), String> {
        let command =
            SystemCommand::from_key(key).ok_or_else(|| "unknown system command".to_owned())?;
        if !self.platform.supported_system_commands().contains(&command) {
            return Err("this system command is not available here".to_owned());
        }
        if command.is_destructive()
            && !self.sink.confirm(&format!(
                "The workflow \"{}\" wants to run the system command \"{key}\". Continue?",
                self.display_name()
            ))
        {
            return Err("you declined".to_owned());
        }
        self.platform
            .run_system_command(command)
            .map_err(|err| format!("the command failed: {err}"))
    }

    /// The environment every script of this workflow starts with: its variables
    /// (when they are plain names), the `SEVAK_*` values and the names Alfred
    /// workflows read.
    fn environment(&self, ctx: &Ctx) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        for (name, value) in &ctx.vars {
            if exportable(name, value) {
                env.insert(name.clone(), value.clone());
            }
        }
        if ctx.arg.len() <= MAX_ENV_BYTES {
            env.insert("SEVAK_QUERY".to_owned(), ctx.arg.clone());
        }
        env.insert(
            "SEVAK_VERSION".to_owned(),
            env!("CARGO_PKG_VERSION").to_owned(),
        );
        env.insert("SEVAK_WORKFLOW_ID".to_owned(), self.id());
        env.insert(
            "SEVAK_WORKFLOW_DIR".to_owned(),
            self.dir.display().to_string(),
        );
        env.insert(
            "SEVAK_WORKFLOW_DATA".to_owned(),
            self.data_dir.display().to_string(),
        );
        env.insert("alfred_workflow_bundleid".to_owned(), self.id());
        env.insert("alfred_workflow_name".to_owned(), self.display_name());
        env.insert(
            "alfred_workflow_data".to_owned(),
            self.data_dir.display().to_string(),
        );
        env.insert(
            "alfred_workflow_cache".to_owned(),
            self.data_dir.display().to_string(),
        );
        env
    }

    /// The environment of a script filter: the workflow's own variables and the
    /// `SEVAK_*` values (there is no argument yet).
    pub(super) fn script_environment(&self) -> Vec<(String, String)> {
        let ctx = Ctx {
            arg: String::new(),
            vars: self.workflow.variables.clone(),
        };
        let mut env = self.environment(&ctx);
        env.remove("SEVAK_QUERY");
        env.into_iter().collect()
    }

    /// Runs `argv` in the workflow folder and waits for it, killing it when
    /// the timeout passes or the runtime shuts down.
    fn run_program(
        &self,
        node: &str,
        argv: &[String],
        stdin: Option<String>,
        env: &BTreeMap<String, String>,
        limits: &Limits,
    ) -> Result<Vec<u8>, String> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| "no program is set".to_owned())?;
        // Best effort: a script that needs the folder reports its own error.
        let _ = std::fs::create_dir_all(&self.data_dir);
        let mut command = Command::new(program);
        // Only the base environment; the node's own variables follow below.
        scrub_environment(&mut command, &[]);
        command
            .args(args)
            .current_dir(&self.dir)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .envs(env);
        configure_helper_command(&mut command);
        ProcessTree::prepare(&mut command);
        let mut child = command
            .spawn()
            .map_err(|err| format!("could not start the program: {err}"))?;
        let tree = ProcessTree::adopt(&child);

        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            // On a thread: a program that never reads must not block us.
            let _ = thread::Builder::new()
                .name("sevak-workflow-stdin".to_owned())
                .spawn(move || {
                    let _ = pipe.write_all(text.as_bytes());
                });
        }
        let stdout = collect(child.stdout.take(), MAX_OUTPUT_BYTES);
        let stderr = collect(child.stderr.take(), MAX_STDERR_BYTES);

        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(err) => {
                    reap(&mut child, &tree);
                    return Err(format!("waiting for the program failed: {err}"));
                }
            }
            if self.cancelled() {
                reap(&mut child, &tree);
                return Err("stopped: Sevak is closing or reloading".to_owned());
            }
            if started.elapsed() > limits.timeout {
                reap(&mut child, &tree);
                return Err(format!(
                    "the program did not finish within {} s and was stopped",
                    limits.timeout.as_secs_f64().ceil() as u64
                ));
            }
            thread::sleep(POLL);
        };

        // It ended by itself: leave anything it started running.
        tree.release();
        let output = join(stdout);
        if limits.log_stderr {
            let errors = String::from_utf8_lossy(&join(stderr)).into_owned();
            let shown: String = errors.trim().chars().take(500).collect();
            if !shown.is_empty() {
                tracing::info!(workflow = self.folder, node, "stderr: {shown}");
            }
        }
        if status.success() {
            Ok(output)
        } else {
            Err(match status.code() {
                Some(code) => format!("the program exited with code {code}"),
                None => "the program was stopped by a signal".to_owned(),
            })
        }
    }
}

struct Limits {
    timeout: Duration,
    log_stderr: bool,
}

/// Runs the scripts of one `show_ui` node for its open UI.
struct NodeUiHost<'a> {
    runtime: &'a Runtime,
    node: &'a Node,
    ctx: &'a Ctx,
    session: &'a UiSession,
    scripts: &'a BTreeMap<String, UiScript>,
}

impl UiHost for NodeUiHost<'_> {
    fn run_script(&self, id: &str, input: &serde_json::Value) -> Result<UiUpdate, String> {
        let script = self
            .scripts
            .get(id)
            .ok_or_else(|| format!("the UI has no script \"{id}\""))?;
        let extension = ui::language_extension(&script.language)
            .ok_or_else(|| "the script's language is not supported".to_owned())?;
        let runtime = self.runtime;
        // The source is written fresh for each run, next to the workflow's other
        // data, so the interpreter gets a plain file path (no quoting of source
        // on a command line).
        let folder = runtime.data_dir.join("ui-scripts");
        std::fs::create_dir_all(&folder)
            .map_err(|err| format!("could not prepare the script: {err}"))?;
        let path = folder.join(format!("{}-{id}.{extension}", self.node.id));
        std::fs::write(&path, &script.script)
            .map_err(|err| format!("could not prepare the script: {err}"))?;
        let mut argv = match sevak_platform::process::script_runner(extension) {
            sevak_platform::process::ScriptRunner::Interpreter(mut prefix) => {
                if let Some(first) = prefix.first_mut() {
                    *first = sevak_platform::process::pin_program(first)
                        .map_err(|err| format!("could not start `{first}`: {err}"))?;
                }
                prefix
            }
            sevak_platform::process::ScriptRunner::Direct => {
                return Err(format!(
                    "no interpreter for {} scripts on this system",
                    script.language
                ))
            }
            sevak_platform::process::ScriptRunner::Missing(names) => {
                return Err(format!("none of {} is on PATH", names.join(", ")))
            }
        };
        argv.push(path.to_string_lossy().into_owned());
        let environment = runtime.environment(self.ctx);
        let limits = Limits {
            timeout: Duration::from_millis(DEFAULT_SCRIPT_TIMEOUT_MS),
            log_stderr: false,
        };
        let stdout = runtime.run_program(
            &self.node.id,
            &argv,
            Some(input.to_string()),
            &environment,
            &limits,
        )?;
        let reply = ui::parse_reply(&String::from_utf8_lossy(&stdout))?;
        let mut update = UiUpdate {
            set: reply.set,
            notify: reply.notify,
            close: reply.close,
            ..UiUpdate::default()
        };
        if let Some(root) = reply.root {
            let root = ui::replacement_root(root, &self.session.definition, &self.session.scripts)?;
            let (images, problems) = ui::load_images(&root, &runtime.dir);
            if let Some(problem) = problems.first() {
                update.notify.get_or_insert_with(|| problem.clone());
            }
            update.images = images;
            update.root = Some(root);
        }
        Ok(update)
    }

    fn cancelled(&self) -> bool {
        self.runtime.cancelled()
    }
}

/// Whether a variable becomes an environment variable of the scripts: a plain
/// name that cannot redirect how programs start.
fn exportable(name: &str, value: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !is_reserved_variable(name)
        && value.len() <= MAX_ENV_BYTES
}

/// What a script printed, as the argument and variables for the next node.
/// Plain text is the argument (minus the final newline); Alfred's
/// `{"alfredworkflow": {"arg": ..., "variables": {...}}}` sets both.
pub fn script_output(stdout: &[u8]) -> (String, BTreeMap<String, String>) {
    let text = String::from_utf8_lossy(stdout);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    if text.trim_start().starts_with('{') {
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text)
        {
            if let Some(serde_json::Value::Object(inner)) = map.get("alfredworkflow") {
                let arg = match inner.get("arg") {
                    Some(serde_json::Value::String(arg)) => arg.clone(),
                    Some(serde_json::Value::Number(n)) => n.to_string(),
                    _ => String::new(),
                };
                let vars = inner
                    .get("variables")
                    .and_then(|v| v.as_object())
                    .into_iter()
                    .flatten()
                    .filter(|(name, _)| valid_variable_name(name))
                    .filter_map(|(name, value)| {
                        let text = match value {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Number(n) => n.to_string(),
                            serde_json::Value::Bool(b) => b.to_string(),
                            _ => return None,
                        };
                        Some((name.clone(), text))
                    })
                    .collect();
                return (arg, vars);
            }
        }
    }
    let trimmed = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    (trimmed.to_owned(), BTreeMap::new())
}

/// The items `split_arg` makes of `input`.
fn split_items(
    input: &str,
    delimiter: Delimiter,
    custom: &str,
    trim: bool,
    discard_empty: bool,
) -> Result<Vec<String>, String> {
    let parts: Vec<&str> = match delimiter {
        Delimiter::Newline => input
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect(),
        Delimiter::Comma => input.split(',').collect(),
        Delimiter::Tab => input.split('\t').collect(),
        Delimiter::Space => input.split(' ').collect(),
        Delimiter::Custom => {
            if custom.is_empty() {
                return Err("the text to split at is empty".to_owned());
            }
            input.split(custom).collect()
        }
    };
    Ok(parts
        .into_iter()
        .map(|part| if trim { part.trim() } else { part })
        .filter(|part| !(discard_empty && part.is_empty()))
        .map(str::to_owned)
        .collect())
}

/// The items of a list argument: the elements of a JSON array, or the lines.
fn list_items(arg: &str) -> Vec<String> {
    if arg.trim_start().starts_with('[') {
        if let Ok(serde_json::Value::Array(items)) = serde_json::from_str(arg) {
            return items
                .into_iter()
                .map(|item| match item {
                    serde_json::Value::String(text) => text,
                    serde_json::Value::Null => String::new(),
                    other => other.to_string(),
                })
                .collect();
        }
    }
    arg.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect()
}

/// `\n`, `\t`, `\r` and `\\` in a separator; anything else stays.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Writes `text` to `path` the way a `write_file` node says.
fn write_text(
    path: &std::path::Path,
    text: &str,
    mode: WriteMode,
    create_dirs: bool,
) -> Result<(), String> {
    if create_dirs {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("could not create the folder: {err}"))?;
        }
    }
    let failed = |err: std::io::Error| format!("could not write the file: {err}");
    match mode {
        WriteMode::Overwrite => std::fs::write(path, text).map_err(failed),
        WriteMode::Append => std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .map_err(failed),
        WriteMode::Prepend => {
            let old = match std::fs::metadata(path) {
                Ok(meta) if meta.len() > MAX_PREPEND_BYTES => {
                    return Err("the file is too large to put text in front of".to_owned());
                }
                Ok(_) => std::fs::read(path).map_err(failed)?,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(err) => return Err(failed(err)),
            };
            let mut all = text.as_bytes().to_vec();
            all.extend_from_slice(&old);
            std::fs::write(path, all).map_err(failed)
        }
    }
}

/// Applies a `transform` node's operation.
pub fn transform(
    op: TransformOp,
    input: &str,
    find: &str,
    replace: &str,
    index: i64,
) -> Result<String, String> {
    Ok(match op {
        TransformOp::Set => input.to_owned(),
        TransformOp::Upper => input.to_uppercase(),
        TransformOp::Lower => input.to_lowercase(),
        TransformOp::Title => title_case(input),
        TransformOp::Trim => input.trim().to_owned(),
        TransformOp::UrlEncode => percent_encode(input),
        TransformOp::UrlDecode => percent_decode(input)?,
        TransformOp::Base64Encode => base64_encode(input.as_bytes()),
        TransformOp::Base64Decode => {
            base64_decode(input).ok_or_else(|| "the text is not valid Base64".to_owned())?
        }
        TransformOp::Replace => {
            if find.is_empty() {
                input.to_owned()
            } else {
                input.replace(find, replace)
            }
        }
        TransformOp::RegexReplace => compile_regex(find, false)?
            .replace_all(input, replace)
            .into_owned(),
        TransformOp::FirstLine => input.lines().next().unwrap_or_default().to_owned(),
        TransformOp::Split => {
            if find.is_empty() {
                return Err("the text to split at is empty".to_owned());
            }
            let parts: Vec<&str> = input.split(find).collect();
            let at = if index < 0 {
                parts.len().checked_sub(index.unsigned_abs() as usize)
            } else {
                Some(index as usize)
            };
            at.and_then(|at| parts.get(at))
                .copied()
                .unwrap_or_default()
                .to_owned()
        }
    })
}

/// Decodes `%XX` escapes (anything that is not a valid escape stays as it
/// is). An error when the result is not UTF-8.
fn percent_decode(text: &str) -> Result<String, String> {
    fn hex(byte: u8) -> Option<u8> {
        char::from(byte)
            .to_digit(16)
            .and_then(|digit| u8::try_from(digit).ok())
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).map_err(|_| "the decoded text is not valid UTF-8".to_owned())
}

/// Decodes standard or URL-safe Base64 (padding optional, whitespace ignored)
/// into text. `None` when it is not Base64 or the bytes are not UTF-8.
fn base64_decode(text: &str) -> Option<String> {
    fn value(byte: u8) -> Option<u32> {
        Some(u32::from(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        }))
    }
    let mut data: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let padding = data.iter().rev().take_while(|b| **b == b'=').count();
    if padding > 2 {
        return None;
    }
    data.truncate(data.len() - padding);
    // A remainder of one character cannot come from any input.
    if data.len() % 4 == 1 {
        return None;
    }
    let mut bytes = Vec::with_capacity(data.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in data {
        buffer = (buffer << 6) | value(byte)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    String::from_utf8(bytes).ok()
}

/// Evaluates a `conditional` node's test.
pub fn evaluate(test: Test, left: &str, right: &str, ignore_case: bool) -> Result<bool, String> {
    if test == Test::Matches {
        return Ok(compile_regex(right, ignore_case)?.is_match(left));
    }
    let (left, right) = if ignore_case {
        (left.to_lowercase(), right.to_lowercase())
    } else {
        (left.to_owned(), right.to_owned())
    };
    Ok(match test {
        Test::Equals => left == right,
        Test::NotEquals => left != right,
        Test::Contains => left.contains(&right),
        Test::NotContains => !left.contains(&right),
        Test::StartsWith => left.starts_with(&right),
        Test::EndsWith => left.ends_with(&right),
        Test::IsEmpty => left.trim().is_empty(),
        Test::NotEmpty => !left.trim().is_empty(),
        Test::Matches => unreachable!("handled above"),
    })
}

/// Ends the program and everything it started. The child must not have been
/// reaped yet (see `ProcessTree::kill`).
fn reap(child: &mut Child, tree: &ProcessTree) {
    tree.kill();
    let _ = child.kill();
    let _ = child.wait();
}

/// Reads a pipe to its end on a helper thread, keeping at most `limit` bytes.
fn collect(pipe: Option<impl Read + Send + 'static>, limit: u64) -> Option<Receiver<Vec<u8>>> {
    let mut pipe = pipe?;
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("sevak-workflow-output".to_owned())
        .spawn(move || {
            let mut kept = Vec::new();
            let _ = (&mut pipe).take(limit).read_to_end(&mut kept);
            let _ = tx.send(kept);
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        })
        .ok()?;
    Some(rx)
}

/// The collected bytes. A grandchild that inherited the pipe can keep it open
/// after the program exits, so wait only briefly.
fn join(rx: Option<Receiver<Vec<u8>>>) -> Vec<u8> {
    rx.and_then(|rx| rx.recv_timeout(Duration::from_millis(500)).ok())
        .unwrap_or_default()
}

static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Counts a running workflow against [`MAX_CONCURRENT_RUNS`].
struct RunGuard(&'static AtomicUsize);

impl RunGuard {
    fn acquire() -> Option<Self> {
        Self::acquire_from(&RUNNING, MAX_CONCURRENT_RUNS)
    }

    fn acquire_from(counter: &'static AtomicUsize, max: usize) -> Option<Self> {
        if counter.fetch_add(1, Ordering::SeqCst) >= max {
            counter.fetch_sub(1, Ordering::SeqCst);
            None
        } else {
            Some(Self(counter))
        }
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;
    use crate::workflow::testing::{
        clipboard, copy, fixture, fixture_with, keyword, node, set_var, wire, wire_port,
        RecordingSink,
    };

    #[test]
    fn nodes_run_in_connection_order_depth_first() {
        // k -> a -> a2 ; k -> b ; a -> a3 (declared after a2)
        let f = fixture(
            vec![
                keyword("k"),
                copy("a", "a"),
                copy("b", "b"),
                copy("a2", "a2"),
                copy("a3", "a3"),
            ],
            vec![
                wire("k", "a"),
                wire("k", "b"),
                wire("a", "a2"),
                wire("a", "a3"),
            ],
        );
        let report = f.runtime.run_blocking("k", Ctx::with_arg("x"));
        assert_eq!(report.executed, ["a", "a2", "a3", "b"]);
        assert_eq!(clipboard(&f), ["a", "a2", "a3", "b"]);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn the_argument_and_variables_flow_down_the_chain() {
        let f = fixture(
            vec![
                keyword("k"),
                set_var("v", "greeting", "Hello {query}"),
                node(
                    "t",
                    NodeKind::Transform {
                        op: TransformOp::Upper,
                        input: "{var:greeting}!".into(),
                        find: String::new(),
                        replace: String::new(),
                        index: 0,
                        into: None,
                    },
                ),
                copy("c", "{query} / {var:greeting}"),
            ],
            vec![wire("k", "v"), wire("v", "t"), wire("t", "c")],
        );
        let report = f.runtime.run_blocking("k", Ctx::with_arg("Ada"));
        assert!(report.errors.is_empty(), "{report:?}");
        // The transform replaced the argument; the variable survived.
        assert_eq!(clipboard(&f), ["HELLO ADA! / Hello Ada"]);
    }

    #[test]
    fn branches_do_not_see_each_others_changes() {
        let f = fixture(
            vec![
                keyword("k"),
                set_var("one", "x", "1"),
                copy("c1", "{var:x}"),
                set_var("two", "x", "2"),
                copy("c2", "{var:x}"),
                copy("c3", "[{var:x}]"),
            ],
            vec![
                wire("k", "one"),
                wire("k", "two"),
                wire("k", "c3"),
                wire("one", "c1"),
                wire("two", "c2"),
            ],
        );
        f.runtime.run_blocking("k", Ctx::with_arg(""));
        assert_eq!(clipboard(&f), ["1", "2", "[]"]);
    }

    #[test]
    fn workflow_variables_are_the_starting_values_and_triggers_can_override() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::empty();
        let workflow = Workflow {
            name: "w".into(),
            variables: [
                ("site".to_owned(), "default".to_owned()),
                ("other".to_owned(), "kept".to_owned()),
            ]
            .into_iter()
            .collect(),
            nodes: vec![keyword("k"), copy("c", "{var:site}/{var:other}")],
            connections: vec![wire("k", "c")],
            ..Workflow::default()
        };
        let runtime = Runtime::new(
            "w",
            dir.path().join("wf"),
            dir.path().join("data"),
            workflow,
            platform.clone(),
            Arc::new(NoSink),
            &Config::default(),
        );
        let mut ctx = Ctx::with_arg("");
        ctx.vars.insert("site".into(), "picked".into());
        runtime.run_blocking("k", ctx);
        assert_eq!(*platform.clipboard.lock().unwrap(), ["picked/kept"]);
    }

    fn conditional(id: &str, left: &str, test: Test, right: &str, ignore_case: bool) -> Node {
        node(
            id,
            NodeKind::Conditional {
                left: left.into(),
                test,
                right: right.into(),
                ignore_case,
            },
        )
    }

    #[test]
    fn a_conditional_takes_one_branch() {
        let build = || {
            fixture(
                vec![
                    keyword("k"),
                    conditional("q", "{query}", Test::Matches, "^[0-9]+$", false),
                    copy("number", "number"),
                    copy("text", "text"),
                ],
                vec![
                    wire("k", "q"),
                    wire_port("q", THEN, "number"),
                    wire_port("q", ELSE, "text"),
                ],
            )
        };
        let f = build();
        let report = f.runtime.run_blocking("k", Ctx::with_arg("123"));
        assert_eq!(report.executed, ["q", "number"]);
        assert_eq!(clipboard(&f), ["number"]);
        let f = build();
        let report = f.runtime.run_blocking("k", Ctx::with_arg("12a"));
        assert_eq!(report.executed, ["q", "text"]);
        assert_eq!(clipboard(&f), ["text"]);
    }

    #[test]
    fn every_test_of_a_conditional() {
        let t = |test, left, right, ic| evaluate(test, left, right, ic).unwrap();
        assert!(t(Test::Equals, "a", "a", false));
        assert!(!t(Test::Equals, "a", "A", false));
        assert!(t(Test::Equals, "a", "A", true));
        assert!(t(Test::NotEquals, "a", "b", false));
        assert!(t(Test::Contains, "hello", "ell", false));
        assert!(t(Test::NotContains, "hello", "xyz", false));
        assert!(t(Test::StartsWith, "hello", "he", false));
        assert!(t(Test::EndsWith, "hello", "lo", false));
        assert!(t(Test::IsEmpty, "  \n", "", false));
        assert!(t(Test::NotEmpty, " x ", "", false));
        assert!(t(Test::Matches, "abc123", r"\d{3}$", false));
        assert!(t(Test::Matches, "ABC", "abc", true));
        assert!(!t(Test::Matches, "ABC", "abc", false));
        assert!(evaluate(Test::Matches, "x", "(", false).is_err());
    }

    #[test]
    fn a_failing_node_ends_its_branch_only() {
        let f = fixture(
            vec![
                keyword("k"),
                node(
                    "bad",
                    NodeKind::Transform {
                        op: TransformOp::Base64Decode,
                        input: "{query}".into(),
                        find: String::new(),
                        replace: String::new(),
                        index: 0,
                        into: None,
                    },
                ),
                copy("after-bad", "never"),
                copy("sibling", "ran"),
            ],
            vec![
                wire("k", "bad"),
                wire("bad", "after-bad"),
                wire("k", "sibling"),
            ],
        );
        let report = f.runtime.run_blocking("k", Ctx::with_arg("%%% not base64"));
        assert_eq!(report.executed, ["bad", "sibling"]);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].node, "bad");
        assert_eq!(clipboard(&f), ["ran"]);
    }

    #[test]
    fn start_tells_the_user_what_failed() {
        let f = fixture(
            vec![keyword("k"), copy("open", "x")],
            vec![wire("k", "open")],
        );
        let report = RunReport {
            executed: vec!["open".into()],
            errors: vec![NodeFailure {
                node: "open".into(),
                message: "boom".into(),
            }],
        };
        f.runtime.report_failures(&report);
        let notes = f.sink.notes.lock().unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].0, "Test flow");
        assert_eq!(notes[0].1, "\"open\" failed: boom");
    }

    #[test]
    fn open_url_encodes_the_query_and_open_file_resolves_paths() {
        let f = fixture(
            vec![
                keyword("k"),
                node(
                    "u",
                    NodeKind::OpenUrl {
                        url: "https://example.com/?q={query}&v={var:v|raw}".into(),
                    },
                ),
                set_var("sv", "v", "a b"),
                node(
                    "f",
                    NodeKind::OpenFile {
                        path: "notes/{query}.txt".into(),
                    },
                ),
            ],
            vec![wire("k", "sv"), wire("sv", "u"), wire("k", "f")],
        );
        let report = f.runtime.run_blocking("k", Ctx::with_arg("x y&z"));
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(
            *f.platform.opened_urls.lock().unwrap(),
            ["https://example.com/?q=x%20y%26z&v=a b"]
        );
        assert_eq!(
            *f.platform.opened_paths.lock().unwrap(),
            [f.runtime.dir.join("notes/x y&z.txt")]
        );
    }

    /// Windows: an open-file node whose path is a share (typed by the user or
    /// built from a variable) is refused before the path is touched.
    #[cfg(windows)]
    #[test]
    fn open_file_refuses_network_paths() {
        let f = fixture(
            vec![
                keyword("k"),
                node(
                    "f",
                    NodeKind::OpenFile {
                        path: "{query}".into(),
                    },
                ),
            ],
            vec![wire("k", "f")],
        );
        for typed in [
            r"\\server\share\x.txt",
            "//server/share/x.txt",
            r"\\.\pipe\x",
        ] {
            let report = f.runtime.run_blocking("k", Ctx::with_arg(typed));
            assert_eq!(report.errors.len(), 1, "{typed}: {report:?}");
            assert!(
                report.errors[0]
                    .message
                    .contains("Network paths are turned off")
                    || report.errors[0].message.contains("device path"),
                "{typed}: {report:?}"
            );
        }
        assert!(f.platform.opened_paths.lock().unwrap().is_empty());
    }

    #[test]
    fn paste_and_terminal_and_outputs() {
        let f = fixture(
            vec![
                keyword("k"),
                node(
                    "p",
                    NodeKind::Paste {
                        text: "<{query}>".into(),
                        restore_clipboard: Some(false),
                    },
                ),
                node(
                    "t",
                    NodeKind::TerminalCommand {
                        command: "echo {query|sh}".into(),
                    },
                ),
                node(
                    "n",
                    NodeKind::Notification {
                        heading: String::new(),
                        body: "done {query}".into(),
                    },
                ),
                node(
                    "l",
                    NodeKind::LargeType {
                        text: "BIG {query}".into(),
                    },
                ),
                node(
                    "v",
                    NodeKind::TextView {
                        heading: "Result".into(),
                        text: "{query}!".into(),
                        markdown: false,
                    },
                ),
            ],
            vec![
                wire("k", "p"),
                wire("p", "t"),
                wire("t", "n"),
                wire("n", "l"),
                wire("l", "v"),
            ],
        );
        let report = f.runtime.run_blocking("k", Ctx::with_arg("it's"));
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(
            *f.platform.pasted.lock().unwrap(),
            [("<it's>".to_owned(), false)]
        );
        let runs = f.platform.terminal_runs.lock().unwrap();
        assert_eq!(runs[0].0, "echo 'it'\\''s'");
        assert_eq!(
            *f.sink.notes.lock().unwrap(),
            [("Test flow".to_owned(), "done it's".to_owned())]
        );
        assert_eq!(*f.sink.large.lock().unwrap(), ["BIG it's"]);
        assert_eq!(
            *f.sink.views.lock().unwrap(),
            [("Result".to_owned(), "it's!".to_owned())]
        );
    }

    #[test]
    fn a_text_view_node_passes_its_markdown_flag_on() {
        let view = |id: &str, markdown: bool| {
            node(
                id,
                NodeKind::TextView {
                    heading: String::new(),
                    text: "# {query}".into(),
                    markdown,
                },
            )
        };
        let f = fixture(
            vec![keyword("k"), view("plain", false), view("md", true)],
            vec![wire("k", "plain"), wire("plain", "md")],
        );
        let report = f.runtime.run_blocking("k", Ctx::with_arg("x"));
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(*f.sink.view_markdown.lock().unwrap(), [false, true]);
        assert_eq!(f.sink.views.lock().unwrap()[1].1, "# x");
    }

    #[test]
    fn a_text_view_node_without_the_flag_loads_as_plain_text() {
        let kind: NodeKind = serde_json::from_str(r#"{"type":"text_view","text":"hi"}"#).unwrap();
        assert!(matches!(
            kind,
            NodeKind::TextView {
                markdown: false,
                ..
            }
        ));
        let json = serde_json::to_string(&kind).unwrap();
        assert!(!json.contains("markdown"), "{json}");
        let on: NodeKind =
            serde_json::from_str(r#"{"type":"text_view","text":"hi","markdown":true}"#).unwrap();
        assert!(serde_json::to_string(&on)
            .unwrap()
            .contains(r#""markdown":true"#));
    }

    #[test]
    fn terminal_commands_quote_placeholders_unless_raw() {
        let terminal = |id: &str, command: &str| {
            node(
                id,
                NodeKind::TerminalCommand {
                    command: command.into(),
                },
            )
        };
        let f = fixture(
            vec![
                keyword("k"),
                terminal("quoted", "grep {query} notes.txt"),
                terminal("raw", "git {query|raw}"),
            ],
            vec![wire("k", "quoted"), wire("quoted", "raw")],
        );
        let report = f.runtime.run_blocking("k", Ctx::with_arg("x; rm -rf ~"));
        assert!(report.errors.is_empty(), "{report:?}");
        let runs: Vec<String> = f
            .platform
            .terminal_runs
            .lock()
            .unwrap()
            .iter()
            .map(|(line, _)| line.clone())
            .collect();
        assert_eq!(runs, ["grep 'x; rm -rf ~' notes.txt", "git x; rm -rf ~"]);

        // PowerShell quoting when that is the shell.
        let f = fixture(
            vec![keyword("k"), terminal("t", "Write-Output {query}")],
            vec![wire("k", "t")],
        );
        *f.platform.shell_quoting.lock().unwrap() = Some(sevak_platform::ShellQuoting::PowerShell);
        let report = f.runtime.run_blocking("k", Ctx::with_arg("it's $env:PATH"));
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(
            f.platform.terminal_runs.lock().unwrap()[0].0,
            "Write-Output 'it''s $env:PATH'"
        );

        // cmd cannot quote a %: the node fails and nothing runs.
        let f = fixture(
            vec![keyword("k"), terminal("t", "echo {query}")],
            vec![wire("k", "t")],
        );
        *f.platform.shell_quoting.lock().unwrap() = Some(sevak_platform::ShellQuoting::Cmd);
        let report = f.runtime.run_blocking("k", Ctx::with_arg("%PATH%"));
        assert_eq!(report.errors.len(), 1, "{report:?}");
        assert!(report.errors[0].message.contains("|raw"), "{report:?}");
        assert!(f.platform.terminal_runs.lock().unwrap().is_empty());
    }

    #[test]
    fn system_commands_ask_before_destroying_things() {
        use sevak_platform::SystemCommand as Sys;
        let make = |answer: bool| {
            let f = fixture_with(
                vec![
                    keyword("k"),
                    node(
                        "lock",
                        NodeKind::SystemCommand {
                            command: "lock".into(),
                        },
                    ),
                    node(
                        "off",
                        NodeKind::SystemCommand {
                            command: "shutdown".into(),
                        },
                    ),
                ],
                vec![wire("k", "lock"), wire("k", "off")],
                RecordingSink {
                    answer,
                    ..RecordingSink::default()
                },
            );
            *f.platform.system_commands.lock().unwrap() = vec![Sys::Lock, Sys::ShutDown];
            f
        };
        let f = make(false);
        let report = f.runtime.run_blocking("k", Ctx::default());
        assert_eq!(*f.platform.ran_commands.lock().unwrap(), [Sys::Lock]);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].message, "you declined");
        assert_eq!(f.sink.asked.lock().unwrap().len(), 1);

        let f = make(true);
        f.runtime.run_blocking("k", Ctx::default());
        assert_eq!(
            *f.platform.ran_commands.lock().unwrap(),
            [Sys::Lock, Sys::ShutDown]
        );

        // A command this system lacks fails instead of pretending.
        let f = make(true);
        f.platform.system_commands.lock().unwrap().clear();
        let report = f.runtime.run_blocking("k", Ctx::default());
        assert_eq!(report.errors.len(), 2);
        assert!(f.platform.ran_commands.lock().unwrap().is_empty());
    }

    #[test]
    fn delays_are_bounded_and_interruptible() {
        let f = fixture(
            vec![
                keyword("k"),
                node("d", NodeKind::Delay { ms: 60 }),
                copy("c", "after"),
            ],
            vec![wire("k", "d"), wire("d", "c")],
        );
        let started = Instant::now();
        f.runtime.run_blocking("k", Ctx::default());
        assert!(started.elapsed() >= Duration::from_millis(55));
        assert_eq!(clipboard(&f), ["after"]);

        // Shutting down wakes a long delay at once.
        let f = fixture(
            vec![
                keyword("k"),
                node("d", NodeKind::Delay { ms: MAX_DELAY_MS }),
                copy("c", "never"),
            ],
            vec![wire("k", "d"), wire("d", "c")],
        );
        let runtime = f.runtime.clone();
        let handle = thread::spawn(move || runtime.run_blocking("k", Ctx::default()));
        thread::sleep(Duration::from_millis(100));
        f.runtime.shutdown();
        let started = Instant::now();
        let report = handle.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(clipboard(&f).is_empty());
        assert_eq!(report.errors[0].node, "d");
    }

    #[test]
    fn a_run_is_capped_in_steps() {
        // A ladder of diamonds: every rung runs the next one twice, so a few
        // dozen nodes would run for ages without the limit.
        let mut nodes = vec![keyword("k")];
        let mut connections = Vec::new();
        let mut previous = "k".to_owned();
        for rung in 0..20 {
            let (left, right, join) = (format!("l{rung}"), format!("r{rung}"), format!("j{rung}"));
            for id in [&left, &right, &join] {
                nodes.push(set_var(id, "x", "1"));
            }
            connections.push(wire(&previous, &left));
            connections.push(wire(&previous, &right));
            connections.push(wire(&left, &join));
            connections.push(wire(&right, &join));
            previous = join;
        }
        let f = fixture(nodes, connections);
        let report = f.runtime.run_blocking("k", Ctx::default());
        assert_eq!(report.executed.len(), MAX_STEPS);
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].message.contains("limit of steps"));
    }

    #[test]
    fn transforms() {
        let t = |op, input: &str, find: &str, replace: &str, index| {
            transform(op, input, find, replace, index)
        };
        assert_eq!(t(TransformOp::Set, "a", "", "", 0).unwrap(), "a");
        assert_eq!(t(TransformOp::Upper, "aé", "", "", 0).unwrap(), "AÉ");
        assert_eq!(t(TransformOp::Lower, "AB", "", "", 0).unwrap(), "ab");
        assert_eq!(
            t(TransformOp::Title, "big dog", "", "", 0).unwrap(),
            "Big Dog"
        );
        assert_eq!(t(TransformOp::Trim, "  a ", "", "", 0).unwrap(), "a");
        assert_eq!(
            t(TransformOp::UrlEncode, "a b/é", "", "", 0).unwrap(),
            "a%20b%2F%C3%A9"
        );
        assert_eq!(
            t(TransformOp::UrlDecode, "a%20b", "", "", 0).unwrap(),
            "a b"
        );
        assert!(t(TransformOp::UrlDecode, "%ff%fe", "", "", 0).is_err());
        assert_eq!(
            t(TransformOp::Base64Encode, "hi", "", "", 0).unwrap(),
            "aGk="
        );
        assert_eq!(
            t(TransformOp::Base64Decode, "aGk=", "", "", 0).unwrap(),
            "hi"
        );
        assert!(t(TransformOp::Base64Decode, "***", "", "", 0).is_err());
        assert_eq!(
            t(TransformOp::Replace, "a-b-c", "-", "+", 0).unwrap(),
            "a+b+c"
        );
        assert_eq!(t(TransformOp::Replace, "abc", "", "x", 0).unwrap(), "abc");
        assert_eq!(
            t(
                TransformOp::RegexReplace,
                "2024-05-06",
                r"(\d+)-(\d+)-(\d+)",
                "$3/$2/$1",
                0
            )
            .unwrap(),
            "06/05/2024"
        );
        assert!(t(TransformOp::RegexReplace, "x", "(", "", 0).is_err());
        assert_eq!(
            t(TransformOp::FirstLine, "one\ntwo", "", "", 0).unwrap(),
            "one"
        );
        assert_eq!(t(TransformOp::FirstLine, "", "", "", 0).unwrap(), "");
        assert_eq!(t(TransformOp::Split, "a,b,c", ",", "", 1).unwrap(), "b");
        assert_eq!(t(TransformOp::Split, "a,b,c", ",", "", -1).unwrap(), "c");
        assert_eq!(t(TransformOp::Split, "a,b,c", ",", "", 7).unwrap(), "");
        assert_eq!(t(TransformOp::Split, "a,b,c", ",", "", -7).unwrap(), "");
        assert!(t(TransformOp::Split, "a", "", "", 0).is_err());
    }

    #[test]
    fn a_transform_can_store_into_a_variable() {
        let f = fixture(
            vec![
                keyword("k"),
                node(
                    "t",
                    NodeKind::Transform {
                        op: TransformOp::Split,
                        input: "{query}".into(),
                        find: ":".into(),
                        replace: String::new(),
                        index: 1,
                        into: Some("port".into()),
                    },
                ),
                copy("c", "{query} -> {var:port}"),
            ],
            vec![wire("k", "t"), wire("t", "c")],
        );
        f.runtime.run_blocking("k", Ctx::with_arg("host:8080"));
        assert_eq!(clipboard(&f), ["host:8080 -> 8080"]);
    }

    #[test]
    fn script_output_is_text_or_alfred_json() {
        let (arg, vars) = script_output(b"hello\n");
        assert_eq!((arg.as_str(), vars.len()), ("hello", 0));
        assert_eq!(script_output(b"two\r\n").0, "two");
        assert_eq!(script_output(b"keep\n\n").0, "keep\n");
        assert_eq!(script_output("\u{feff}bom".as_bytes()).0, "bom");
        // JSON that is not an Alfred envelope is just text.
        assert_eq!(script_output(br#"{"a": 1}"#).0, r#"{"a": 1}"#);
        let (arg, vars) = script_output(
            br#"{"alfredworkflow": {"arg": "x", "variables": {"n": 3, "ok": true, "s": "t", "bad name": "u", "nested": {}}}}"#,
        );
        assert_eq!(arg, "x");
        let names: Vec<_> = vars.keys().map(String::as_str).collect();
        assert_eq!(names, ["n", "ok", "s"]);
        assert_eq!(vars["n"], "3");
    }

    #[test]
    fn only_plain_variables_become_environment_variables() {
        assert!(exportable("greeting", "x"));
        assert!(exportable("_x1", "x"));
        for bad in [
            "PATH",
            "path",
            "Home",
            "LD_PRELOAD",
            "DYLD_X",
            "SEVAK_QUERY",
            "BASH_ENV",
            "ENV",
            "PYTHONSTARTUP",
            "PythonPath",
            "PYTHONHOME",
            "NODE_OPTIONS",
            "NODE_PATH",
            "PERL5OPT",
            "PERL5LIB",
            "RUBYOPT",
            "RUBYLIB",
            "JAVA_TOOL_OPTIONS",
            "_JAVA_OPTIONS",
            "GIT_SSH_COMMAND",
            "DOTNET_STARTUP_HOOKS",
            "PSModulePath",
            "HTTPS_PROXY",
            "SSL_CERT_FILE",
            "TMPDIR",
            "ComSpec",
            "PATHEXT",
            "a-b",
            "1a",
            "",
        ] {
            assert!(!exportable(bad, "x"), "{bad}");
        }
        assert!(!exportable("big", &"x".repeat(MAX_ENV_BYTES + 1)));
    }

    fn dialog(buttons: &[&str]) -> Node {
        node(
            "ask",
            NodeKind::DialogConditional {
                heading: "Sure?".into(),
                message: "Delete {query}?".into(),
                buttons: buttons.iter().map(|b| (*b).into()).collect(),
            },
        )
    }

    fn dialog_flow(button: Option<usize>) -> crate::workflow::testing::Fixture {
        fixture_with(
            vec![
                keyword("k"),
                dialog(&["Yes", "No {query}"]),
                copy("one", "one"),
                copy("two", "two"),
                copy("none", "none"),
            ],
            vec![
                wire("k", "ask"),
                wire_port("ask", "button1", "one"),
                wire_port("ask", "button2", "two"),
                wire_port("ask", CANCEL, "none"),
            ],
            RecordingSink {
                button,
                ..RecordingSink::default()
            },
        )
    }

    #[test]
    fn a_dialog_continues_along_the_button_pressed() {
        for (button, expected) in [(Some(0), "one"), (Some(1), "two")] {
            let f = dialog_flow(button);
            let report = f.runtime.run_blocking("k", Ctx::with_arg("x"));
            assert_eq!(clipboard(&f), [expected], "{button:?}");
            assert!(report.errors.is_empty(), "{:?}", report.errors);
        }
        let f = dialog_flow(Some(1));
        f.runtime.run_blocking("k", Ctx::with_arg("x"));
        // Heading, message and buttons are templates.
        assert_eq!(
            *f.sink.dialogs.lock().unwrap(),
            [(
                "Sure?".to_owned(),
                "Delete x?".to_owned(),
                vec!["Yes".to_owned(), "No x".to_owned()]
            )]
        );
    }

    #[test]
    fn a_dismissed_or_impossible_dialog_takes_the_cancel_port() {
        // Dismissed, a button that does not exist, and a sink without dialogs.
        for button in [None, Some(5)] {
            let f = dialog_flow(button);
            f.runtime.run_blocking("k", Ctx::default());
            assert_eq!(clipboard(&f), ["none"], "{button:?}");
        }
    }

    #[test]
    fn a_dialog_with_no_sink_cancels() {
        struct Silent;
        impl OutputSink for Silent {
            fn notify(&self, _: &str, _: &str) {}
            fn large_type(&self, _: &str) {}
            fn text_view(&self, _: &str, _: &str) {}
        }
        assert_eq!(Silent.ask("h", "m", &["a".to_owned()]), None);
        assert_eq!(NoSink.ask("h", "m", &["a".to_owned()]), None);
    }

    fn speak(text: &str, voice: Option<&str>) -> Node {
        node(
            "say",
            NodeKind::Speak {
                text: text.into(),
                voice: voice.map(str::to_owned),
            },
        )
    }

    #[test]
    fn speak_sends_the_expanded_text_and_voice_to_the_platform() {
        let f = fixture(
            vec![keyword("k"), speak("Hello {query}", Some(" {var:v} "))],
            vec![wire("k", "say")],
        );
        let mut ctx = Ctx::with_arg("world");
        ctx.vars.insert("v".into(), "Samantha".into());
        f.runtime.run_blocking("k", ctx);
        assert_eq!(
            *f.platform.spoken.lock().unwrap(),
            [("Hello world".to_owned(), Some("Samantha".to_owned()))]
        );

        // A blank voice means the default one, and nothing is spoken for
        // empty text.
        let f = fixture(
            vec![keyword("k"), speak("{query}", Some("  "))],
            vec![wire("k", "say")],
        );
        f.runtime.run_blocking("k", Ctx::with_arg("hi"));
        f.runtime.run_blocking("k", Ctx::with_arg("  "));
        assert_eq!(
            *f.platform.spoken.lock().unwrap(),
            [("hi".to_owned(), None)]
        );
    }

    #[test]
    fn play_sound_checks_the_extension_and_the_file() {
        let f = fixture(
            vec![
                keyword("k"),
                node(
                    "snd",
                    NodeKind::PlaySound {
                        path: "{query}".into(),
                    },
                ),
            ],
            vec![wire("k", "snd")],
        );
        std::fs::create_dir_all(&f.runtime.dir).unwrap();
        let wav = f.runtime.dir.join("ding.WAV");
        std::fs::write(&wav, b"RIFF").unwrap();
        std::fs::write(f.runtime.dir.join("notes.txt"), b"x").unwrap();

        let report = f
            .runtime
            .run_blocking("k", Ctx::with_arg(wav.to_string_lossy()));
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        // A relative path is inside the workflow folder.
        f.runtime.run_blocking("k", Ctx::with_arg("ding.WAV"));
        assert_eq!(*f.platform.sounds.lock().unwrap(), [wav.clone(), wav]);

        let report = f.runtime.run_blocking("k", Ctx::with_arg("notes.txt"));
        assert!(report.errors[0].message.contains("not a sound file"));
        let report = f.runtime.run_blocking("k", Ctx::with_arg("missing.mp3"));
        assert!(report.errors[0].message.contains("not found"));
        assert_eq!(f.platform.sounds.lock().unwrap().len(), 2);
    }

    #[test]
    fn debug_output_hides_the_argument() {
        let ctx = Ctx::with_arg("my secret");
        assert!(!format!("{ctx:?}").contains("secret"));
    }

    #[test]
    fn at_most_a_few_runs_at_once() {
        // A counter of its own: the real one is shared by parallel tests.
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let first = RunGuard::acquire_from(&COUNTER, 2).unwrap();
        let second = RunGuard::acquire_from(&COUNTER, 2).unwrap();
        assert!(RunGuard::acquire_from(&COUNTER, 2).is_none());
        assert_eq!(
            COUNTER.load(Ordering::SeqCst),
            2,
            "a refusal counts nothing"
        );
        drop(first);
        let third = RunGuard::acquire_from(&COUNTER, 2).unwrap();
        drop((second, third));
        assert_eq!(COUNTER.load(Ordering::SeqCst), 0);
    }
}

#[cfg(test)]
mod utility_node_tests {
    use super::*;
    use crate::test_util::MockPlatform;
    use crate::workflow::model::Connection;
    use crate::workflow::testing::{
        clipboard, copy, fixture, keyword, node, wire, Fixture, RecordingSink,
    };

    fn split(delimiter: Delimiter, custom: &str) -> NodeKind {
        NodeKind::SplitArg {
            delimiter,
            custom: custom.into(),
            discard_empty: true,
            trim: true,
            prefix: "split_".into(),
        }
    }

    /// `k -> n -> c` where `c` copies `show`; returns what was copied.
    fn run_one(kind: NodeKind, show: &str, arg: &str) -> Vec<String> {
        let f = fixture(
            vec![keyword("k"), node("n", kind), copy("c", show)],
            vec![wire("k", "n"), wire("n", "c")],
        );
        f.runtime.run_blocking("k", Ctx::with_arg(arg));
        clipboard(&f)
    }

    fn errors_of(kind: NodeKind, arg: &str) -> Vec<NodeFailure> {
        let f = fixture(vec![keyword("k"), node("n", kind)], vec![wire("k", "n")]);
        f.runtime.run_blocking("k", Ctx::with_arg(arg)).errors
    }

    #[test]
    fn split_arg_makes_a_list_and_numbered_variables() {
        assert_eq!(
            run_one(
                split(Delimiter::Comma, ""),
                "{query}|{var:split_count}|{var:split_1}|{var:split_3}",
                "a, b ,,c"
            ),
            ["a\nb\nc|3|a|c"]
        );
        assert_eq!(
            run_one(
                split(Delimiter::Newline, ""),
                "{var:split_2}",
                "x\r\ny\n\nz"
            ),
            ["y"]
        );
        assert_eq!(
            run_one(split(Delimiter::Tab, ""), "{var:split_count}", "a\tb"),
            ["2"]
        );
        assert_eq!(
            run_one(split(Delimiter::Space, ""), "{var:split_2}", "a  b"),
            ["b"]
        );
        assert_eq!(
            run_one(split(Delimiter::Custom, "::"), "{var:split_2}", "a::b::c"),
            ["b"]
        );
    }

    #[test]
    fn split_arg_options_and_prefix() {
        let kind = NodeKind::SplitArg {
            delimiter: Delimiter::Comma,
            custom: String::new(),
            discard_empty: false,
            trim: false,
            prefix: "p_".into(),
        };
        assert_eq!(
            run_one(
                kind,
                "{var:p_count}[{var:p_1}][{var:p_2}][{var:p_3}]",
                "a, ,b"
            ),
            ["3[a][ ][b]"]
        );
    }

    #[test]
    fn a_second_split_leaves_no_stale_variables() {
        let f = fixture(
            vec![
                keyword("k"),
                node("s1", split(Delimiter::Comma, "")),
                node("s2", split(Delimiter::Comma, "")),
                copy("c", "[{var:split_3}]{var:split_count}"),
            ],
            vec![wire("k", "s1"), wire("s1", "s2"), wire("s2", "c")],
        );
        // The second split sees the first one's list (one item per line), so
        // it holds a single item.
        f.runtime.run_blocking("k", Ctx::with_arg("a,b,c"));
        assert_eq!(clipboard(&f), ["[]1"]);
    }

    #[test]
    fn a_custom_split_needs_its_text() {
        // The editor refuses it; a hand-written file that gets here fails.
        assert!(split_items("a", Delimiter::Custom, "", true, true).is_err());
    }

    fn join(separator: &str, discard_empty: bool) -> NodeKind {
        NodeKind::JoinArg {
            separator: separator.into(),
            discard_empty,
        }
    }

    #[test]
    fn join_arg_joins_lines_and_json_arrays() {
        assert_eq!(
            run_one(join(", ", false), "{query}", "a\nb\n\nc"),
            ["a, b, , c"]
        );
        assert_eq!(
            run_one(join(", ", true), "{query}", "a\nb\n\n \nc"),
            ["a, b, c"]
        );
        assert_eq!(
            run_one(join("-", false), "{query}", r#"["x", 2, true, null]"#),
            ["x-2-true-"]
        );
        // Not valid JSON: it is just lines.
        assert_eq!(run_one(join("+", false), "{query}", "[a\nb"), ["[a+b"]);
    }

    #[test]
    fn join_arg_separator_escapes() {
        assert_eq!(run_one(join(r"\t", false), "{query}", "a\nb"), ["a\tb"]);
        assert_eq!(run_one(join(r"\\", false), "{query}", "a\nb"), [r"a\b"]);
        assert_eq!(run_one(join(r"\n", false), "{query}", "a\nb"), ["a\nb"]);
        assert_eq!(
            unescape(r"x\ny\tz\qw\"),
            "x\ny\tz\\qw\\",
            "unknown escapes stay"
        );
    }

    fn random(
        mode: RandomMode,
        min: i64,
        max: i64,
        lines: &str,
        variable: Option<&str>,
    ) -> NodeKind {
        NodeKind::Random {
            mode,
            min,
            max,
            lines: lines.into(),
            variable: variable.map(Into::into),
        }
    }

    /// A runtime whose random numbers are `values`, in turn.
    fn seeded(kind: NodeKind, show: &str, values: Vec<u64>) -> Vec<String> {
        let f = fixture(
            vec![keyword("k"), node("n", kind), copy("c", show)],
            vec![wire("k", "n"), wire("n", "c")],
        );
        let mut values = values.into_iter().cycle();
        f.runtime
            .set_random_source(move || values.next().unwrap_or_default());
        f.runtime.run_blocking("k", Ctx::with_arg("in"));
        clipboard(&f)
    }

    #[test]
    fn random_numbers_stay_in_range_and_are_inclusive() {
        for (value, expected) in [(0, "5"), (1, "6"), (2, "7"), (3, "5"), (u64::MAX, "5")] {
            assert_eq!(
                seeded(
                    random(RandomMode::Number, 5, 7, "", None),
                    "{query}",
                    vec![value]
                ),
                [expected],
                "{value}"
            );
        }
        assert_eq!(
            seeded(
                random(RandomMode::Number, -2, -2, "", None),
                "{query}",
                vec![9]
            ),
            ["-2"]
        );
        // The whole i64 range does not overflow.
        let all = random(RandomMode::Number, i64::MIN, i64::MAX, "", None);
        assert_eq!(seeded(all, "{query}", vec![0]), [i64::MIN.to_string()]);
    }

    #[test]
    fn random_line_template_and_variable() {
        let kind = random(
            RandomMode::Line,
            0,
            0,
            "red\n\nblue {query}\r\ngreen",
            Some("pick"),
        );
        assert_eq!(
            seeded(kind.clone(), "{query}/{var:pick}", vec![1]),
            ["in/blue in"]
        );
        assert_eq!(seeded(kind, "{var:pick}", vec![5]), ["green"]);
    }

    #[test]
    fn random_uuid_is_version_4_and_follows_the_source() {
        let make = || {
            seeded(
                random(RandomMode::Uuid, 0, 0, "", None),
                "{query}",
                vec![0x0123_4567_89ab_cdef],
            )
        };
        let out = make();
        let uuid = &out[0];
        assert_eq!(uuid.len(), 36, "{uuid}");
        assert_eq!(&uuid[14..15], "4", "{uuid}");
        assert!(matches!(&uuid[19..20], "8" | "9" | "a" | "b"), "{uuid}");
        assert_eq!(make(), out);
    }

    #[test]
    fn random_with_nothing_to_pick_fails() {
        // Valid in the editor, empty once the placeholder is filled in.
        let empty = random(RandomMode::Line, 0, 0, "{var:none}", None);
        assert_eq!(errors_of(empty, "").len(), 1);
    }

    fn expression(text: &str, variable: &str, set_arg: bool) -> NodeKind {
        NodeKind::Expression {
            expression: text.into(),
            variable: variable.into(),
            set_arg,
        }
    }

    #[test]
    fn expression_calculates_after_placeholders() {
        let f = fixture(
            vec![
                keyword("k"),
                node(
                    "s",
                    NodeKind::SetVariable {
                        name: "count".into(),
                        value: "41".into(),
                    },
                ),
                node("e", expression("{var:count} + 1", "next", true)),
                copy("c", "{query}/{var:next}"),
            ],
            vec![wire("k", "s"), wire("s", "e"), wire("e", "c")],
        );
        f.runtime.run_blocking("k", Ctx::with_arg("in"));
        assert_eq!(clipboard(&f), ["42/42"]);
        // Only the variable.
        assert_eq!(
            run_one(
                expression("2 * {query}", "twice", false),
                "{query}/{var:twice}",
                "21"
            ),
            ["21/42"]
        );
    }

    #[test]
    fn an_invalid_expression_is_a_node_error() {
        let failures = errors_of(expression("2 +", "x", true), "");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].node, "n");
        assert!(failures[0].message.contains("expression"), "{failures:?}");
        assert_eq!(errors_of(expression("1 / 0", "x", true), "").len(), 1);
    }

    fn write_file(path: &str, content: &str, mode: WriteMode, create_dirs: bool) -> NodeKind {
        NodeKind::WriteFile {
            path: path.into(),
            content: content.into(),
            mode,
            create_dirs,
        }
    }

    fn data_file(f: &Fixture, name: &str) -> PathBuf {
        f.runtime.data_dir.join(name)
    }

    #[test]
    fn write_file_overwrites_appends_and_prepends_in_the_data_folder() {
        let f = fixture(
            vec![
                keyword("k"),
                node(
                    "a",
                    write_file("notes.txt", "one\n", WriteMode::Overwrite, false),
                ),
                node(
                    "b",
                    write_file("notes.txt", "two\n", WriteMode::Append, false),
                ),
                node(
                    "c",
                    write_file("notes.txt", "zero\n", WriteMode::Prepend, false),
                ),
                copy("done", "{query}"),
            ],
            vec![
                wire("k", "a"),
                wire("a", "b"),
                wire("b", "c"),
                wire("c", "done"),
            ],
        );
        f.runtime.run_blocking("k", Ctx::with_arg("ignored"));
        let path = data_file(&f, "notes.txt");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "zero\none\ntwo\n");
        // The argument that leaves the node is the path.
        assert_eq!(clipboard(&f), [path.display().to_string()]);
    }

    #[test]
    fn write_file_prepend_and_append_create_a_missing_file() {
        let f = fixture(
            vec![
                keyword("k"),
                node("a", write_file("new.txt", "x", WriteMode::Prepend, false)),
                node("b", write_file("also.txt", "y", WriteMode::Append, false)),
            ],
            vec![wire("k", "a"), wire("a", "b")],
        );
        let report = f.runtime.run_blocking("k", Ctx::default());
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(
            std::fs::read_to_string(data_file(&f, "new.txt")).unwrap(),
            "x"
        );
        assert_eq!(
            std::fs::read_to_string(data_file(&f, "also.txt")).unwrap(),
            "y"
        );
    }

    #[test]
    fn write_file_needs_create_dirs_for_new_folders() {
        let f = fixture(
            vec![
                keyword("k"),
                node(
                    "a",
                    write_file("deep/er/x.txt", "{query}", WriteMode::Overwrite, false),
                ),
                node(
                    "b",
                    write_file("deep/er/x.txt", "{query}", WriteMode::Overwrite, true),
                ),
            ],
            vec![wire("k", "a"), wire("k", "b")],
        );
        let report = f.runtime.run_blocking("k", Ctx::with_arg("hi"));
        assert_eq!(report.errors.len(), 1, "{report:?}");
        assert_eq!(report.errors[0].node, "a");
        assert_eq!(
            std::fs::read_to_string(data_file(&f, "deep/er/x.txt")).unwrap(),
            "hi"
        );
    }

    #[test]
    fn write_file_takes_absolute_paths_and_templates() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out-{query}.txt");
        let f = fixture(
            vec![
                keyword("k"),
                node(
                    "w",
                    write_file(
                        &target.display().to_string(),
                        "{query}!",
                        WriteMode::Overwrite,
                        false,
                    ),
                ),
            ],
            vec![wire("k", "w")],
        );
        let report = f.runtime.run_blocking("k", Ctx::with_arg("abc"));
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("out-abc.txt")).unwrap(),
            "abc!"
        );
        let blank = write_file("{var:none}", "x", WriteMode::Overwrite, false);
        assert_eq!(errors_of(blank, "").len(), 1);
    }

    struct DebugSink(Mutex<Vec<String>>);

    impl OutputSink for DebugSink {
        fn notify(&self, _: &str, _: &str) {}
        fn large_type(&self, _: &str) {}
        fn text_view(&self, _: &str, _: &str) {}
        fn debug(&self, text: &str) {
            self.0.lock().unwrap().push(text.to_owned());
        }
    }

    #[test]
    fn debug_shows_the_argument_and_variables_and_passes_through() {
        let dir = tempfile::tempdir().unwrap();
        let sink = Arc::new(DebugSink(Mutex::default()));
        let workflow = Workflow {
            name: "d".into(),
            nodes: vec![
                keyword("k"),
                node("d", NodeKind::Debug { passthrough: true }),
                node("e", NodeKind::Debug { passthrough: false }),
                copy("c", "[{query}]"),
            ],
            connections: vec![wire("k", "d"), wire("d", "e"), wire("e", "c")],
            ..Workflow::default()
        };
        let platform = MockPlatform::empty();
        let runtime = Runtime::new(
            "d",
            dir.path().join("wf"),
            dir.path().join("data"),
            workflow,
            platform.clone(),
            sink.clone(),
            &Config::default(),
        );
        let mut vars = BTreeMap::new();
        vars.insert("who".to_owned(), "Ada".to_owned());
        runtime.run_blocking(
            "k",
            Ctx {
                arg: "hello".into(),
                vars,
            },
        );
        let shown = sink.0.lock().unwrap().clone();
        assert_eq!(shown.len(), 2);
        assert!(
            shown[0].contains("hello") && shown[0].contains("who = Ada"),
            "{shown:?}"
        );
        // `e` clears the argument for what follows.
        assert_eq!(*platform.clipboard.lock().unwrap(), ["[]"]);
    }

    #[test]
    fn the_default_sink_drops_debug_output() {
        NoSink.debug("anything");
        let f = fixture(
            vec![
                keyword("k"),
                node("d", NodeKind::Debug { passthrough: true }),
                copy("c", "{query}"),
            ],
            vec![wire("k", "d"), wire("d", "c")],
        );
        f.runtime.run_blocking("k", Ctx::with_arg("kept"));
        assert_eq!(clipboard(&f), ["kept"]);
    }

    #[test]
    fn reveal_file_shows_every_line() {
        let f = fixture(
            vec![
                keyword("k"),
                node(
                    "r",
                    NodeKind::RevealFile {
                        path: "{query}".into(),
                    },
                ),
            ],
            vec![wire("k", "r")],
        );
        let dir = f._dir.path().to_path_buf();
        let (a, b) = (dir.join("a.txt"), dir.join("b dir"));
        let arg = format!("{}\n\n{}\r\n", a.display(), b.display());
        let report = f.runtime.run_blocking("k", Ctx::with_arg(arg));
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(*f.platform.revealed.lock().unwrap(), [a, b]);
        assert_eq!(
            errors_of(
                NodeKind::RevealFile {
                    path: "{query}".into()
                },
                ""
            )
            .len(),
            1
        );
    }

    // ---- call_external ----------------------------------------------------

    fn flow(nodes: Vec<Node>, connections: Vec<Connection>) -> Workflow {
        Workflow {
            name: "w".into(),
            nodes,
            connections,
            ..Workflow::default()
        }
    }

    /// Runtimes by folder, wired to find each other.
    fn registry(
        dir: &tempfile::TempDir,
        platform: &Arc<MockPlatform>,
        workflows: Vec<(&str, Workflow)>,
    ) -> Arc<Registry> {
        let map: Arc<Registry> = Arc::new(Mutex::new(HashMap::new()));
        for (folder, workflow) in workflows {
            assert!(workflow.is_valid(), "{:?}", workflow.validate());
            let runtime = Runtime::new(
                folder,
                dir.path().join(folder),
                dir.path().join("data").join(folder),
                workflow,
                platform.clone(),
                Arc::new(RecordingSink::default()),
                &Config::default(),
            );
            runtime.set_registry(Arc::downgrade(&map));
            map.lock().unwrap().insert(folder.to_owned(), runtime);
        }
        map
    }

    fn call(workflow: &str, trigger: &str, arg: &str, pass_variables: bool) -> NodeKind {
        NodeKind::CallExternal {
            workflow: workflow.into(),
            trigger: trigger.into(),
            arg: arg.into(),
            pass_variables,
        }
    }

    fn external(id: &str) -> Node {
        node(id, NodeKind::External {})
    }

    fn runtime_of(map: &Arc<Registry>, folder: &str) -> Arc<Runtime> {
        map.lock().unwrap()[folder].clone()
    }

    #[test]
    fn call_external_runs_the_other_workflows_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::empty();
        let map = registry(
            &dir,
            &platform,
            vec![
                (
                    "caller",
                    flow(
                        vec![
                            keyword("k"),
                            node("call", call("callee", "go", "<{query}>", false)),
                            copy("after", "after {query} {var:shared}"),
                        ],
                        vec![wire("k", "call"), wire("call", "after")],
                    ),
                ),
                (
                    "callee",
                    flow(
                        vec![
                            external("go"),
                            copy("c", "callee got {query} / {var:shared}"),
                        ],
                        vec![wire("go", "c")],
                    ),
                ),
            ],
        );
        let mut vars = BTreeMap::new();
        vars.insert("shared".to_owned(), "S".to_owned());
        let report = runtime_of(&map, "caller").run_blocking(
            "k",
            Ctx {
                arg: "x".into(),
                vars,
            },
        );
        assert!(report.errors.is_empty(), "{report:?}");
        // Variables stay home unless asked for; the caller's own arg goes on.
        assert_eq!(
            *platform.clipboard.lock().unwrap(),
            ["callee got <x> / ", "after x S"]
        );
    }

    #[test]
    fn call_external_can_pass_variables() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::empty();
        let map = registry(
            &dir,
            &platform,
            vec![
                (
                    "a",
                    flow(
                        vec![keyword("k"), node("call", call("b", "go", "{query}", true))],
                        vec![wire("k", "call")],
                    ),
                ),
                (
                    "b",
                    flow(
                        vec![external("go"), copy("c", "{var:shared}")],
                        vec![wire("go", "c")],
                    ),
                ),
            ],
        );
        let mut vars = BTreeMap::new();
        vars.insert("shared".to_owned(), "S".to_owned());
        runtime_of(&map, "a").run_blocking(
            "k",
            Ctx {
                arg: String::new(),
                vars,
            },
        );
        assert_eq!(*platform.clipboard.lock().unwrap(), ["S"]);
    }

    #[test]
    fn passed_variables_do_not_override_the_targets_own() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::empty();
        let mut callee = flow(
            vec![external("go"), copy("c", "{var:safe}/{var:extra}")],
            vec![wire("go", "c")],
        );
        callee.variables.insert("safe".to_owned(), "own".to_owned());
        let map = registry(
            &dir,
            &platform,
            vec![
                (
                    "a",
                    flow(
                        vec![keyword("k"), node("call", call("b", "go", "", true))],
                        vec![wire("k", "call")],
                    ),
                ),
                ("b", callee),
            ],
        );
        let mut vars = BTreeMap::new();
        vars.insert("safe".to_owned(), "evil".to_owned());
        vars.insert("extra".to_owned(), "E".to_owned());
        runtime_of(&map, "a").run_blocking(
            "k",
            Ctx {
                arg: String::new(),
                vars,
            },
        );
        assert_eq!(*platform.clipboard.lock().unwrap(), ["own/E"]);
    }

    #[test]
    fn call_external_reports_a_missing_target_and_a_failed_callee() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::empty();
        let map = registry(
            &dir,
            &platform,
            vec![
                (
                    "a",
                    flow(
                        vec![
                            keyword("k"),
                            node("none", call("missing", "go", "", false)),
                            node("notrigger", call("b", "nope", "", false)),
                            node("notexternal", call("b", "c", "", false)),
                            node("fails", call("b", "go", "", false)),
                        ],
                        vec![
                            wire("k", "none"),
                            wire("k", "notrigger"),
                            wire("k", "notexternal"),
                            wire("k", "fails"),
                        ],
                    ),
                ),
                (
                    "b",
                    flow(
                        vec![
                            external("go"),
                            node("bad", expression("1 +", "x", true)),
                            copy("c", "x"),
                        ],
                        vec![wire("go", "bad")],
                    ),
                ),
            ],
        );
        let report = runtime_of(&map, "a").run_blocking("k", Ctx::default());
        let by_node: BTreeMap<&str, &str> = report
            .errors
            .iter()
            .map(|e| (e.node.as_str(), e.message.as_str()))
            .collect();
        assert!(by_node["none"].contains("missing"), "{by_node:?}");
        assert!(by_node["notrigger"].contains("no node"), "{by_node:?}");
        assert!(
            by_node["notexternal"].contains("not an external"),
            "{by_node:?}"
        );
        assert!(by_node["fails"].contains("failed"), "{by_node:?}");
    }

    #[test]
    fn call_external_stops_endless_recursion() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::empty();
        // `loop` calls itself: every level copies once, until the depth limit.
        let map = registry(
            &dir,
            &platform,
            vec![(
                "loop",
                flow(
                    vec![
                        external("go"),
                        copy("c", "level"),
                        node("again", call("loop", "go", "", false)),
                    ],
                    vec![wire("go", "c"), wire("c", "again")],
                ),
            )],
        );
        let report = runtime_of(&map, "loop").run_blocking("go", Ctx::default());
        assert_eq!(
            platform.clipboard.lock().unwrap().len(),
            MAX_CALL_DEPTH + 1,
            "one run per level, including the first"
        );
        assert_eq!(report.errors.len(), 1, "{report:?}");
        assert!(report.errors[0].message.contains("failed"), "{report:?}");
    }

    #[test]
    fn nested_calls_share_the_step_budget_of_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let platform = MockPlatform::empty();
        // Five workflows of 60 steps each, every one calling the next: 300 in
        // all, more than one run may take, although none is over on its own.
        let names: Vec<String> = (0..5).map(|i| format!("w{i}")).collect();
        let workflows = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let start = if i == 0 { "k" } else { "go" };
                let mut nodes = vec![if i == 0 { keyword("k") } else { external("go") }];
                let mut connections = Vec::new();
                let mut previous = start.to_owned();
                for n in 0..59 {
                    let id = format!("s{n}");
                    nodes.push(crate::workflow::testing::set_var(&id, "x", "1"));
                    connections.push(wire(&previous, &id));
                    previous = id;
                }
                if let Some(next) = names.get(i + 1) {
                    nodes.push(node("call", call(next, "go", "", false)));
                    connections.push(wire(&previous, "call"));
                }
                (name.as_str(), flow(nodes, connections))
            })
            .collect();
        let map = registry(&dir, &platform, workflows);
        let report = runtime_of(&map, "w0").run_blocking("k", Ctx::default());
        assert_eq!(report.errors.len(), 1, "{report:?}");
        assert!(
            report.errors[0].message.contains("limit of steps"),
            "{report:?}"
        );
    }

    #[test]
    fn call_external_without_a_registry_fails_cleanly() {
        let f = fixture(
            vec![keyword("k"), node("c", call("other", "go", "", false))],
            vec![wire("k", "c")],
        );
        let report = f.runtime.run_blocking("k", Ctx::default());
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].message.contains("other"), "{report:?}");
    }
}

#[cfg(test)]
mod ui_node_tests {
    use super::*;
    use crate::workflow::testing::{
        clipboard, copy, fixture_with, keyword, node, wire, wire_port, Fixture, RecordingSink,
        UiScripted,
    };
    use serde_json::json;

    const DEFINITION: &str = r#"{
        "state": {"name": ""},
        "root": {"type": "column", "children": [
            {"type": "textfield", "id": "name"},
            {"type": "button", "label": "Save", "events": {"click": {"output": "save"}}},
            {"type": "button", "label": "Run", "events": {"click": {"run": "up"}}}
        ]}
    }"#;

    fn show_ui(scripts: &[(&str, &str, &str)]) -> Node {
        node(
            "ui",
            NodeKind::ShowUi {
                target: UiTarget::Launcher,
                definition: Some(DEFINITION.to_owned()),
                file: None,
                scripts: scripts
                    .iter()
                    .map(|(id, language, script)| {
                        (
                            (*id).to_owned(),
                            UiScript {
                                language: (*language).to_owned(),
                                script: (*script).to_owned(),
                            },
                        )
                    })
                    .collect(),
                ports: Vec::new(),
                titlebar: true,
                always_on_top: false,
            },
        )
    }

    fn flow(scripts: &[(&str, &str, &str)], ui_script: Vec<UiScripted>) -> Fixture {
        let sink = RecordingSink::default();
        sink.ui_script.lock().unwrap().extend(ui_script);
        let f = fixture_with(
            vec![
                keyword("k"),
                show_ui(scripts),
                copy("saved", "saved:{query}"),
                copy("closed", "closed"),
            ],
            vec![
                wire("k", "ui"),
                wire_port("ui", "save", "saved"),
                wire_port("ui", "closed", "closed"),
            ],
            sink,
        );
        std::fs::create_dir_all(&f.runtime.dir).unwrap();
        f
    }

    fn outcome(port: &str, arg: Option<&str>) -> UiOutcome {
        UiOutcome {
            port: port.to_owned(),
            arg: arg.map(str::to_owned),
            state: json!({"name": "Ada"}),
        }
    }

    #[test]
    fn the_workflow_continues_from_the_port_the_ui_chose() {
        let f = flow(
            &[("up", "shell", "echo")],
            vec![UiScripted {
                runs: vec![],
                outcome: outcome("save", None),
            }],
        );
        let report = f.runtime.run_blocking("k", Ctx::with_arg("seed"));
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        // No explicit argument: the state as JSON.
        assert_eq!(clipboard(&f), [r#"saved:{"name":"Ada"}"#]);
        let sessions = f.sink.ui_sessions.lock().unwrap();
        assert_eq!(sessions[0].query, "seed");
        assert_eq!(sessions[0].node, "ui");
        assert_eq!(sessions[0].scripts.len(), 1);
    }

    #[test]
    fn an_explicit_argument_and_the_closed_port() {
        let f = flow(
            &[("up", "shell", "echo")],
            vec![UiScripted {
                runs: vec![],
                outcome: outcome("save", Some("Ada")),
            }],
        );
        f.runtime.run_blocking("k", Ctx::default());
        assert_eq!(clipboard(&f), ["saved:Ada"]);

        let f = flow(
            &[("up", "shell", "echo")],
            vec![UiScripted {
                runs: vec![],
                outcome: outcome("closed", None),
            }],
        );
        f.runtime.run_blocking("k", Ctx::default());
        assert_eq!(clipboard(&f), ["closed"]);
    }

    #[test]
    fn a_sink_that_cannot_show_uis_fails_the_node() {
        let f = flow(&[("up", "shell", "echo")], vec![]);
        let report = f.runtime.run_blocking("k", Ctx::default());
        assert_eq!(report.errors.len(), 1, "{report:?}");
        assert!(
            report.errors[0].message.contains("cannot show"),
            "{report:?}"
        );
        assert!(clipboard(&f).is_empty());
    }

    #[test]
    fn a_broken_definition_is_a_node_error_and_never_reaches_the_sink() {
        let mut broken = show_ui(&[]);
        if let NodeKind::ShowUi {
            definition, file, ..
        } = &mut broken.kind
        {
            *definition = None;
            *file = Some("ui.json".to_owned());
        }
        let f = fixture_with(
            vec![keyword("k"), broken],
            vec![wire("k", "ui")],
            RecordingSink::default(),
        );
        std::fs::create_dir_all(&f.runtime.dir).unwrap();
        std::fs::write(
            f.runtime.dir.join("ui.json"),
            r#"{"root": {"type": "webview"}}"#,
        )
        .unwrap();
        let report = f.runtime.run_blocking("k", Ctx::default());
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].message.contains("not valid"), "{report:?}");
        assert!(f.sink.ui_sessions.lock().unwrap().is_empty());
    }

    #[test]
    fn waiting_for_the_user_does_not_use_up_the_run_limit() {
        let budget = Budget {
            deadline: Instant::now() - Duration::from_secs(5),
            steps: AtomicUsize::new(0),
            paused_ms: AtomicU64::new(0),
        };
        assert!(budget.expired());
        budget.paused_ms.store(60_000, Ordering::SeqCst);
        assert!(!budget.expired());
    }

    #[cfg(unix)]
    #[test]
    fn a_script_gets_the_event_on_stdin_and_its_json_updates_the_ui() {
        let script = r#"
read input
case "$input" in
  *Ada*) echo '{"set": {"out": "got Ada"}, "notify": "hi"}' ;;
  *) echo '{"set": {"out": "no"}}' ;;
esac
"#;
        let f = flow(
            &[("up", "shell", script)],
            vec![UiScripted {
                runs: vec![
                    (
                        "up".to_owned(),
                        json!({"event": "click", "component": "go", "state": {"name": "Ada"}, "vars": {}}),
                    ),
                    ("up".to_owned(), json!({"state": {"name": "Bob"}})),
                    ("missing".to_owned(), json!({})),
                ],
                outcome: outcome("closed", None),
            }],
        );
        let report = f.runtime.run_blocking("k", Ctx::default());
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let updates = f.sink.ui_updates.lock().unwrap();
        let first = updates[0].as_ref().unwrap();
        assert_eq!(first.set["out"], json!("got Ada"));
        assert_eq!(first.notify.as_deref(), Some("hi"));
        assert_eq!(updates[1].as_ref().unwrap().set["out"], json!("no"));
        assert!(updates[2].as_ref().unwrap_err().contains("no script"));
    }

    #[cfg(unix)]
    #[test]
    fn a_script_can_replace_the_whole_ui_and_bad_output_is_an_error() {
        let replace = r#"
cat > /dev/null
echo '{"root": {"type": "label", "text": "new"}}'
"#;
        let invalid = "cat > /dev/null; echo '{\"root\": {\"type\": \"webview\"}}'";
        let text = "cat > /dev/null; echo hello";
        let f = flow(
            &[
                ("a", "shell", replace),
                ("b", "shell", invalid),
                ("c", "shell", text),
                ("up", "shell", "true"),
            ],
            vec![UiScripted {
                runs: ["a", "b", "c"]
                    .map(|id| (id.to_owned(), json!({})))
                    .to_vec(),
                outcome: outcome("closed", None),
            }],
        );
        // The definition names only "up"; scripts a to c are declared by the node.
        f.runtime.run_blocking("k", Ctx::default());
        let updates = f.sink.ui_updates.lock().unwrap();
        assert!(matches!(
            updates[0].as_ref().unwrap().root,
            Some(Component::Label { .. })
        ));
        assert!(updates[1].is_err());
        assert!(updates[2].as_ref().unwrap_err().contains("JSON"));
    }

    #[test]
    fn a_failing_script_reports_its_exit_code() {
        let f = flow(
            &[("up", "shell", "exit 3")],
            vec![UiScripted {
                runs: vec![("up".to_owned(), json!({}))],
                outcome: outcome("closed", None),
            }],
        );
        f.runtime.run_blocking("k", Ctx::default());
        let updates = f.sink.ui_updates.lock().unwrap();
        assert!(updates[0].is_err(), "{updates:?}");
    }
}
