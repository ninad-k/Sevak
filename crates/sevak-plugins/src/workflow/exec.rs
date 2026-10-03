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

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use sevak_core::{Config, LaunchTarget, ShellConfig};
use sevak_platform::process::configure_helper_command;
use sevak_platform::{PasteOutcome, PlatformProvider, SystemCommand};

use super::model::{
    Node, NodeKind, Test, TransformOp, Workflow, DEFAULT_SCRIPT_TIMEOUT_MS, ELSE, MAX_DELAY_MS,
    MAX_SCRIPT_TIMEOUT_MS, OUT, THEN,
};
use super::template::{expand, Scope, Target};
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
/// Output kept from one script run.
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;
const MAX_STDERR_BYTES: u64 = 64 * 1024;
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
    /// Asks the user a yes/no question before something that cannot be undone
    /// (restart, shut down...). Anything but a clear yes is a no, which is what
    /// the default answers.
    fn confirm(&self, _question: &str) -> bool {
        false
    }
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
    port: &'static str,
}

struct Run {
    report: RunReport,
    deadline: Instant,
    stopped: bool,
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
        let mut vars = self.workflow.variables.clone();
        vars.extend(ctx.vars);
        let ctx = Ctx { arg: ctx.arg, vars };
        let mut run = Run {
            report: RunReport::default(),
            deadline: Instant::now() + RUN_LIMIT,
            stopped: false,
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
        } else if run.report.executed.len() >= MAX_STEPS {
            Some("the run reached the limit of steps")
        } else if Instant::now() > run.deadline {
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
        match self.execute(node, &ctx) {
            Ok(step) => self.follow(id, step.port, &step.ctx, run),
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

    fn execute(&self, node: &Node, ctx: &Ctx) -> Result<Step, String> {
        let scope = Scope {
            query: &ctx.arg,
            vars: &ctx.vars,
        };
        let plain = |text: &str| expand(text, &scope, Target::Plain);
        let mut next = ctx.clone();
        let mut port = OUT;
        match &node.kind {
            NodeKind::Keyword { .. }
            | NodeKind::Hotkey { .. }
            | NodeKind::Selection { .. }
            | NodeKind::External {}
            | NodeKind::ScriptFilter { .. } => {
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
                let path = self.resolve_path(&plain(path));
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
                self.platform
                    .run_in_terminal(&plain(command), &self.shell)
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
            NodeKind::TextView { heading, text } => {
                let heading = plain(heading);
                let heading = if heading.trim().is_empty() {
                    self.display_name()
                } else {
                    heading
                };
                self.sink.text_view(&heading, &plain(text));
            }
        }
        Ok(Step { ctx: next, port })
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
    fn resolve_path(&self, text: &str) -> PathBuf {
        let path = expand_home(text.trim(), home_dir().as_deref());
        if path.is_absolute() {
            path
        } else {
            self.dir.join(path)
        }
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
                let path = self.resolve_path(app);
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
        let mut child = command
            .spawn()
            .map_err(|err| format!("could not start the program: {err}"))?;

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
                    reap(&mut child);
                    return Err(format!("waiting for the program failed: {err}"));
                }
            }
            if self.cancelled() {
                reap(&mut child);
                return Err("stopped: Sevak is closing or reloading".to_owned());
            }
            if started.elapsed() > limits.timeout {
                reap(&mut child);
                return Err(format!(
                    "the program did not finish within {} s and was stopped",
                    limits.timeout.as_secs_f64().ceil() as u64
                ));
            }
            thread::sleep(POLL);
        };

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

/// Whether a variable becomes an environment variable of the scripts: a plain
/// name that cannot redirect how programs start.
fn exportable(name: &str, value: &str) -> bool {
    const DENIED: [&str; 14] = [
        "PATH",
        "PATHEXT",
        "COMSPEC",
        "SYSTEMROOT",
        "WINDIR",
        "HOME",
        "USERPROFILE",
        "USER",
        "SHELL",
        "TMP",
        "TEMP",
        "IFS",
        "PYTHONPATH",
        "NODE_OPTIONS",
    ];
    let upper = name.to_ascii_uppercase();
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !DENIED.contains(&upper.as_str())
        && !upper.starts_with("LD_")
        && !upper.starts_with("DYLD_")
        && !upper.starts_with("SEVAK_")
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

fn reap(child: &mut Child) {
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
            "a-b",
            "1a",
            "",
        ] {
            assert!(!exportable(bad, "x"), "{bad}");
        }
        assert!(!exportable("big", &"x".repeat(MAX_ENV_BYTES + 1)));
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
