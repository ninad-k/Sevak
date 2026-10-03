//! The persistent-process wire protocol (version 1): newline-delimited JSON,
//! one object per line, in both directions. `docs/plugins.md` is the reference;
//! these types are its Rust form.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A line longer than this is a protocol violation (the script is stopped).
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Sevak to script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToScript<'a> {
    Initialize {
        protocol: u32,
        sevak_version: &'a str,
        plugin_id: &'a str,
    },
    Query {
        request_id: u64,
        input: &'a str,
    },
    Execute {
        key: &'a str,
        payload: &'a str,
    },
    Shutdown,
}

impl ToScript<'_> {
    /// The message as one line, without the trailing newline.
    ///
    /// Everything outside ASCII is written as a `\uXXXX` escape. Any JSON
    /// parser decodes that to the same text, and it spares scripts the classic
    /// trap of a pipe read with the wrong encoding (Windows consoles default to
    /// a legacy code page, so raw UTF-8 would arrive as mojibake).
    pub fn to_line(&self) -> String {
        // Serializing these plain types cannot fail; an empty line is ignored
        // by scripts at worst.
        escape_non_ascii(&serde_json::to_string(self).unwrap_or_default())
    }
}

/// Replaces every non-ASCII character of serialized JSON with `\uXXXX` (a
/// surrogate pair above the Basic Multilingual Plane). Non-ASCII characters can
/// only occur inside strings, where the escape is valid.
fn escape_non_ascii(json: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(json.len());
    for ch in json.chars() {
        if ch.is_ascii() {
            out.push(ch);
        } else {
            let mut units = [0u16; 2];
            for unit in ch.encode_utf16(&mut units) {
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
    }
    out
}

/// Script to Sevak. Unknown message types are tolerated (`Unknown`), and so are
/// unknown fields, so protocol 1 can grow without breaking old scripts or Sevaks.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromScript {
    Ready {},
    Results {
        request_id: u64,
        #[serde(default)]
        items: Vec<Value>,
    },
    Error {
        request_id: Option<u64>,
        #[serde(default)]
        message: String,
    },
    #[serde(other)]
    Unknown,
}

/// Parses one line of script output; `None` for anything that is not a JSON
/// object with a `type`.
pub fn parse_line(line: &str) -> Option<FromScript> {
    serde_json::from_str(line.trim()).ok()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn serializes_messages_as_tagged_objects() {
        let line = ToScript::Query {
            request_id: 42,
            input: "wor\"ld",
        }
        .to_line();
        assert_eq!(
            line,
            r#"{"type":"query","request_id":42,"input":"wor\"ld"}"#
        );
        assert!(!line.contains('\n'));
        assert_eq!(ToScript::Shutdown.to_line(), r#"{"type":"shutdown"}"#);
        let init = ToScript::Initialize {
            protocol: 1,
            sevak_version: "0.1.0",
            plugin_id: "script:hello",
        }
        .to_line();
        assert_eq!(
            serde_json::from_str::<Value>(&init).unwrap(),
            json!({"type": "initialize", "protocol": 1, "sevak_version": "0.1.0", "plugin_id": "script:hello"})
        );
        let execute = ToScript::Execute {
            key: "k",
            payload: "p",
        }
        .to_line();
        assert_eq!(execute, r#"{"type":"execute","key":"k","payload":"p"}"#);
    }

    #[test]
    fn non_ascii_text_is_escaped_and_decodes_back() {
        for input in [
            "caf\u{e9}",
            "\u{65e5}\u{672c}\u{8a9e}",
            "emoji \u{1f600}",
            "mixed \"q\" \\ \u{e9}",
        ] {
            let line = ToScript::Query {
                request_id: 1,
                input,
            }
            .to_line();
            assert!(line.is_ascii(), "{line}");
            let back: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(back["input"], input);
        }
        let line = ToScript::Query {
            request_id: 1,
            input: "caf\u{e9}",
        }
        .to_line();
        assert!(
            line.contains(&format!("caf{}u00e9", char::from(92u8))),
            "{line}"
        );
    }

    #[test]
    fn newlines_in_input_stay_on_one_line() {
        let line = ToScript::Query {
            request_id: 1,
            input: "a\nb",
        }
        .to_line();
        assert!(!line.contains('\n'));
    }

    #[test]
    fn parses_script_messages() {
        assert_eq!(
            parse_line(r#"{"type":"ready"}"#),
            Some(FromScript::Ready {})
        );
        assert_eq!(
            parse_line(r#"{"type":"ready","protocol":1,"future":true}"#),
            Some(FromScript::Ready {})
        );
        assert_eq!(
            parse_line(r#" {"type":"results","request_id":7,"items":[{"title":"x"}]} "#),
            Some(FromScript::Results {
                request_id: 7,
                items: vec![json!({"title": "x"})]
            })
        );
        assert_eq!(
            parse_line(r#"{"type":"results","request_id":7}"#),
            Some(FromScript::Results {
                request_id: 7,
                items: Vec::new()
            })
        );
        assert_eq!(
            parse_line(r#"{"type":"error","request_id":3,"message":"boom"}"#),
            Some(FromScript::Error {
                request_id: Some(3),
                message: "boom".into()
            })
        );
        assert_eq!(
            parse_line(r#"{"type":"error","message":"boom"}"#),
            Some(FromScript::Error {
                request_id: None,
                message: "boom".into()
            })
        );
        assert_eq!(
            parse_line(r#"{"type":"from_the_future","x":1}"#),
            Some(FromScript::Unknown)
        );
    }

    #[test]
    fn garbage_is_not_a_message() {
        for line in [
            "",
            "hello",
            "[1,2]",
            "{}",
            r#"{"type":"results"}"#,
            r#"{"request_id":1}"#,
        ] {
            assert_eq!(parse_line(line), None, "{line:?}");
        }
    }
}
