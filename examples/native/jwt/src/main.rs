//! Decode a JSON Web Token offline.
//!
//! Type `jwt` and paste a token. Sevak lists whether it is expired, the
//! algorithm, the header and the claims (times shown as dates), and the
//! signature. Enter copies the row's value. The token never leaves the
//! computer, and **the signature is not verified**: this reads a token, it does
//! not vouch for it.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};
use sevak_extension_sdk::{run, Action, Icon, Item, Query};

/// The most claim rows shown; Sevak shows at most 50 rows in all.
const MAX_CLAIMS: usize = 30;
/// Claims with a meaning of their own (RFC 7519), listed first and explained.
const REGISTERED: [(&str, &str); 7] = [
    ("iss", "Issuer"),
    ("sub", "Subject"),
    ("aud", "Audience"),
    ("exp", "Expires"),
    ("nbf", "Not valid before"),
    ("iat", "Issued at"),
    ("jti", "Token id"),
];

fn main() {
    run(|query: &Query| Ok(answer(query.text(), now())));
}

/// Seconds since 1970-01-01 UTC; 0 if the clock is before it.
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The rows for what the user typed after the keyword, judged at time `now`.
fn answer(input: &str, now: i64) -> Vec<Item> {
    if input.trim().is_empty() {
        return usage();
    }
    let token = clean(input);
    let parts: Vec<&str> = token.split('.').collect();
    match parts.len() {
        3 => decode(&parts, now),
        5 => vec![problem(
            "This is an encrypted token (JWE, five parts)",
            "Its content is encrypted, so there is nothing to read without the key.",
        )],
        n => vec![problem(
            format!("Not a JWT: expected 3 parts separated by dots, found {n}"),
            "A JWT looks like header.payload.signature (each part is base64url text).",
        )],
    }
}

fn usage() -> Vec<Item> {
    vec![
        Item::new("Paste a JWT after the keyword")
            .key("hint")
            .subtitle("jwt eyJhbGciOi... shows the header, the claims and when it expires")
            .icon(Icon::builtin("plugin")),
        Item::new("Decoded here, offline")
            .key("hint-offline")
            .subtitle("The token is never sent anywhere. The signature is not verified."),
    ]
}

fn problem(title: impl Into<String>, subtitle: impl Into<String>) -> Item {
    Item::new(title)
        .key("problem")
        .subtitle(subtitle)
        .icon(Icon::builtin("warning"))
}

/// The token as typed or pasted: no `Bearer ` prefix, no quotes, no whitespace
/// (a token copied from a log is often wrapped).
fn clean(input: &str) -> String {
    let mut text = input.trim();
    if text
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer "))
    {
        text = &text[7..];
    }
    let text = text.trim().trim_matches(['"', '\'', '`']);
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn decode(parts: &[&str], now: i64) -> Vec<Item> {
    let header_bytes = match base64url_decode(parts[0]) {
        Ok(bytes) => bytes,
        Err(why) => {
            return vec![problem(
                format!("The header is not base64url: {why}"),
                parts[0],
            )]
        }
    };
    let Ok(Value::Object(header)) = serde_json::from_slice::<Value>(&header_bytes) else {
        return vec![problem(
            "The header is not a JSON object",
            "This does not look like a JWT.",
        )];
    };
    let payload_bytes = match base64url_decode(parts[1]) {
        Ok(bytes) => bytes,
        Err(why) => {
            return vec![problem(
                format!("The payload is not base64url: {why}"),
                parts[1],
            )]
        }
    };

    let mut rows = Vec::new();
    let claims = match serde_json::from_slice::<Value>(&payload_bytes) {
        Ok(Value::Object(claims)) => Some(claims),
        _ => None,
    };
    if let Some(claims) = &claims {
        rows.push(status_row(claims, now));
    }
    rows.push(algorithm_row(&header));
    rows.push(json_row("header", "Header", &Value::Object(header)));
    match &claims {
        Some(claims) => {
            rows.push(json_row(
                "payload",
                "Payload",
                &Value::Object(claims.clone()),
            ));
            rows.extend(claim_rows(claims, now));
        }
        None => {
            // A JWS payload need not be JSON; show what it is.
            let text = String::from_utf8_lossy(&payload_bytes).into_owned();
            rows.push(
                Item::new(format!("Payload (not a JSON object): {text}"))
                    .key("payload")
                    .subtitle("Enter copies it")
                    .action(Action::copy_text(text.clone()))
                    .text_view(text),
            );
        }
    }
    rows.push(signature_row(parts[2]));
    rows
}

/// Is it usable right now: not yet valid, expired, expiring, or no expiry.
fn status_row(claims: &Map<String, Value>, now: i64) -> Item {
    let exp = claim_time(claims, "exp");
    let nbf = claim_time(claims, "nbf");
    let note = "Judged by the claims only; the signature is not verified";
    match (nbf, exp) {
        (Some(nbf), _) if nbf > now => Item::new(format!(
            "Not valid yet: starts {}",
            relative(nbf - now, true)
        ))
        .key("status")
        .subtitle(format!("nbf {} · {note}", date(nbf)))
        .icon(Icon::builtin("warning"))
        .action(Action::copy_text(date(nbf))),
        (_, Some(exp)) if exp <= now => {
            Item::new(format!("Expired {}", relative(now - exp, false)))
                .key("status")
                .subtitle(format!("exp {} · {note}", date(exp)))
                .icon(Icon::builtin("warning"))
                .action(Action::copy_text(date(exp)))
        }
        (_, Some(exp)) => Item::new(format!("Expires {}", relative(exp - now, true)))
            .key("status")
            .subtitle(format!("exp {} · {note}", date(exp)))
            .action(Action::copy_text(date(exp))),
        (_, None) => Item::new("No expiry: the token has no exp claim")
            .key("status")
            .subtitle(note),
    }
}

fn algorithm_row(header: &Map<String, Value>) -> Item {
    let alg = header
        .get("alg")
        .and_then(Value::as_str)
        .unwrap_or("(none given)");
    let meaning = match alg {
        "HS256" => "HMAC with SHA-256 (shared secret)",
        "HS384" => "HMAC with SHA-384 (shared secret)",
        "HS512" => "HMAC with SHA-512 (shared secret)",
        "RS256" | "RS384" | "RS512" => "RSA PKCS#1 signature (public key)",
        "PS256" | "PS384" | "PS512" => "RSA-PSS signature (public key)",
        "ES256" | "ES384" | "ES512" => "ECDSA signature (public key)",
        "EdDSA" => "Edwards-curve signature (public key)",
        "none" => "unsigned: anyone could have written this token",
        _ => "not a standard algorithm name",
    };
    let mut details = vec![meaning.to_owned()];
    for name in ["typ", "kid"] {
        if let Some(value) = header.get(name).and_then(Value::as_str) {
            details.push(format!("{name} {value}"));
        }
    }
    let icon = if alg == "none" { "warning" } else { "plugin" };
    Item::new(format!("Algorithm {alg}"))
        .key("alg")
        .subtitle(details.join(" · "))
        .icon(Icon::builtin(icon))
        .action(Action::copy_text(alg))
}

/// A whole header or payload: compact in the row, pretty in the text view.
fn json_row(key: &str, label: &str, value: &Value) -> Item {
    let compact = serde_json::to_string(value).unwrap_or_default();
    let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
    Item::new(format!("{label}: {compact}"))
        .key(key)
        .subtitle("Enter copies it pretty-printed; Ctrl+T shows it in full")
        .action(Action::copy_text(pretty.clone()))
        .text_view(pretty)
}

/// One row per claim, registered claims first, then the others by name.
fn claim_rows(claims: &Map<String, Value>, now: i64) -> Vec<Item> {
    let mut names: Vec<&String> = claims.keys().collect();
    names.sort_by_key(|name| {
        let rank = REGISTERED
            .iter()
            .position(|(registered, _)| registered == name)
            .unwrap_or(REGISTERED.len());
        (rank, (*name).clone())
    });
    let mut rows = Vec::new();
    for name in names.iter().take(MAX_CLAIMS) {
        let value = &claims[*name];
        let shown = match value {
            Value::String(text) => text.clone(),
            other => serde_json::to_string(other).unwrap_or_default(),
        };
        let meaning = REGISTERED
            .iter()
            .find(|(registered, _)| registered == name)
            .map_or("Custom claim", |(_, meaning)| *meaning);
        let subtitle = match (
            matches!(name.as_str(), "exp" | "nbf" | "iat"),
            claim_time(claims, name),
        ) {
            (true, Some(time)) => {
                let delta = time - now;
                let when = if delta >= 0 {
                    relative(delta, true)
                } else {
                    relative(-delta, false)
                };
                format!("{meaning} · {} · {when}", date(time))
            }
            _ => meaning.to_owned(),
        };
        rows.push(
            Item::new(format!("{name}: {shown}"))
                .key(format!("claim-{name}"))
                .subtitle(subtitle)
                .action(Action::copy_text(shown)),
        );
    }
    if names.len() > MAX_CLAIMS {
        rows.push(
            Item::new(format!(
                "{} more claims are in the payload row",
                names.len() - MAX_CLAIMS
            ))
            .key("more-claims")
            .subtitle("Ctrl+T on the Payload row shows them all"),
        );
    }
    rows
}

fn signature_row(signature: &str) -> Item {
    if signature.is_empty() {
        return Item::new("No signature")
            .key("signature")
            .subtitle("An unsecured token: nothing proves who wrote it");
    }
    let length = base64url_decode(signature).map_or_else(
        |_| "not valid base64url".to_owned(),
        |bytes| format!("{} bytes", bytes.len()),
    );
    Item::new(format!("Signature: {length}, not verified"))
        .key("signature")
        .subtitle("Enter copies the signature as written in the token")
        .action(Action::copy_text(signature))
}

/// A numeric date claim (seconds since 1970) as whole seconds.
fn claim_time(claims: &Map<String, Value>, name: &str) -> Option<i64> {
    let number = claims.get(name)?.as_f64()?;
    // Years 0000 to 9999, which also rejects NaN-like and absurd values.
    (-62_167_219_200.0..=253_402_300_799.0)
        .contains(&number)
        .then(|| number.floor() as i64)
}

/// `2024-01-02 03:04:05 UTC`.
fn date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
/// (Howard Hinnant's `civil_from_days`.)
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// "in 2 hours" (`future`) or "2 hours ago".
fn relative(secs: i64, future: bool) -> String {
    let span = human(secs.unsigned_abs());
    if future {
        format!("in {span}")
    } else {
        format!("{span} ago")
    }
}

fn human(secs: u64) -> String {
    fn plural(n: u64, unit: &str) -> String {
        format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
    }
    let (minutes, hours, days) = (secs / 60, secs / 3600, secs / 86_400);
    match secs {
        0..=59 => plural(secs, "second"),
        60..=3599 => plural(minutes, "minute"),
        3600..=86_399 => match minutes % 60 {
            0 => plural(hours, "hour"),
            m => format!("{} {}", plural(hours, "hour"), plural(m, "minute")),
        },
        86_400..=5_183_999 => match hours % 24 {
            0 => plural(days, "day"),
            h => format!("{} {}", plural(days, "day"), plural(h, "hour")),
        },
        _ if days < 730 => plural(days, "day"),
        _ => plural(days / 365, "year"),
    }
}

/// Decodes base64url (RFC 4648 section 5), with or without padding. The
/// standard alphabet's `+` and `/` are accepted too, since tokens get mangled.
fn base64url_decode(text: &str) -> Result<Vec<u8>, String> {
    let body = text.trim_end_matches('=');
    if body.len() % 4 == 1 {
        return Err("the length is impossible".to_owned());
    }
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for c in body.chars() {
        let value = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '-' | '+' => 62,
            '_' | '/' => 63,
            other => return Err(format!("{other:?} is not allowed")),
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000; // 2023-11-14 22:13:20 UTC

    /// A test-only encoder, so tokens are built from readable JSON.
    fn base64url_encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0_u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
            for i in 0..=chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        out
    }

    fn token(header: &str, payload: &str, signature: &str) -> String {
        format!(
            "{}.{}.{signature}",
            base64url_encode(header.as_bytes()),
            base64url_encode(payload.as_bytes())
        )
    }

    fn dump(rows: &[Item]) -> String {
        rows.iter()
            .map(|row| format!("{row:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_well_known_example_token_decodes() {
        // The sample token from jwt.io.
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let text = dump(&answer(token, NOW));
        assert!(text.contains("Algorithm HS256"), "{text}");
        assert!(text.contains("HMAC with SHA-256"), "{text}");
        assert!(text.contains("name: John Doe"), "{text}");
        assert!(text.contains("sub: 1234567890"), "{text}");
        assert!(text.contains("2018-01-18 01:30:22 UTC"), "{text}");
        assert!(text.contains("No expiry"), "{text}");
        assert!(text.contains("Signature: 32 bytes, not verified"), "{text}");
    }

    #[test]
    fn expiry_is_judged_against_the_clock() {
        let make = |claims: &str| token(r#"{"alg":"HS256"}"#, claims, "c2ln");
        let expired = dump(&answer(&make(&format!(r#"{{"exp":{}}}"#, NOW - 7200)), NOW));
        assert!(expired.contains("Expired 2 hours ago"), "{expired}");
        let live = dump(&answer(&make(&format!(r#"{{"exp":{}}}"#, NOW + 90)), NOW));
        assert!(live.contains("Expires in 1 minute"), "{live}");
        let early = dump(&answer(
            &make(&format!(r#"{{"nbf":{}}}"#, NOW + 3 * 86_400)),
            NOW,
        ));
        assert!(early.contains("Not valid yet: starts in 3 days"), "{early}");
    }

    #[test]
    fn registered_claims_come_first_and_times_are_dates() {
        let rows = answer(
            &token(
                r#"{"alg":"ES256","kid":"k1"}"#,
                r#"{"zeta":true,"iss":"me","exp":1700003600}"#,
                "AA",
            ),
            NOW,
        );
        let text = dump(&rows);
        assert!(text.contains("kid k1"), "{text}");
        let iss = text.find("iss: me").unwrap();
        let exp = text.find("exp: 1700003600").unwrap();
        let zeta = text.find("zeta: true").unwrap();
        assert!(iss < exp && exp < zeta, "{text}");
        assert!(text.contains("2023-11-14 23:13:20 UTC"), "{text}");
    }

    #[test]
    fn alg_none_and_a_missing_signature_are_called_out() {
        let text = dump(&answer(&token(r#"{"alg":"none"}"#, r#"{"a":1}"#, ""), NOW));
        assert!(text.contains("unsigned"), "{text}");
        assert!(text.contains("No signature"), "{text}");
    }

    #[test]
    fn a_bearer_prefix_quotes_and_line_breaks_are_ignored() {
        let plain = token(r#"{"alg":"HS256"}"#, r#"{"a":1}"#, "c2ln");
        let (a, b) = plain.split_at(10);
        let messy = format!("Bearer \"{a}\n  {b}\"");
        assert_eq!(dump(&answer(&messy, NOW)), dump(&answer(&plain, NOW)));
    }

    #[test]
    fn bad_input_gets_a_message_not_a_failure() {
        assert!(dump(&answer("hello", NOW)).contains("found 1"));
        assert!(dump(&answer("a.b", NOW)).contains("found 2"));
        assert!(dump(&answer("a.b.c.d.e", NOW)).contains("encrypted"));
        assert!(dump(&answer("!!!.b.c", NOW)).contains("header is not base64url"));
        let not_json = format!("{}.e30.c2ln", base64url_encode(b"not json"));
        assert!(dump(&answer(&not_json, NOW)).contains("header is not a JSON object"));
        let bad_payload = format!("{}.@@.c2ln", base64url_encode(br#"{"alg":"x"}"#));
        assert!(dump(&answer(&bad_payload, NOW)).contains("payload is not base64url"));
        let text_payload = token(r#"{"alg":"x"}"#, "plain text", "c2ln");
        assert!(dump(&answer(&text_payload, NOW)).contains("not a JSON object"));
    }

    #[test]
    fn an_empty_query_shows_usage() {
        let rows = answer("  ", NOW);
        assert!(dump(&rows).contains("Paste a JWT"));
        assert!(rows.iter().all(|row| row.problems().is_empty()));
    }

    #[test]
    fn every_row_is_within_what_sevak_accepts() {
        let claims: Vec<String> = (0..60)
            .map(|i| format!("\"c{i:02}\":\"{}\"", "x".repeat(400)))
            .collect();
        let rows = answer(
            &token(
                r#"{"alg":"HS256"}"#,
                &format!("{{{}}}", claims.join(",")),
                "c2ln",
            ),
            NOW,
        );
        assert!(rows.len() <= 50, "{} rows", rows.len());
        assert!(dump(&rows).contains("more claims"));
    }

    #[test]
    fn dates_and_durations() {
        assert_eq!(date(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(date(951_782_400), "2000-02-29 00:00:00 UTC");
        assert_eq!(date(-1), "1969-12-31 23:59:59 UTC");
        assert_eq!(human(1), "1 second");
        assert_eq!(human(3 * 3600 + 120), "3 hours 2 minutes");
        assert_eq!(human(40 * 86_400), "40 days");
        assert_eq!(human(1000 * 86_400), "2 years");
    }

    #[test]
    fn base64url_handles_padding_and_both_alphabets() {
        assert_eq!(base64url_decode("aGk").unwrap(), b"hi");
        assert_eq!(base64url_decode("aGk=").unwrap(), b"hi");
        assert_eq!(
            base64url_decode("-_-_").unwrap(),
            base64url_decode("+/+/").unwrap()
        );
        assert!(base64url_decode("a").is_err());
        assert!(base64url_decode("a b").is_err());
        assert_eq!(base64url_decode("").unwrap(), b"");
    }
}
