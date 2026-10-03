//! Text transformations offered for a selection. Pure functions: each returns
//! the new text, or `None` when it does not apply (the text is not valid
//! Base64 or JSON, or the result would be the same text).

use crate::web_search::percent_encode;

/// One applicable transformation of a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transform {
    /// Stable key for the action's id (`upper`, `base64_decode`, ...).
    pub key: &'static str,
    /// Shown in the action panel.
    pub label: &'static str,
    pub result: String,
}

/// Every transformation that applies to `text` and changes it, in the order
/// they are listed.
pub fn applicable(text: &str) -> Vec<Transform> {
    let candidates: [(&str, &str, Option<String>); 10] = [
        ("upper", "Uppercase", Some(text.to_uppercase())),
        ("lower", "Lowercase", Some(text.to_lowercase())),
        ("title", "Title Case", Some(title_case(text))),
        ("trim", "Trim whitespace", Some(text.trim().to_owned())),
        ("url_encode", "URL-encode", Some(percent_encode(text))),
        ("url_decode", "URL-decode", url_decode(text)),
        (
            "base64_encode",
            "Base64 encode",
            Some(base64_encode(text.as_bytes())),
        ),
        ("base64_decode", "Base64 decode", base64_decode(text)),
        ("json_pretty", "Pretty-print JSON", json_pretty(text)),
        ("json_minify", "Minify JSON", json_minify(text)),
    ];
    candidates
        .into_iter()
        .filter_map(|(key, label, result)| {
            let result = result?;
            (result != text && !result.is_empty()).then_some(Transform { key, label, result })
        })
        .collect()
}

/// First letter of every word upper case, the rest lower case. A word is a run
/// of letters and digits; an apostrophe inside one does not start a new word
/// (`don't` becomes `Don't`).
pub fn title_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_word = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if in_word {
                out.extend(ch.to_lowercase());
            } else {
                out.extend(ch.to_uppercase());
            }
            in_word = true;
        } else {
            // An apostrophe between letters keeps the word going.
            in_word = in_word && matches!(ch, '\'' | '\u{2019}');
            out.push(ch);
        }
    }
    out
}

/// Percent-decodes `%XX` escapes (a `+` stays a plus). `None` if there is no
/// escape, or the bytes are not UTF-8.
pub fn url_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut escapes = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                escapes += 1;
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    if escapes == 0 {
        return None;
    }
    String::from_utf8(out).ok()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard Base64 with padding.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(BASE64[(n >> 18) as usize & 63] as char);
        out.push(BASE64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            BASE64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn base64_value(byte: u8) -> Option<u32> {
    Some(match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' | b'-' => 62,
        b'/' | b'_' => 63,
        _ => return None,
    } as u32)
}

/// Decodes standard or URL-safe Base64 (padding optional, whitespace ignored)
/// into text. `None` unless the result is printable UTF-8: decoding an
/// ordinary word as Base64 must not produce a suggestion.
pub fn base64_decode(text: &str) -> Option<String> {
    let mut data: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let padding = data.iter().rev().take_while(|b| **b == b'=').count();
    if padding > 2 {
        return None;
    }
    data.truncate(data.len() - padding);
    // Shorter than one group is too little to call Base64; a remainder of one
    // character cannot come from any input.
    if data.len() < 4 || data.len() % 4 == 1 {
        return None;
    }
    let mut bytes = Vec::with_capacity(data.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in data {
        buffer = (buffer << 6) | base64_value(byte)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    let decoded = String::from_utf8(bytes).ok()?;
    let printable = decoded
        .chars()
        .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'));
    (printable && !decoded.trim().is_empty()).then_some(decoded)
}

/// JSON with one member per line, indented by two spaces, keys in their
/// original order. `None` unless `text` is a JSON object or array.
pub fn json_pretty(text: &str) -> Option<String> {
    reformat_json(text, true)
}

/// JSON without any whitespace between tokens.
pub fn json_minify(text: &str) -> Option<String> {
    reformat_json(text, false)
}

/// Re-lays out JSON text without parsing it into a map, which would sort or
/// drop keys: validity is checked first, then only whitespace changes.
fn reformat_json(text: &str, pretty: bool) -> Option<String> {
    let trimmed = text.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    serde_json::from_str::<serde::de::IgnoredAny>(trimmed).ok()?;

    let mut out = String::with_capacity(trimmed.len() * 2);
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = trimmed.chars().peekable();
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        for _ in 0..depth {
            out.push_str("  ");
        }
    };
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
            }
            '{' | '[' => {
                out.push(ch);
                // Empty containers stay on one line.
                let mut ahead = chars.clone().skip_while(|c| c.is_whitespace());
                if matches!(ahead.next(), Some('}' | ']')) {
                    while chars.next_if(|c| c.is_whitespace()).is_some() {}
                    out.extend(chars.next());
                } else if pretty {
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            '}' | ']' => {
                if pretty {
                    depth = depth.saturating_sub(1);
                    newline(&mut out, depth);
                }
                out.push(ch);
            }
            ',' => {
                out.push(ch);
                if pretty {
                    newline(&mut out, depth);
                }
            }
            ':' => {
                out.push(ch);
                if pretty {
                    out.push(' ');
                }
            }
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str) -> Vec<&'static str> {
        applicable(text).into_iter().map(|t| t.key).collect()
    }

    fn result(text: &str, key: &str) -> Option<String> {
        applicable(text)
            .into_iter()
            .find(|t| t.key == key)
            .map(|t| t.result)
    }

    #[test]
    fn case_transforms() {
        assert_eq!(
            result("Hello World", "upper").as_deref(),
            Some("HELLO WORLD")
        );
        assert_eq!(
            result("Hello World", "lower").as_deref(),
            Some("hello world")
        );
        assert_eq!(
            title_case("hello wORLD-foo don't STOP"),
            "Hello World-Foo Don't Stop"
        );
        assert_eq!(title_case("it\u{2019}s 3RD place"), "It\u{2019}s 3rd Place");
        assert_eq!(title_case("  "), "  ");
        assert_eq!(result("stra\u{df}e", "upper").as_deref(), Some("STRASSE"));
    }

    #[test]
    fn transforms_that_change_nothing_are_not_offered() {
        assert!(!keys("HELLO").contains(&"upper"));
        assert!(!keys("hello").contains(&"lower"));
        assert!(!keys("Hello").contains(&"title"));
        assert!(!keys("hello").contains(&"trim"));
        assert!(keys("  hello ").contains(&"trim"));
        assert_eq!(result("  hello \n", "trim").as_deref(), Some("hello"));
        // Blank results are never offered.
        assert!(!keys("   ").contains(&"trim"));
    }

    #[test]
    fn url_encoding_and_decoding() {
        assert_eq!(
            result("a b&c=d/é", "url_encode").as_deref(),
            Some("a%20b%26c%3Dd%2F%C3%A9")
        );
        assert!(!keys("plain-text_1.0~").contains(&"url_encode"));
        assert_eq!(
            result("a%20b%26c%3Dd%2F%C3%A9", "url_decode").as_deref(),
            Some("a b&c=d/é")
        );
        // A plus is left alone; lowercase hex works; a stray percent is kept.
        assert_eq!(url_decode("a+b%2f100%").as_deref(), Some("a+b/100%"));
        assert_eq!(url_decode("100%"), None);
        assert_eq!(url_decode("no escapes"), None);
        // Bytes that are not UTF-8 are not text.
        assert_eq!(url_decode("%FF%FE"), None);
        assert_eq!(url_decode("%41%"), Some("A%".to_owned()));
        assert_eq!(url_decode("%4"), None);
    }

    #[test]
    fn base64_round_trips() {
        for sample in [
            "foo",
            "foob",
            "fooba",
            "foobar",
            "h\u{e9}llo \u{2713}",
            "a longer line of text",
        ] {
            assert_eq!(
                base64_decode(&base64_encode(sample.as_bytes())).as_deref(),
                Some(sample),
                "{sample:?}"
            );
        }
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b""), "");
        // Less than a group is not worth suggesting.
        assert_eq!(base64_decode("Zg=="), None);
    }

    #[test]
    fn base64_decoding_is_forgiving_about_form_and_strict_about_content() {
        assert_eq!(base64_decode("Zm9vYmFy").as_deref(), Some("foobar"));
        assert_eq!(base64_decode("Zm9vYg==").as_deref(), Some("foob"));
        assert_eq!(base64_decode("Zm9vYg").as_deref(), Some("foob"));
        assert_eq!(base64_decode("Zm9v\nYmFy\n").as_deref(), Some("foobar"));
        // URL-safe alphabet.
        assert_eq!(base64_decode("4pyT").as_deref(), Some("\u{2713}"));
        assert_eq!(base64_decode("-_-_"), None); // not UTF-8
                                                 // Not Base64 at all, or not text.
        assert_eq!(base64_decode("hello world"), None);
        assert_eq!(base64_decode("abc"), None);
        assert_eq!(base64_decode("Zm9vY==="), None);
        assert_eq!(base64_decode("Zm9=vYmFy"), None);
        assert_eq!(base64_decode("AAAA"), None); // control characters
        assert_eq!(base64_decode("Test"), None); // an ordinary word
    }

    #[test]
    fn json_is_laid_out_without_touching_its_content() {
        let source = r#"{"b":1,"a":[1,2,{"x":"a,b:{c}\"d"}],"e":{},"f":[ ],"g":1.50}"#;
        let pretty = json_pretty(source).unwrap();
        assert_eq!(
            pretty,
            "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2,\n    {\n      \"x\": \"a,b:{c}\\\"d\"\n    }\n  ],\n  \"e\": {},\n  \"f\": [],\n  \"g\": 1.50\n}"
        );
        // Key order and number spelling survive, and it is still the same JSON.
        let a: serde_json::Value = serde_json::from_str(source).unwrap();
        let b: serde_json::Value = serde_json::from_str(&pretty).unwrap();
        assert_eq!(a, b);
        assert_eq!(
            json_minify(&pretty).as_deref(),
            json_minify(source).as_deref()
        );
        assert_eq!(
            json_minify(" [ 1 , { \"a b\" : \"x y\" } ] ").as_deref(),
            Some(r#"[1,{"a b":"x y"}]"#)
        );
    }

    #[test]
    fn only_json_objects_and_arrays_count() {
        for not_json in [
            "123", "\"str\"", "true", "{\"a\":}", "[1,2", "{a:1}", "hello", "",
        ] {
            assert_eq!(json_pretty(not_json), None, "{not_json:?}");
            assert_eq!(json_minify(not_json), None, "{not_json:?}");
        }
        assert_eq!(json_pretty("[]").as_deref(), Some("[]"));
        // Already laid out: nothing to offer.
        assert!(!keys("[1,2]").contains(&"json_minify"));
        assert!(keys("[1,2]").contains(&"json_pretty"));
        assert!(!keys("[]").contains(&"json_pretty"));
    }

    #[test]
    fn a_plain_word_gets_the_expected_menu() {
        assert_eq!(keys("hello"), ["upper", "title", "base64_encode"]);
    }
}
