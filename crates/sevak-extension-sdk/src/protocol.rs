//! The wire messages (protocol version 1), as the extension sees them.

use serde::Deserialize;
use serde_json::{json, Value};

/// The protocol version this SDK speaks. It is `protocol = 1` in the manifest.
pub const PROTOCOL_VERSION: u32 = 1;

/// A line longer than this makes Sevak stop the extension, so the SDK never
/// writes one.
pub(crate) const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Sevak's first message.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Initialize {
    /// The protocol version Sevak speaks.
    #[serde(default)]
    pub protocol: u32,
    /// The version of Sevak that started the extension.
    #[serde(default)]
    pub sevak_version: String,
    /// The extension's id.
    #[serde(default)]
    pub plugin_id: String,
}

/// The user picked a row whose action is [`Action::Custom`](crate::Action::Custom).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Execute {
    /// The row's key.
    #[serde(default)]
    pub key: String,
    /// The action's payload.
    #[serde(default)]
    pub payload: String,
}

/// Sevak to the extension. Unknown message types and fields are ignored, so
/// protocol 1 can grow without breaking an extension built today.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Incoming {
    Initialize(Initialize),
    Query {
        request_id: u64,
        #[serde(default)]
        input: String,
    },
    Execute(Execute),
    Shutdown {},
    #[serde(other)]
    Unknown,
}

/// Parses one line from Sevak; `None` for anything that is not a JSON object
/// with a `type`.
pub(crate) fn parse_line(line: &str) -> Option<Incoming> {
    serde_json::from_str(line.trim()).ok()
}

pub(crate) fn ready_line() -> String {
    json!({"type": "ready"}).to_string()
}

pub(crate) fn error_line(request_id: Option<u64>, message: &str) -> String {
    json!({"type": "error", "request_id": request_id, "message": message}).to_string()
}

/// A `results` line for `request_id`. The items are dropped from the end until
/// the line fits the host's limit.
pub(crate) fn results_line(request_id: u64, mut items: Vec<Value>) -> String {
    loop {
        let line = json!({"type": "results", "request_id": request_id, "items": items}).to_string();
        if line.len() <= MAX_LINE_BYTES || items.is_empty() {
            return line;
        }
        items.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_sevak_sends() {
        assert_eq!(
            parse_line(
                r#"{"type":"initialize","protocol":1,"sevak_version":"0.1.0","plugin_id":"script:x"}"#
            ),
            Some(Incoming::Initialize(Initialize {
                protocol: 1,
                sevak_version: "0.1.0".into(),
                plugin_id: "script:x".into()
            }))
        );
        assert_eq!(
            parse_line(r#"{"type":"query","request_id":42,"input":"woréld"}"#),
            Some(Incoming::Query {
                request_id: 42,
                input: "wor\u{e9}ld".into()
            })
        );
        assert_eq!(
            parse_line(r#" {"type":"execute","key":"k","payload":"p"} "#),
            Some(Incoming::Execute(Execute {
                key: "k".into(),
                payload: "p".into()
            }))
        );
        assert_eq!(
            parse_line(r#"{"type":"shutdown"}"#),
            Some(Incoming::Shutdown {})
        );
    }

    #[test]
    fn unknown_types_and_fields_are_tolerated() {
        assert_eq!(
            parse_line(r#"{"type":"from_the_future","x":1}"#),
            Some(Incoming::Unknown)
        );
        assert_eq!(
            parse_line(r#"{"type":"query","request_id":1,"input":"a","future":true}"#),
            Some(Incoming::Query {
                request_id: 1,
                input: "a".into()
            })
        );
    }

    #[test]
    fn garbage_is_not_a_message() {
        for line in [
            "",
            "hello",
            "[1,2]",
            "{}",
            r#"{"request_id":1}"#,
            r#"{"type":"query"}"#,
        ] {
            assert_eq!(parse_line(line), None, "{line:?}");
        }
    }

    #[test]
    fn replies_have_the_documented_shape() {
        assert_eq!(ready_line(), r#"{"type":"ready"}"#);
        assert_eq!(
            serde_json::from_str::<Value>(&error_line(Some(3), "boom")).unwrap(),
            json!({"type": "error", "request_id": 3, "message": "boom"})
        );
        assert_eq!(
            serde_json::from_str::<Value>(&error_line(None, "boom")).unwrap()["request_id"],
            Value::Null
        );
        assert_eq!(
            serde_json::from_str::<Value>(&results_line(7, vec![json!({"title": "a"})])).unwrap(),
            json!({"type": "results", "request_id": 7, "items": [{"title": "a"}]})
        );
    }

    #[test]
    fn an_oversized_answer_loses_items_instead_of_the_extension() {
        let big = "x".repeat(400 * 1024);
        let items = vec![
            json!({"title": "1", "text": big.clone()}),
            json!({"title": "2", "text": big.clone()}),
            json!({"title": "3", "text": big}),
        ];
        let line = results_line(1, items);
        assert!(line.len() <= MAX_LINE_BYTES);
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["items"].as_array().unwrap().len(), 2);
    }
}
