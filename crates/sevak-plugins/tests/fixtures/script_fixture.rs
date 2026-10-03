//! A tiny script plugin written in Rust, used by `tests/script_plugins.rs`.
//!
//! The integration test needs a script that runs unchanged on Windows, macOS
//! and Linux CI without any interpreter installed, so it is a compiled
//! helper binary. It is not a template for plugin authors: see `examples/plugins/`
//! at the repository root for Python, PowerShell and Node examples.
//!
//! Modes (first argument):
//!
//! - none: a persistent plugin speaking the JSON-lines protocol on stdin/stdout.
//! - `oneshot-sevak <query>`: prints one Sevak-format `{"items": [...]}` document.
//! - `oneshot-alfred <query>`: prints Alfred Script Filter JSON.
//!
//! Special queries: `slow` answers after 800 ms, `crash` exits abruptly,
//! `hang` never answers, `garbage` prints a non-protocol line first.

use std::io::{BufRead, Write};
use std::time::Duration;

use serde_json::{json, Value};

fn pid_note() -> String {
    format!(
        "{} pid {}",
        std::env::var("SEVAK_PLUGIN_ID").unwrap_or_default(),
        std::process::id()
    )
}

fn sevak_items(input: &str) -> Value {
    json!([
        {
            "key": "echo",
            "title": format!("echo: {input}"),
            "subtitle": pid_note(),
            "icon": {"kind": "file", "path": "icon.png"},
            "action": {"type": "copy_text", "text": input},
            "score": 100.0
        },
        {
            "key": "custom",
            "title": format!("custom: {input}"),
            "action": {"type": "custom", "payload": input},
            "score": 50.0
        },
        {
            "key": "escape",
            "title": "icon outside the plugin folder",
            "icon": {"kind": "file", "path": "../icon.png"},
            "score": 10.0
        }
    ])
}

fn pause_for(input: &str) {
    if input == "slow" || input.starts_with("slow") {
        std::thread::sleep(Duration::from_millis(800));
    }
}

fn persistent() {
    let mut out = std::io::stdout().lock();
    let mut send = |value: Value| {
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    };
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match message["type"].as_str() {
            Some("initialize") => send(json!({"type": "ready"})),
            Some("query") => {
                let id = message["request_id"].clone();
                let input = message["input"].as_str().unwrap_or_default();
                match input {
                    "crash" => std::process::exit(3),
                    "hang" => loop {
                        std::thread::sleep(Duration::from_secs(60));
                    },
                    "garbage" => {
                        println!("this is not a protocol message");
                    }
                    _ => pause_for(input),
                }
                send(json!({"type": "results", "request_id": id, "items": sevak_items(input)}));
            }
            Some("execute") => {
                if let Ok(dir) = std::env::var("SEVAK_PLUGIN_DATA") {
                    let note = format!(
                        "{} {}",
                        message["key"].as_str().unwrap_or_default(),
                        message["payload"].as_str().unwrap_or_default()
                    );
                    let _ = std::fs::write(std::path::Path::new(&dir).join("executed.txt"), note);
                }
            }
            Some("shutdown") => {
                if let Ok(dir) = std::env::var("SEVAK_PLUGIN_DATA") {
                    let _ = std::fs::write(std::path::Path::new(&dir).join("shutdown.txt"), "bye");
                }
                return;
            }
            _ => {}
        }
    }
}

/// The programs workflow nodes run in `tests/workflows.rs` (first argument
/// `wf-...`). Returns false for any other mode.
fn workflow_mode(mode: &str, rest: &[String]) -> bool {
    match mode {
        // The arguments after the mode, joined with `|`.
        "wf-echo" => println!("{}", rest.join("|")),
        // The value of the environment variable named by the first argument.
        "wf-env" => println!(
            "{}",
            std::env::var(&rest[0]).unwrap_or_else(|_| "<unset>".into())
        ),
        // Standard input, upper-cased.
        "wf-stdin" => {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text);
            print!("{}", text.to_uppercase());
        }
        // Exits with the code in the first argument, complaining on stderr.
        "wf-exit" => {
            eprintln!("this is private output");
            std::process::exit(rest[0].parse().unwrap_or(1));
        }
        // Sleeps for the milliseconds in the first argument, then prints "late".
        "wf-sleep" => {
            std::thread::sleep(Duration::from_millis(rest[0].parse().unwrap_or(0)));
            println!("late");
        }
        // Prints Alfred's workflow envelope: an argument and variables.
        "wf-envelope" => println!(
            "{}",
            json!({"alfredworkflow": {"arg": "from-envelope", "variables": {"picked": "yes", "n": 2}}})
        ),
        // Writes the first argument into the workflow's data folder.
        "wf-record" => {
            let dir = std::env::var("SEVAK_WORKFLOW_DATA").unwrap_or_default();
            let path = std::path::Path::new(&dir).join("record.txt");
            let mut note = std::fs::read_to_string(&path).unwrap_or_default();
            note.push_str(&rest.join(" "));
            note.push('\n');
            let _ = std::fs::write(path, note);
        }
        // A script filter: rows for the query, with variables, mods and autocomplete.
        "wf-filter" => {
            let query = rest.last().cloned().unwrap_or_default();
            println!(
                "{}",
                json!({"items": [
                    {
                        "uid": "one",
                        "title": format!("one: {query}"),
                        "subtitle": std::env::var("SEVAK_WORKFLOW_ID").unwrap_or_default(),
                        "arg": format!("arg-{query}"),
                        "autocomplete": format!("one {query}"),
                        "variables": {"from": "filter", "site": "row"},
                        "mods": {
                            "alt": {"arg": "alt-arg", "subtitle": "The alt way"},
                            "cmd": {"valid": false}
                        }
                    },
                    {"uid": "info", "title": "only information", "valid": false}
                ]})
            );
        }
        _ => return false,
    }
    true
}

fn main() {
    let all: Vec<String> = std::env::args().skip(1).collect();
    if let Some((mode, rest)) = all.split_first() {
        if mode.starts_with("wf-") && workflow_mode(mode, rest) {
            return;
        }
    }
    let mut args = std::env::args().skip(1);
    let mode = args.next();
    let query = args.next().unwrap_or_default();
    match mode.as_deref() {
        Some("oneshot-sevak") => {
            pause_for(&query);
            println!("{}", json!({"items": sevak_items(&query)}));
        }
        Some("oneshot-alfred") => {
            pause_for(&query);
            println!(
                "{}",
                json!({"items": [
                    {
                        "uid": "link",
                        "title": format!("alfred: {query}"),
                        "subtitle": pid_note(),
                        "arg": format!("https://example.com/?q={query}"),
                        "autocomplete": format!("alfred {query} "),
                        "mods": {
                            "alt": {"arg": format!("copy {query}"), "subtitle": "Copy the text"},
                            "cmd+shift": {"arg": "https://example.com/both", "subtitle": "Open both"}
                        },
                        "icon": {"path": "icon.png"},
                        "valid": true
                    },
                    {"title": "plain text", "arg": "just text"}
                ]})
            );
        }
        _ => persistent(),
    }
}
