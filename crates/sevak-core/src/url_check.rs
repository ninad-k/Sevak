//! Checking an address before it is handed to the operating system.
//!
//! [`check_open_url`] is the gate behind `open_url`: addresses come from search
//! results, user commands, workflows and scripts, and the system's handler
//! (ShellExecute, `open`, `xdg-open`) receives them as one command-line
//! argument. The closed set of schemes stays `http`, `https` and `mailto`; on
//! top of that the address is parsed (not just prefix-matched), and anything
//! that could confuse a handler's own argument parsing is refused.

use url::Url;

/// The longest address that is opened.
pub const MAX_URL_BYTES: usize = 8192;
const MAX_MAILTO_ADDRESSES: usize = 50;
const MAX_ADDRESS_BYTES: usize = 254;

/// Validates `raw` and returns the normalised address to open (spaces and other
/// unsafe characters percent-encoded, host in its ASCII form), or why it was
/// refused.
///
/// Refused: anything but `http:`, `https:` and `mailto:`; control characters;
/// `"`, `<`, `>` and `\`; more than [`MAX_URL_BYTES`]; an `http(s)` address
/// without a host or with a user name or password in it; a `mailto:` whose
/// recipients are not plain addresses or whose options go beyond the
/// recipients, `subject` and `body`.
pub fn check_open_url(raw: &str) -> Result<String, &'static str> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("the URL is empty");
    }
    if raw.len() > MAX_URL_BYTES {
        return Err("the URL is too long");
    }
    if raw.chars().any(char::is_control) {
        return Err("the URL contains a control character");
    }
    if raw.contains(['"', '<', '>', '\\']) {
        return Err("the URL contains a character that is not allowed");
    }
    if raw.to_ascii_lowercase().contains("%00") {
        return Err("the URL contains an encoded NUL");
    }
    // `https:example.com` parses leniently; only the written-out forms count.
    let lower = raw.to_ascii_lowercase();
    if !["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return Err("only http://, https:// and mailto: links can be opened");
    }
    // `https:///path` also parses (the path becomes the host); it is not a link.
    if lower
        .split_once("://")
        .is_some_and(|(_, rest)| rest.starts_with('/'))
    {
        return Err("the URL has no host");
    }
    let url = Url::parse(raw).map_err(|_| "the URL is not valid")?;
    match url.scheme() {
        "http" | "https" => check_web(&url)?,
        "mailto" => check_mailto(&url)?,
        _ => return Err("only http://, https:// and mailto: links can be opened"),
    }
    let normalised = url.as_str();
    if normalised.len() > MAX_URL_BYTES {
        return Err("the URL is too long");
    }
    Ok(normalised.to_owned())
}

fn check_web(url: &Url) -> Result<(), &'static str> {
    if url.host_str().is_none_or(str::is_empty) {
        return Err("the URL has no host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("the URL contains a user name or password");
    }
    Ok(())
}

fn check_mailto(url: &Url) -> Result<(), &'static str> {
    if url.fragment().is_some() {
        return Err("the mailto: link is not valid");
    }
    let mut recipients = 0usize;
    let path = percent_decode(url.path())?;
    if path.is_empty() && url.query().is_none() {
        return Err("the mailto: link has no recipient");
    }
    if !path.is_empty() {
        recipients += check_address_list(&path)?;
    }
    if let Some(query) = url.query() {
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = percent_decode(key)?.to_ascii_lowercase();
            match key.as_str() {
                "subject" => {
                    if percent_decode(value)?.chars().any(char::is_control) {
                        return Err("the mailto: link has a control character");
                    }
                }
                "body" => {
                    let body = percent_decode(value)?;
                    if body
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                    {
                        return Err("the mailto: link has a control character");
                    }
                }
                "to" | "cc" | "bcc" => recipients += check_address_list(&percent_decode(value)?)?,
                _ => return Err("the mailto: link uses an option that is not allowed"),
            }
        }
    }
    if recipients > MAX_MAILTO_ADDRESSES {
        return Err("the mailto: link has too many recipients");
    }
    Ok(())
}

/// A comma separated list of plain `local@domain` addresses; returns how many.
fn check_address_list(list: &str) -> Result<usize, &'static str> {
    let mut count = 0;
    for address in list.split(',') {
        if !is_plain_address(address.trim()) {
            return Err("the mailto: link has an address that is not valid");
        }
        count += 1;
    }
    Ok(count)
}

fn is_plain_address(address: &str) -> bool {
    let Some((local, domain)) = address.rsplit_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local.chars().all(|c| {
            c.is_ascii_alphanumeric() || "!#$%&'*+/=?^_`{|}~.-".contains(c) || !c.is_ascii()
        });
    let domain_ok = !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || !c.is_ascii())
        });
    address.len() <= MAX_ADDRESS_BYTES && local_ok && domain_ok
}

/// Decodes `%XX` escapes (`+` stays a plus, as in addresses). Refuses control
/// characters in the result and bytes that are not UTF-8.
fn percent_decode(text: &str) -> Result<String, &'static str> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3).ok_or("the URL is not valid")?;
            let hex = std::str::from_utf8(hex).map_err(|_| "the URL is not valid")?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| "the URL is not valid")?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "the URL is not valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(raw: &str) -> String {
        check_open_url(raw).unwrap_or_else(|why| panic!("{raw:?} refused: {why}"))
    }

    fn refused(raw: &str) {
        assert!(check_open_url(raw).is_err(), "{raw:?} should be refused");
    }

    #[test]
    fn ordinary_links_pass_and_are_normalised() {
        assert_eq!(ok("https://example.com"), "https://example.com/");
        assert_eq!(
            ok("  HTTP://Example.COM/a?b=1#c  "),
            "http://example.com/a?b=1#c"
        );
        assert_eq!(
            ok("https://example.com/search?q=hello world"),
            "https://example.com/search?q=hello%20world"
        );
        assert_eq!(
            ok("https://example.com:8443/x"),
            "https://example.com:8443/x"
        );
        assert_eq!(ok("https://[::1]/x"), "https://[::1]/x");
        assert_eq!(
            ok("https://bücher.example/"),
            "https://xn--bcher-kva.example/"
        );
        assert_eq!(
            ok("https://example.com/%22quoted%22"),
            "https://example.com/%22quoted%22"
        );
    }

    #[test]
    fn other_schemes_are_refused() {
        for raw in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ms-msdt:/id PCWDiagnostic",
            "ftp://example.com/",
            "data:text/html,hi",
            "tel:5550100",
            "example.com",
            "//example.com/x",
            "https:example.com",
            "",
            "   ",
        ] {
            refused(raw);
        }
    }

    #[test]
    fn characters_that_confuse_handlers_are_refused() {
        for raw in [
            "https://example.com/\" --evil",
            "https://example.com/a\"b",
            "https://example.com/<script>",
            "https://example.com/a>b",
            "https://example.com\\evil",
            "https://example.com/a\\b",
            "https://example.com/a\nb",
            "https://example.com/a\rb",
            "https://example.com/a\tb",
            "https://example.com/a\0b",
            "https://example.com/a\u{7f}b",
            "https://example.com/a\u{85}b",
            "https://example.com/%00",
        ] {
            refused(raw);
        }
    }

    #[test]
    fn web_links_need_a_host_and_no_credentials() {
        for raw in [
            "http://",
            "https://",
            "https:///path",
            "https://user@example.com/",
            "https://user:pw@example.com/",
            "https://exa mple.com/",
            "https://exa<mple.com/",
            "http://[::1/",
            "http://example.com:99999/",
        ] {
            refused(raw);
        }
    }

    #[test]
    fn long_links_are_refused() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES));
        refused(&long);
        let just = format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES - 20));
        assert_eq!(just.len(), MAX_URL_BYTES);
        ok(&just);
        // Percent-encoding spaces can make an address longer than it was typed.
        let spaces = format!("https://example.com/{}", "a b".repeat(MAX_URL_BYTES / 4));
        refused(&spaces);
    }

    #[test]
    fn mailto_takes_an_address_list_and_a_few_options() {
        assert_eq!(ok("mailto:a@example.com"), "mailto:a@example.com");
        ok("mailto:a@example.com,b@example.org");
        ok("mailto:a+tag@example.com?subject=Hello%20there&body=Line1%0ALine2");
        ok("mailto:?to=a@example.com&cc=b@example.org");
        ok("mailto:a@example.com?subject=Hi&BCC=c@example.com");
        ok("MAILTO:a@example.com");
        ok("mailto:a%40example.com");
    }

    #[test]
    fn mailto_refuses_anything_else() {
        for raw in [
            "mailto:",
            "mailto:not-an-address",
            "mailto:a@",
            "mailto:@example.com",
            "mailto:a@example.com,",
            "mailto:a@example.com;b@example.com",
            "mailto:a@example.com?attach=C:/secret.txt",
            "mailto:a@example.com?attachment=x",
            "mailto:a@example.com?subject=x&attach=y",
            "mailto:a@example.com?cc=nope",
            "mailto:a b@example.com",
            "mailto:a@exa mple.com",
            "mailto:a@example.com#frag",
            "mailto:a..b@example.com",
            "mailto:a@-example.com",
            "mailto:a@example.com?subject=%ZZ",
            "mailto:a@example.com?subject=x%0D%0ABcc:%20evil@example.com",
            "mailto:a@example.com?body=%07",
            "mailto:a@example.com\"",
        ] {
            refused(raw);
        }
        let many = (0..=MAX_MAILTO_ADDRESSES)
            .map(|i| format!("u{i}@example.com"))
            .collect::<Vec<_>>()
            .join(",");
        refused(&format!("mailto:{many}"));
    }
}
