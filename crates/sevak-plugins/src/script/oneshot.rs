//! One-shot mode: a fresh process per query, the query as the last argument,
//! the answer as one JSON document on stdout (Alfred's model).
//!
//! Each query runs on its own short-lived thread. It first waits a moment
//! ([`DEBOUNCE`]) so a burst of keystrokes starts one process, not ten, and it
//! gives up (killing the process if it already started) as soon as a newer
//! query exists. The answer goes through the plugin's [`Delivery`] like any
//! other, so a slow script never blocks typing.
//!
//! [`Delivery`]: super::delivery::Delivery

use std::io::Read;
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::runner::{parse_oneshot, Runner};

/// Wait this long before starting a process, in case the user types on.
const DEBOUNCE: Duration = Duration::from_millis(30);
/// Output kept from one run; the rest is dropped (and the JSON will not parse).
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;
const MAX_STDERR_BYTES: u64 = 64 * 1024;
const POLL: Duration = Duration::from_millis(5);

/// Runs request `id` (`input` is the query text) in the background.
pub(super) fn spawn_query(runner: &Arc<Runner>, id: u64, input: String) {
    let worker = Arc::clone(runner);
    let started = thread::Builder::new()
        .name("sevak-script-query".to_owned())
        .spawn(move || run(&worker, id, &input));
    if let Err(err) = started {
        tracing::error!("could not start a script query thread: {err}");
        runner.delivery.abandon(id);
    }
}

fn run(runner: &Runner, id: u64, input: &str) {
    let manifest = &runner.spec.manifest;
    thread::sleep(DEBOUNCE);
    if !runner.delivery.is_current(id) {
        return;
    }

    let mut command = match runner.spec.command(Some(input)) {
        Ok(command) => command,
        Err(err) => {
            tracing::warn!(plugin = manifest.id, "cannot run the script: {err}");
            runner.note_failure();
            runner.delivery.deliver(id, Vec::new());
            return;
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            tracing::warn!(plugin = manifest.id, "could not start the script: {err}");
            runner.note_failure();
            runner.delivery.deliver(id, Vec::new());
            return;
        }
    };
    let stdout = collect(child.stdout.take(), MAX_OUTPUT_BYTES);
    let stderr = collect(child.stderr.take(), MAX_STDERR_BYTES);

    let started = Instant::now();
    let finished = loop {
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(plugin = manifest.id, %err, "waiting for the script failed");
                break false;
            }
        }
        if !runner.delivery.is_current(id) {
            // Superseded: nobody wants this answer any more.
            reap(&mut child);
            return;
        }
        if started.elapsed() > manifest.hard_timeout {
            tracing::warn!(
                plugin = manifest.id,
                timeout_ms = manifest.hard_timeout.as_millis() as u64,
                "the script took too long; stopped it"
            );
            reap(&mut child);
            runner.delivery.deliver(id, Vec::new());
            return;
        }
        thread::sleep(POLL);
    };
    if !finished {
        reap(&mut child);
    }

    let output = String::from_utf8_lossy(&join(stdout)).into_owned();
    let errors = String::from_utf8_lossy(&join(stderr)).into_owned();
    if !errors.trim().is_empty() {
        let shown: String = errors.trim().chars().take(500).collect();
        tracing::info!(plugin = manifest.id, "stderr: {shown}");
    }
    match parse_oneshot(&runner.spec, &output) {
        Ok(items) => {
            runner.note_success();
            runner.delivery.deliver(id, items);
        }
        Err(err) => {
            tracing::warn!(plugin = manifest.id, "{err}");
            runner.delivery.deliver(id, Vec::new());
        }
    }
}

fn reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Reads a pipe to its end on a helper thread (so a chatty script cannot block
/// on a full pipe), keeping at most `limit` bytes.
fn collect(pipe: Option<impl Read + Send + 'static>, limit: u64) -> Option<Receiver<Vec<u8>>> {
    let mut pipe = pipe?;
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("sevak-script-output".to_owned())
        .spawn(move || {
            let mut kept = Vec::new();
            let _ = (&mut pipe).take(limit).read_to_end(&mut kept);
            // The answer is complete; hand it over before draining the rest.
            let _ = tx.send(kept);
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        })
        .ok()?;
    Some(rx)
}

/// The collected bytes. A grandchild that inherited the pipe can keep it open
/// after the script exits, so wait only briefly.
fn join(rx: Option<Receiver<Vec<u8>>>) -> Vec<u8> {
    rx.and_then(|rx| rx.recv_timeout(Duration::from_millis(500)).ok())
        .unwrap_or_default()
}
