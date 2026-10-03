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

fn main() {
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
