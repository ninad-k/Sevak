//! Running the script: process lifecycle for both modes.
//!
//! A [`Runner`] owns everything about one plugin's script: how to start it, the
//! child process (persistent mode), the crash and restart policy, and the
//! [`Delivery`] that matches answers to queries. [`ScriptPlugin`] is a thin
//! `Plugin` over it.
//!
//! # Threads (persistent mode)
//!
//! | Thread | Job |
//! |---|---|
//! | writer | owns the child's stdin; sends queued lines, watches for a hung or idle script and stops it |
//! | reader | owns stdout; parses answers into the `Delivery`; on EOF reaps the child and applies the restart policy |
//! | stderr | forwards the script's stderr to Sevak's log |
//!
//! The query path only ever pushes onto a channel, so a script that stops
//! reading its stdin cannot block typing.
//!
//! [`ScriptPlugin`]: super::ScriptPlugin

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use sevak_platform::process::{configure_helper_command, scrub_environment};

use super::approvals::script_approval_key;
use super::delivery::Delivery;
use super::items::{convert_items, ItemContext};
use super::manifest::{Format, Manifest, Mode, PROTOCOL};
use super::protocol::{parse_line, FromScript, ToScript, MAX_LINE_BYTES};
use super::{alfred, oneshot};

/// Consecutive failures after which the plugin stays off until the next reload.
pub const MAX_FAILURES: u32 = 5;
const FIRST_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// How often the writer thread looks for a hung or idle script.
const TICK: Duration = Duration::from_millis(500);
/// After `shutdown` the script gets this long to exit before it is killed.
const GRACE: Duration = Duration::from_millis(500);
/// Stderr lines logged per process before the rest is dropped.
const MAX_STDERR_LINES: usize = 200;
/// Non-protocol stdout lines logged per process.
const MAX_BAD_LINES: usize = 5;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Everything fixed about a plugin: its manifest and where it lives.
#[derive(Debug, Clone)]
pub struct Spec {
    pub manifest: Manifest,
    /// The plugin folder; also the script's working directory.
    pub dir: PathBuf,
    /// A per-plugin folder the script may keep state in (`SEVAK_PLUGIN_DATA`).
    pub data_dir: PathBuf,
    /// Extra environment for the script: a workflow's variables.
    pub env: Vec<(String, String)>,
    /// The approval key this plugin was allowed under. It is checked again
    /// every time a process starts, so a script replaced after Sevak loaded
    /// the plugin does not run until the user has reviewed it (a reload asks).
    /// `None` for workflow script nodes, which have their own approval.
    pub expected_key: Option<String>,
}

impl Spec {
    /// Fails when the plugin's files no longer match what was allowed.
    fn check_approval(&self) -> Result<(), String> {
        let Some(expected) = &self.expected_key else {
            return Ok(());
        };
        match script_approval_key(&self.dir) {
            Ok((_, key)) if key == *expected => Ok(()),
            _ => Err(
                "the plugin changed after it was allowed; choose Reload index to review it"
                    .to_owned(),
            ),
        }
    }

    pub fn item_context(&self) -> ItemContext<'_> {
        ItemContext {
            plugin_id: &self.manifest.id,
            dir: &self.dir,
            allow_custom: self.manifest.mode == Mode::Persistent,
        }
    }

    /// The command to start the script; `query` is appended as the last
    /// argument (one-shot mode).
    pub fn command(&self, query: Option<&str>) -> Result<Command, String> {
        self.check_approval()?;
        let argv = self.manifest.resolve_argv(&self.dir)?;
        let mut command = Command::new(&argv[0]);
        // A scrubbed environment: only the base set and what the manifest
        // asks for (`inherit_env`), then the plugin's own variables below.
        scrub_environment(&mut command, &self.manifest.inherit_env);
        command.args(&argv[1..]);
        if let Some(query) = query {
            command.arg(query);
        }
        // Best effort: a script that wants the folder reports its own error.
        let _ = std::fs::create_dir_all(&self.data_dir);
        command
            .current_dir(&self.dir)
            .env("SEVAK_VERSION", env!("CARGO_PKG_VERSION"))
            .env("SEVAK_PLUGIN_ID", &self.manifest.id)
            .env("SEVAK_PLUGIN_DIR", &self.dir)
            .env("SEVAK_PLUGIN_DATA", &self.data_dir);
        if matches!(
            self.manifest.format,
            Format::Alfred | Format::AlfredWorkflow
        ) {
            // The variables Alfred workflows read.
            command
                .env("alfred_workflow_bundleid", &self.manifest.id)
                .env("alfred_workflow_name", &self.manifest.name)
                .env("alfred_workflow_data", &self.data_dir)
                .env("alfred_workflow_cache", &self.data_dir);
        }
        command.envs(self.env.iter().map(|(name, value)| (name, value)));
        configure_helper_command(&mut command);
        Ok(command)
    }
}

/// A message for the writer thread.
enum Outgoing {
    Line(String),
    /// Send `shutdown`, give the script a moment, then kill it.
    Stop,
}

/// One running child process (persistent mode).
pub struct Link {
    generation: u64,
    tx: Sender<Outgoing>,
    child: Mutex<Child>,
    alive: AtomicBool,
    /// We asked it to stop: its exit is not a failure.
    stopping: AtomicBool,
    last_activity: Mutex<Instant>,
    /// When the oldest query still without any reply was sent.
    unanswered_since: Mutex<Option<Instant>>,
}

impl Link {
    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn send(&self, line: String) -> bool {
        self.tx.send(Outgoing::Line(line)).is_ok()
    }

    fn kill(&self) {
        // Already exited is fine.
        let _ = lock(&self.child).kill();
    }

    fn touch(&self) {
        *lock(&self.last_activity) = Instant::now();
    }

    fn mark_query_sent(&self) {
        self.touch();
        lock(&self.unanswered_since).get_or_insert_with(Instant::now);
    }

    fn mark_replied(&self) {
        *lock(&self.unanswered_since) = None;
    }

    /// Whether a query has gone unanswered for longer than `limit`.
    fn is_hung(&self, limit: Duration) -> bool {
        lock(&self.unanswered_since).is_some_and(|since| since.elapsed() > limit)
    }
}

enum Phase {
    /// Not running (never started, or stopped on purpose).
    Stopped,
    Running(Arc<Link>),
    /// Crashed; do not restart before `until`.
    Backoff {
        until: Instant,
    },
    /// Too many failures in a row: off until the next reload.
    Disabled,
}

struct State {
    phase: Phase,
    /// Consecutive failures; reset by a good answer.
    failures: u32,
}

/// The delay before restart number `failures` (1-based): 0.5 s, 1 s, 2 s, ...
/// up to a minute.
pub fn backoff(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(16);
    FIRST_BACKOFF
        .saturating_mul(1 << doublings)
        .min(MAX_BACKOFF)
}

pub struct Runner {
    pub spec: Spec,
    pub delivery: Delivery,
    state: Mutex<State>,
    generation: AtomicU64,
}

impl Runner {
    pub fn new(spec: Spec) -> Arc<Self> {
        Arc::new(Self {
            delivery: Delivery::new(&spec.manifest.id),
            spec,
            state: Mutex::new(State {
                phase: Phase::Stopped,
                failures: 0,
            }),
            generation: AtomicU64::new(0),
        })
    }

    /// Hands request `id` (`input` is the query text) to the script. Returns
    /// false when the script cannot be reached right now (not started and
    /// failing, in backoff, or disabled); the caller then answers with nothing.
    pub fn request(self: &Arc<Self>, id: u64, input: &str) -> bool {
        match self.spec.manifest.mode {
            Mode::Oneshot => {
                if !self.can_start() {
                    return false;
                }
                oneshot::spawn_query(self, id, input.to_owned());
                true
            }
            Mode::Persistent => {
                let Some(link) = self.running_link() else {
                    return false;
                };
                if link.is_hung(self.spec.manifest.hard_timeout) {
                    self.report_hung(&link);
                    return false;
                }
                link.mark_query_sent();
                link.send(
                    ToScript::Query {
                        request_id: id,
                        input,
                    }
                    .to_line(),
                )
            }
        }
    }

    /// Forwards a `custom` action to the script (persistent mode).
    pub fn send_execute(self: &Arc<Self>, key: &str, payload: &str) -> Result<(), String> {
        if self.spec.manifest.mode != Mode::Persistent {
            return Err("only persistent plugins handle custom actions".to_owned());
        }
        let link = self
            .running_link()
            .ok_or_else(|| format!("the {} plugin is not running", self.spec.manifest.name))?;
        link.touch();
        if link.send(ToScript::Execute { key, payload }.to_line()) {
            Ok(())
        } else {
            Err(format!("the {} plugin stopped", self.spec.manifest.name))
        }
    }

    /// Stops the script. With `wait`, blocks a moment until it has exited.
    pub fn stop(&self, wait: bool) {
        let link = {
            let mut state = lock(&self.state);
            match std::mem::replace(&mut state.phase, Phase::Stopped) {
                Phase::Running(link) => Some(link),
                other => {
                    state.phase = other;
                    None
                }
            }
        };
        let Some(link) = link else { return };
        link.stopping.store(true, Ordering::SeqCst);
        let _ = link.tx.send(Outgoing::Stop);
        if wait {
            let deadline = Instant::now() + GRACE + Duration::from_millis(300);
            while link.is_alive() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    // ---- phase bookkeeping -------------------------------------------------

    /// Whether a new process may be started now (one-shot mode).
    pub(super) fn can_start(&self) -> bool {
        let state = lock(&self.state);
        match state.phase {
            Phase::Disabled => false,
            Phase::Backoff { until } => Instant::now() >= until,
            _ => true,
        }
    }

    /// The live process, starting (or restarting) it when allowed.
    fn running_link(self: &Arc<Self>) -> Option<Arc<Link>> {
        let mut state = lock(&self.state);
        match &state.phase {
            Phase::Running(link) if link.is_alive() && !link.stopping.load(Ordering::SeqCst) => {
                return Some(Arc::clone(link))
            }
            Phase::Disabled => return None,
            Phase::Backoff { until } if Instant::now() < *until => return None,
            _ => {}
        }
        match self.start() {
            Ok(link) => {
                state.phase = Phase::Running(Arc::clone(&link));
                Some(link)
            }
            Err(err) => {
                tracing::warn!(
                    plugin = self.spec.manifest.id,
                    "could not start the script: {err}"
                );
                self.record_failure(&mut state);
                None
            }
        }
    }

    /// Counts a failure and decides between backoff and giving up.
    fn record_failure(&self, state: &mut State) {
        state.failures += 1;
        if state.failures >= MAX_FAILURES {
            state.phase = Phase::Disabled;
            tracing::error!(
                plugin = self.spec.manifest.id,
                failures = state.failures,
                "the script keeps failing; it stays off until the plugins are reloaded \
                 (tray: Reload index)"
            );
        } else {
            let wait = backoff(state.failures);
            state.phase = Phase::Backoff {
                until: Instant::now() + wait,
            };
            tracing::warn!(
                plugin = self.spec.manifest.id,
                failures = state.failures,
                retry_in_ms = wait.as_millis() as u64,
                "the script failed; retrying later"
            );
        }
    }

    pub(super) fn note_failure(&self) {
        let mut state = lock(&self.state);
        self.record_failure(&mut state);
    }

    pub(super) fn note_success(&self) {
        let mut state = lock(&self.state);
        state.failures = 0;
        if matches!(state.phase, Phase::Backoff { .. }) {
            state.phase = Phase::Stopped;
        }
    }

    fn on_exit(&self, link: &Link) {
        let mut state = lock(&self.state);
        let current = matches!(&state.phase, Phase::Running(l) if l.generation == link.generation);
        if !current {
            return;
        }
        if link.stopping.load(Ordering::SeqCst) {
            state.phase = Phase::Stopped;
        } else {
            tracing::warn!(
                plugin = self.spec.manifest.id,
                "the script exited unexpectedly"
            );
            self.record_failure(&mut state);
        }
    }

    fn report_hung(&self, link: &Link) {
        tracing::warn!(
            plugin = self.spec.manifest.id,
            timeout_ms = self.spec.manifest.hard_timeout.as_millis() as u64,
            "the script did not answer in time; stopping it"
        );
        link.kill();
    }

    // ---- starting the process ----------------------------------------------

    fn start(self: &Arc<Self>) -> Result<Arc<Link>, String> {
        let mut command = self.spec.command(None)?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|err| format!("{}: {err}", self.spec.manifest.command_line()))?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            return Err("the script's pipes are unavailable".to_owned());
        };

        let (tx, rx) = mpsc::channel();
        let link = Arc::new(Link {
            generation: self.generation.fetch_add(1, Ordering::SeqCst) + 1,
            tx,
            child: Mutex::new(child),
            alive: AtomicBool::new(true),
            stopping: AtomicBool::new(false),
            last_activity: Mutex::new(Instant::now()),
            unanswered_since: Mutex::new(None),
        });
        tracing::info!(plugin = self.spec.manifest.id, "script started");

        link.send(
            ToScript::Initialize {
                protocol: PROTOCOL,
                sevak_version: env!("CARGO_PKG_VERSION"),
                plugin_id: &self.spec.manifest.id,
            }
            .to_line(),
        );

        let name = &self.spec.manifest.id;
        let spawned = [
            self.thread("writer", {
                let (runner, link) = (Arc::clone(self), Arc::clone(&link));
                move || runner.write_loop(&link, stdin, &rx)
            }),
            self.thread("reader", {
                let (runner, link) = (Arc::clone(self), Arc::clone(&link));
                move || runner.read_loop(&link, stdout)
            }),
            self.thread("stderr", {
                let name = name.clone();
                move || log_stderr(&name, stderr)
            }),
        ];
        if spawned.iter().any(Option::is_none) {
            link.kill();
            return Err("could not start the script's helper threads".to_owned());
        }
        Ok(link)
    }

    fn thread(&self, role: &str, work: impl FnOnce() + Send + 'static) -> Option<()> {
        let name = format!("sevak-script-{}-{role}", self.spec.manifest.id);
        thread::Builder::new()
            .name(name)
            .spawn(work)
            .map_err(|err| tracing::error!("could not start a script thread: {err}"))
            .ok()
            .map(drop)
    }

    // ---- the writer thread -------------------------------------------------

    fn write_loop(&self, link: &Link, mut stdin: ChildStdin, rx: &Receiver<Outgoing>) {
        let manifest = &self.spec.manifest;
        loop {
            match rx.recv_timeout(TICK) {
                Ok(Outgoing::Line(mut line)) => {
                    line.push('\n');
                    if let Err(err) = stdin
                        .write_all(line.as_bytes())
                        .and_then(|()| stdin.flush())
                    {
                        tracing::debug!(plugin = manifest.id, %err, "writing to the script failed");
                        break;
                    }
                }
                Ok(Outgoing::Stop) | Err(RecvTimeoutError::Disconnected) => {
                    stop_child(link, stdin);
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {
                    if !link.is_alive() {
                        return;
                    }
                    if link.is_hung(manifest.hard_timeout) {
                        self.report_hung(link);
                        return;
                    }
                    let idle = manifest.idle_timeout.is_some_and(|limit| {
                        lock(&link.unanswered_since).is_none()
                            && lock(&link.last_activity).elapsed() > limit
                    });
                    if idle {
                        tracing::info!(plugin = manifest.id, "stopping the idle script");
                        link.stopping.store(true, Ordering::SeqCst);
                        stop_child(link, stdin);
                        return;
                    }
                }
            }
        }
        // The pipe broke: the reader sees the exit and applies the restart policy.
        link.kill();
    }

    // ---- the reader thread -------------------------------------------------

    fn read_loop(&self, link: &Link, stdout: impl Read) {
        let manifest = &self.spec.manifest;
        let ctx = self.spec.item_context();
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        let mut bad_lines = 0;
        loop {
            buf.clear();
            let read = (&mut reader)
                .take(MAX_LINE_BYTES as u64 + 1)
                .read_until(b'\n', &mut buf);
            match read {
                Ok(0) | Err(_) => break,
                Ok(_) if buf.len() > MAX_LINE_BYTES => {
                    tracing::warn!(
                        plugin = manifest.id,
                        "the script sent a line over 1 MiB; stopping it"
                    );
                    break;
                }
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&buf);
            match parse_line(&line) {
                Some(FromScript::Results { request_id, items }) => {
                    link.mark_replied();
                    self.note_success();
                    let items = convert_items(ctx, &items);
                    self.delivery.deliver(request_id, items);
                }
                Some(FromScript::Error {
                    request_id,
                    message,
                }) => {
                    link.mark_replied();
                    tracing::warn!(
                        plugin = manifest.id,
                        "the script reported an error: {message}"
                    );
                    if let Some(id) = request_id {
                        self.delivery.deliver(id, Vec::new());
                    }
                }
                Some(FromScript::Ready {}) => {
                    tracing::debug!(plugin = manifest.id, "the script is ready");
                }
                Some(FromScript::Unknown) => {}
                None => {
                    bad_lines += 1;
                    if bad_lines <= MAX_BAD_LINES {
                        let shown: String = line.trim().chars().take(200).collect();
                        tracing::warn!(
                            plugin = manifest.id,
                            "ignoring stdout that is not a protocol message (stdout is for \
                             the protocol; log to stderr): {shown}"
                        );
                    }
                }
            }
        }
        // EOF (or a violation): make sure it is gone and reap it.
        link.alive.store(false, Ordering::SeqCst);
        link.kill();
        let _ = lock(&link.child).wait();
        self.on_exit(link);
    }
}

/// Sends `shutdown`, closes stdin (EOF is a second hint to exit), gives the
/// script [`GRACE`] to leave, then kills it.
fn stop_child(link: &Link, mut stdin: ChildStdin) {
    let mut line = ToScript::Shutdown.to_line();
    line.push('\n');
    let _ = stdin
        .write_all(line.as_bytes())
        .and_then(|()| stdin.flush());
    drop(stdin);
    let deadline = Instant::now() + GRACE;
    while Instant::now() < deadline {
        if matches!(lock(&link.child).try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    link.kill();
}

/// Forwards the script's stderr to the log, tagged with the plugin id.
fn log_stderr(plugin: &str, stderr: impl Read) {
    let lines = BufReader::new(stderr).lines().map_while(Result::ok);
    for (logged, line) in lines.enumerate() {
        if logged == MAX_STDERR_LINES {
            tracing::info!(plugin, "further stderr output is not logged");
        }
        if logged < MAX_STDERR_LINES {
            let shown: String = line.chars().take(500).collect();
            tracing::info!(plugin, "stderr: {shown}");
        }
    }
}

/// Parses one-shot output into items according to the manifest's format.
pub(super) fn parse_oneshot(
    spec: &Spec,
    stdout: &str,
) -> Result<Vec<sevak_core::ResultItem>, String> {
    let ctx = spec.item_context();
    match spec.manifest.format {
        Format::Alfred => alfred::parse(ctx, stdout),
        Format::AlfredWorkflow => alfred::parse_workflow(ctx, stdout),
        Format::Sevak => {
            let value: serde_json::Value =
                serde_json::from_str(stdout.trim_start_matches('\u{feff}').trim())
                    .map_err(|err| format!("the output is not JSON: {err}"))?;
            let items = match &value {
                serde_json::Value::Array(items) => items.as_slice(),
                serde_json::Value::Object(map) => match map.get("items") {
                    Some(serde_json::Value::Array(items)) => items.as_slice(),
                    _ => return Err("the output has no \"items\" array".to_owned()),
                },
                _ => return Err("the output must be an object with \"items\"".to_owned()),
            };
            Ok(convert_items(ctx, items))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let ms = |n| backoff(n).as_millis();
        assert_eq!(ms(1), 500);
        assert_eq!(ms(2), 1_000);
        assert_eq!(ms(3), 2_000);
        assert_eq!(ms(4), 4_000);
        assert_eq!(backoff(10), MAX_BACKOFF);
        assert_eq!(backoff(u32::MAX), MAX_BACKOFF);
        assert_eq!(backoff(0), FIRST_BACKOFF);
    }

    fn spec(extra: &str) -> Spec {
        let manifest = Manifest::parse(
            &format!("protocol = 1\nkeyword = \"k\"\ncommand = [\"x\"]\n{extra}"),
            "t",
        )
        .unwrap();
        Spec {
            manifest,
            dir: PathBuf::from("plugin"),
            data_dir: PathBuf::from("data"),
            env: Vec::new(),
            expected_key: None,
        }
    }

    #[test]
    fn oneshot_sevak_output_accepts_an_object_or_an_array() {
        let spec = spec("mode = \"oneshot\"");
        let object = parse_oneshot(&spec, r#"{"items":[{"title":"a"},{"title":"b"}]}"#).unwrap();
        assert_eq!(object.len(), 2);
        let array = parse_oneshot(&spec, "\u{feff}[{\"title\":\"a\"}]\n").unwrap();
        assert_eq!(array.len(), 1);
        assert!(parse_oneshot(&spec, "oops").is_err());
        assert!(parse_oneshot(&spec, r#"{"nope":1}"#).is_err());
        assert!(parse_oneshot(&spec, "42").is_err());
    }

    #[test]
    fn oneshot_alfred_output_goes_through_the_alfred_mapper() {
        let spec = spec("mode = \"oneshot\"\nformat = \"alfred\"");
        let items = parse_oneshot(
            &spec,
            r#"{"items":[{"title":"a","arg":"https://x.example"}]}"#,
        )
        .unwrap();
        assert!(matches!(
            items[0].action,
            sevak_core::Action::OpenUrl { .. }
        ));
    }

    #[test]
    fn custom_actions_are_only_allowed_in_persistent_mode() {
        assert!(spec("").item_context().allow_custom);
        assert!(!spec("mode = \"oneshot\"").item_context().allow_custom);
    }

    #[test]
    fn query_is_passed_as_the_last_argument_with_the_plugin_environment() {
        let mut spec = spec("mode = \"oneshot\"\nformat = \"alfred\"");
        spec.manifest.launch =
            super::super::manifest::Launch::Command(vec!["prog".into(), "a".into()]);
        let command = spec.command(Some("hello world")).unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["a", "hello world"]);
        let env = |name: &str| {
            command
                .get_envs()
                .find(|(key, _)| *key == name)
                .and_then(|(_, value)| value.map(|v| v.to_string_lossy().into_owned()))
        };
        assert_eq!(env("SEVAK_PLUGIN_ID").as_deref(), Some("script:t"));
        assert_eq!(env("alfred_workflow_bundleid").as_deref(), Some("script:t"));
        assert!(env("SEVAK_PLUGIN_DIR").is_some());
        assert!(env("SEVAK_VERSION").is_some());
    }

    #[test]
    fn a_missing_program_fails_cleanly_and_eventually_disables_the_plugin() {
        let mut s = spec("");
        s.manifest.launch =
            super::super::manifest::Launch::Command(vec!["sevak-definitely-not-a-program".into()]);
        s.dir = std::env::temp_dir();
        s.data_dir = std::env::temp_dir().join("sevak-script-test-data");
        let runner = Runner::new(s);
        assert!(!runner.request(1, "x"));
        // In backoff: no new attempt yet.
        assert!(!runner.can_start());
        // Fast-forward through the failures.
        for _ in 0..MAX_FAILURES {
            runner.note_failure();
        }
        assert!(matches!(lock(&runner.state).phase, Phase::Disabled));
        assert!(!runner.can_start());
        runner.note_success();
        assert_eq!(lock(&runner.state).failures, 0);
    }
}
