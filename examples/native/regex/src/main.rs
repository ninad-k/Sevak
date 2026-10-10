//! Test a regular expression against sample text.
//!
//! Type `regex`, the pattern, `=>` and the text to try it on
//! (`regex (\d+)-(\d+) => call 555-1234 now`). Sevak lists every match with
//! where it starts and what each capture group caught. Enter copies a match.
//!
//! The syntax is the Rust `regex` crate's: no lookaround and no
//! backreferences, in exchange for matching in time linear in the text, so no
//! pattern can hang the launcher.

use regex::{Regex, RegexBuilder};
use sevak_extension_sdk::{run, Action, Icon, Item, Query};

/// Match rows listed; the summary row still counts them all (up to `COUNT_CAP`).
const MAX_ROWS: usize = 20;
/// Stop counting here: a pattern like `` (empty) matches between every letter.
const COUNT_CAP: usize = 10_000;
/// The largest compiled pattern accepted, in bytes (the crate's default is 10 MiB).
const SIZE_LIMIT: usize = 2 * 1024 * 1024;
/// What separates the pattern from the sample text.
const SEPARATOR: &str = "=>";

fn main() {
    run(|query: &Query| Ok(answer(query.raw())));
}

fn answer(input: &str) -> Vec<Item> {
    let input = input.trim_start();
    if input.trim().is_empty() {
        return usage();
    }
    let (pattern, sample) = match input.split_once(SEPARATOR) {
        // One space after `=>` is the separator's, the rest is the sample.
        Some((pattern, sample)) => (
            pattern.trim_end(),
            Some(sample.strip_prefix(' ').unwrap_or(sample)),
        ),
        None => (input.trim_end(), None),
    };
    let regex = match RegexBuilder::new(pattern).size_limit(SIZE_LIMIT).build() {
        Ok(regex) => regex,
        Err(why) => return vec![invalid(&why.to_string())],
    };
    match sample {
        None => vec![valid(&regex)],
        Some(sample) => matches(&regex, sample),
    }
}

fn usage() -> Vec<Item> {
    vec![
        Item::new("Type a pattern, then => and the text to test")
            .key("hint")
            .subtitle(r"For example: regex (\d+)-(\d+) => call 555-1234 now")
            .icon(Icon::builtin("plugin")),
        Item::new("Rust regex syntax")
            .key("hint-syntax")
            .subtitle(r"(?i) ignores case, \b is a word edge, (?P<name>...) names a group"),
        Item::new("No lookaround or backreferences")
            .key("hint-limits")
            .subtitle("In return a pattern always finishes quickly, whatever the text"),
    ]
}

fn invalid(message: &str) -> Item {
    // The crate's message shows the pattern with a caret; the useful line is
    // the one starting `error:`.
    let reason = message
        .lines()
        .find_map(|line| line.trim().strip_prefix("error: "))
        .unwrap_or_else(|| message.lines().next().unwrap_or("it did not compile"));
    Item::new(format!("Invalid pattern: {reason}"))
        .key("invalid")
        .subtitle("Lookaround and backreferences are not supported; escape literal ( [ { with \\")
        .icon(Icon::builtin("warning"))
}

/// The pattern alone: it compiles; say what it captures.
fn valid(regex: &Regex) -> Item {
    let groups = group_labels(regex);
    let summary = if groups.is_empty() {
        "no capture groups".to_owned()
    } else {
        format!("capture groups {}", groups.join(", "))
    };
    Item::new(format!("Valid pattern, {summary}"))
        .key("valid")
        .subtitle("Add => and some text to see the matches")
        .icon(Icon::builtin("plugin"))
}

/// `1`, `2`, `year` (a named group shows its name) for each group after the whole match.
fn group_labels(regex: &Regex) -> Vec<String> {
    regex
        .capture_names()
        .enumerate()
        .skip(1)
        .map(|(i, name)| name.map_or_else(|| i.to_string(), str::to_owned))
        .collect()
}

fn matches(regex: &Regex, sample: &str) -> Vec<Item> {
    let labels = group_labels(regex);
    let mut rows = Vec::new();
    let mut found = Vec::new();
    let mut total = 0;
    for captures in regex.captures_iter(sample) {
        total += 1;
        if rows.len() < MAX_ROWS {
            let whole = captures.get(0).expect("group 0 is the whole match");
            let start = sample[..whole.start()].chars().count();
            let end = start + whole.as_str().chars().count();
            let mut details = vec![format!("chars {start}-{end}")];
            for (i, label) in labels.iter().enumerate() {
                details.push(match captures.get(i + 1) {
                    Some(group) => format!("{label}: \"{}\"", visible(group.as_str())),
                    None => format!("{label}: (did not take part)"),
                });
            }
            let title = if whole.as_str().is_empty() {
                "(empty match)".to_owned()
            } else {
                format!("\"{}\"", visible(whole.as_str()))
            };
            rows.push(
                Item::new(title)
                    .key(format!("match-{total}"))
                    .subtitle(details.join(" · "))
                    .action(Action::copy_text(whole.as_str())),
            );
        }
        found.push(captures.get(0).map_or("", |m| m.as_str()).to_owned());
        if total == COUNT_CAP {
            break;
        }
    }

    let count = if total == COUNT_CAP {
        format!("{COUNT_CAP}+ matches")
    } else if total == 1 {
        "1 match".to_owned()
    } else {
        format!("{total} matches")
    };
    let summary = if total == 0 {
        Item::new("No match")
            .key("summary")
            .subtitle("The pattern is valid but nothing in the text matches it")
            .icon(Icon::builtin("warning"))
    } else {
        Item::new(count)
            .key("summary")
            .subtitle("Enter copies all matches, one per line")
            .icon(Icon::builtin("plugin"))
            .action(Action::copy_text(found.join("\n")))
    };
    rows.insert(0, summary);
    if total > MAX_ROWS {
        let more = total - MAX_ROWS;
        rows.push(
            Item::new(format!(
                "{more}{} more matches not listed",
                if total == COUNT_CAP { "+" } else { "" }
            ))
            .key("more")
            .subtitle("The first matches are above; Enter on the summary copies them all"),
        );
    }
    rows
}

/// Control characters shown as escapes, so a match never breaks the row.
fn visible(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(rows: &[Item]) -> String {
        rows.iter()
            .map(|row| format!("{row:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn matches_groups_and_positions_are_listed() {
        let rows = answer(r"(\d+)-(\d+) => call 555-1234 or 800-9999");
        let text = dump(&rows);
        assert_eq!(rows.len(), 3, "{text}");
        assert!(text.contains("title: \"2 matches\""), "{text}");
        assert!(text.contains("title: \"\\\"555-1234\\\"\""), "{text}");
        assert!(
            text.contains("chars 5-13 · 1: \\\"555\\\" · 2: \\\"1234\\\""),
            "{text}"
        );
        assert!(text.contains("chars 17-25"), "{text}");
        // The summary copies every match.
        assert!(text.contains("555-1234\\n800-9999"), "{text}");
    }

    #[test]
    fn named_groups_show_their_names_and_optional_ones_may_be_absent() {
        let text = dump(&answer(
            r"(?P<year>\d{4})(-(?P<month>\d\d))? => 2024 and 1999-12",
        ));
        assert!(text.contains("year: \\\"2024\\\""), "{text}");
        assert!(text.contains("month: (did not take part)"), "{text}");
        assert!(text.contains("month: \\\"12\\\""), "{text}");
    }

    #[test]
    fn positions_count_characters_not_bytes() {
        let text = dump(&answer("b+ => héllo bb"));
        assert!(text.contains("chars 6-8"), "{text}");
    }

    #[test]
    fn no_match_and_the_pattern_alone() {
        assert!(dump(&answer(r"\d => letters")).contains("No match"));
        let text = dump(&answer(r"(\w+)@(?P<host>\w+)"));
        assert!(
            text.contains("Valid pattern, capture groups 1, host"),
            "{text}"
        );
        assert!(dump(&answer("abc")).contains("no capture groups"));
    }

    #[test]
    fn flags_and_unicode_work() {
        assert!(dump(&answer("(?i)rust => I like RUST")).contains("1 match"));
        assert!(dump(&answer(r"\p{Greek}+ => alpha α β")).contains("2 matches"));
    }

    #[test]
    fn a_bad_pattern_gets_the_reason_not_a_failure() {
        let text = dump(&answer("(abc => text"));
        assert!(text.contains("Invalid pattern:"), "{text}");
        assert!(text.contains("unclosed group"), "{text}");
        let lookahead = dump(&answer("a(?=b) => ab"));
        assert!(lookahead.contains("Invalid pattern:"), "{lookahead}");
        // Too big to compile is refused, not attempted.
        let big = dump(&answer(r"(?:\w{1000}){1000} => x"));
        assert!(big.contains("Invalid pattern:"), "{big}");
    }

    #[test]
    fn many_matches_are_counted_but_few_are_listed() {
        let rows = answer(&format!("a => {}", "a".repeat(100)));
        let text = dump(&rows);
        assert!(text.contains("100 matches"), "{text}");
        assert!(text.contains("80 more matches not listed"), "{text}");
        assert_eq!(rows.len(), 1 + MAX_ROWS + 1);
        let capped = dump(&answer(&format!(" => {}", "a".repeat(20_000))));
        assert!(capped.contains("10000+ matches"), "{capped}");
        assert!(answer(&format!("a => {}", "a".repeat(100)))
            .iter()
            .all(|row| row.problems().is_empty()));
    }

    #[test]
    fn control_characters_are_shown_as_escapes() {
        assert_eq!(visible("a\tb\nc\u{1}"), "a\\tb\\nc\\u{1}");
        let text = dump(&answer(r"\s+ => a   b"));
        assert!(text.contains("1 match"), "{text}");
    }

    #[test]
    fn a_pattern_cannot_hang_it() {
        // Catastrophic for a backtracking engine; instant here.
        let started = std::time::Instant::now();
        let text = dump(&answer(&format!("(a+)+$ => {}b", "a".repeat(5000))));
        assert!(text.contains("No match"), "{text}");
        assert!(started.elapsed().as_secs() < 5);
    }

    #[test]
    fn an_empty_query_shows_usage() {
        let rows = answer("   ");
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.problems().is_empty()));
    }

    #[test]
    fn the_separator_may_appear_in_the_text() {
        let text = dump(&answer("x => x => y"));
        assert!(text.contains("1 match"), "{text}");
    }
}
