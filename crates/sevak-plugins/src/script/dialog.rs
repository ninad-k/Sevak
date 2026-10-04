//! The text of the Allow dialogs.
//!
//! Everything a plugin or workflow author controls (names, commands, folder
//! names, node settings) ends up in a native dialog the user reads to decide
//! whether to run code. So all of it goes through [`sanitize`] first: control
//! characters, line breaks and invisible or direction-changing characters are
//! removed (they could hide text, reorder it or fake extra lines), and long
//! text is cut with an ellipsis. The builders here are pure functions of their
//! inputs so they can be tested without a window.

use super::manifest::Manifest;

/// Longest single value shown (a name, a keyword, a folder name).
pub const MAX_VALUE_CHARS: usize = 80;
/// Longest command line shown. A command that is longer says how much was left
/// out.
pub const MAX_COMMAND_CHARS: usize = 600;

/// Characters that are not control characters but are invisible, join text
/// without a visible mark, or change the direction text is drawn in.
fn is_hidden(ch: char) -> bool {
    matches!(
        ch,
        '\u{00ad}' // soft hyphen
        | '\u{034f}' // combining grapheme joiner
        | '\u{061c}' // Arabic letter mark
        | '\u{115f}' | '\u{1160}' // Hangul fillers
        | '\u{17b4}' | '\u{17b5}' // Khmer inherent vowels
        | '\u{180b}'..='\u{180e}' // Mongolian selectors and separator
        | '\u{200b}'..='\u{200f}' // zero-width space/joiners, direction marks
        | '\u{202a}'..='\u{202e}' // bidi embeddings and overrides
        | '\u{2060}'..='\u{206f}' // word joiner, invisible operators, bidi isolates
        | '\u{3164}' // Hangul filler
        | '\u{feff}' // byte order mark
        | '\u{fff9}'..='\u{fffb}' // interlinear annotation
        | '\u{e0000}'..='\u{e007f}' // tag characters
    )
}

/// Makes `text` safe to show in a dialog: control characters and line breaks
/// become single spaces (or nothing, for hidden and direction-changing
/// characters), runs of spaces collapse, and anything past `max_chars`
/// characters is replaced by an ellipsis.
pub fn sanitize(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut chars = 0;
    let mut pending_space = false;
    let mut truncated = false;
    for ch in text.chars() {
        if is_hidden(ch) {
            continue;
        }
        if ch.is_control() || ch.is_whitespace() || matches!(ch, '\u{2028}' | '\u{2029}') {
            pending_space = !out.is_empty();
            continue;
        }
        if chars >= max_chars {
            truncated = true;
            break;
        }
        if pending_space {
            out.push(' ');
            chars += 1;
            pending_space = false;
            if chars >= max_chars {
                truncated = true;
                break;
            }
        }
        out.push(ch);
        chars += 1;
    }
    if truncated {
        out.push('…');
    }
    out
}

/// [`sanitize`] for a command line, saying how much was left out.
pub fn sanitize_command(text: &str) -> String {
    let shown = sanitize(text, MAX_COMMAND_CHARS);
    if shown.ends_with('…') {
        let total = text.chars().count();
        let left_out = total.saturating_sub(MAX_COMMAND_CHARS);
        format!("{shown} ({left_out} more characters not shown)")
    } else {
        shown
    }
}

/// A short, stable name for a long hash key (`v2:sha256:3fa9...` or
/// `sha256:3fa9...`): the first 12 hex digits, so two folders that look alike
/// can be told apart.
pub fn short_id(key: &str) -> String {
    key.rsplit(':')
        .next()
        .unwrap_or(key)
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(12)
        .collect()
}

/// What the Allow dialog for a script plugin says.
///
/// `folder` is the plugin folder's name, `location` its full path, `key` the
/// approval key (for the short id) and `reviewed_before` whether an earlier
/// allowance exists that no longer matches.
pub fn script_prompt(
    manifest: &Manifest,
    folder: &str,
    location: &str,
    key: &str,
    reviewed_before: bool,
) -> String {
    let mut text = String::new();
    if reviewed_before {
        text.push_str(
            "Sevak found a script plugin to review again.\n\n\
             The plugin's contents changed or this is the first review under the new rules: an \
             allowance now covers the plugin's files and folder, not just its name.\n\n",
        );
    } else {
        text.push_str("Sevak found a script plugin it has not run before.\n\n");
    }
    let value = |text: &str| sanitize(text, MAX_VALUE_CHARS);
    text.push_str(&format!("Name: {}\n", value(&manifest.name)));
    text.push_str(&format!("Keyword: {}\n", value(&manifest.keyword)));
    text.push_str(&format!(
        "Folder: {} (contents id {})\n",
        value(folder),
        short_id(key)
    ));
    text.push_str(&format!(
        "Location: {}\n",
        sanitize(location, MAX_COMMAND_CHARS)
    ));
    text.push_str(&format!(
        "Runs: {}\n",
        sanitize_command(&manifest.command_line())
    ));
    let files = manifest.support_files();
    if !files.is_empty() {
        let files: Vec<String> = files.iter().map(|file| value(file)).collect();
        text.push_str(&format!("Files covered: {}\n", files.join(", ")));
    }
    if manifest.inherit_env.is_empty() {
        text.push_str("Environment: only the standard set; none of your other variables\n");
    } else {
        let names: Vec<String> = manifest.inherit_env.iter().map(|n| value(n)).collect();
        text.push_str(&format!(
            "Environment: also receives your variables {}\n",
            names.join(", ")
        ));
    }
    let capabilities = manifest.capabilities.names();
    if !capabilities.is_empty() {
        text.push_str(&format!(
            "Also can: start applications from its results ({})\n",
            capabilities.join(", ")
        ));
    }
    text.push_str(
        "\nA script plugin runs with your account's permissions, like any program you start. \
         Allow it only if you trust where it came from. You can switch it off any time in \
         Settings.",
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_and_line_breaks_become_single_spaces() {
        assert_eq!(sanitize("a\nb\r\nc\td", 80), "a b c d");
        assert_eq!(
            sanitize("  leading and   trailing  ", 80),
            "leading and trailing"
        );
        assert_eq!(sanitize("bell\u{7}here\u{0}x", 80), "bell here x");
        assert_eq!(sanitize("esc\u{1b}[31mred", 80), "esc [31mred");
        assert_eq!(
            sanitize("line\u{2028}sep\u{2029}para\u{85}next", 80),
            "line sep para next"
        );
        assert_eq!(sanitize("", 80), "");
        assert_eq!(sanitize("\n\n", 80), "");
    }

    #[test]
    fn invisible_and_direction_changing_characters_are_removed() {
        // Right-to-left override: would show "exe.txt" reversed.
        assert_eq!(sanitize("report\u{202e}txt.exe", 80), "reporttxt.exe");
        for hidden in [
            '\u{200b}',
            '\u{200c}',
            '\u{200d}',
            '\u{200e}',
            '\u{200f}',
            '\u{202a}',
            '\u{202b}',
            '\u{202c}',
            '\u{202d}',
            '\u{202e}',
            '\u{2066}',
            '\u{2067}',
            '\u{2068}',
            '\u{2069}',
            '\u{2060}',
            '\u{feff}',
            '\u{00ad}',
            '\u{061c}',
            '\u{e0041}',
        ] {
            assert_eq!(
                sanitize(&format!("a{hidden}b"), 80),
                "ab",
                "{:x}",
                hidden as u32
            );
        }
        // Ordinary non-ASCII text survives.
        assert_eq!(sanitize("Grüße 日本語 ✓", 80), "Grüße 日本語 ✓");
    }

    #[test]
    fn long_text_is_cut_with_an_ellipsis() {
        let long = "x".repeat(500);
        let shown = sanitize(&long, 80);
        assert_eq!(shown.chars().count(), 81);
        assert!(shown.ends_with('…'));
        assert_eq!(
            sanitize(&"x".repeat(80), 80),
            "x".repeat(80),
            "exactly at the limit"
        );
        // Multi-byte characters are counted, not bytes.
        assert_eq!(sanitize(&"é".repeat(100), 10).chars().count(), 11);
    }

    #[test]
    fn a_long_command_says_how_much_was_left_out() {
        let command = format!("python {}", "a".repeat(1_000));
        let shown = sanitize_command(&command);
        assert!(shown.contains('…'));
        assert!(shown.contains("more characters not shown"), "{shown}");
        assert_eq!(sanitize_command("python main.py"), "python main.py");
    }

    #[test]
    fn short_ids_are_twelve_hex_digits() {
        assert_eq!(
            short_id("v2:sha256:0123456789abcdef0123456789abcdef"),
            "0123456789ab"
        );
        assert_eq!(short_id("sha256:ffff"), "ffff");
        assert_eq!(short_id(""), "");
    }

    fn manifest(extra: &str) -> Manifest {
        Manifest::parse(
            &format!(
                "protocol = 1\nname = \"Notes\"\nkeyword = \"n\"\ncommand = [\"python\", \"main.py\"]\n{extra}"
            ),
            "notes",
        )
        .unwrap()
    }

    const KEY: &str = "v2:sha256:aabbccddeeff00112233";

    #[test]
    fn the_script_prompt_names_the_folder_hash_command_and_extras() {
        let m = manifest(
            "inherit_env = [\"OPENAI_API_KEY\"]\ncapabilities = [\"launch\"]\nfiles = [\"lib.py\"]\n",
        );
        let text = script_prompt(&m, "notes", "C:\\cfg\\plugins\\notes", KEY, false);
        assert!(text.contains("has not run before"));
        assert!(text.contains("Name: Notes"));
        assert!(
            text.contains("Folder: notes (contents id aabbccddeeff)"),
            "{text}"
        );
        assert!(text.contains("Location: C:\\cfg\\plugins\\notes"));
        assert!(text.contains("Runs: python main.py"));
        assert!(
            text.contains("Files covered: lib.py, main.py, python"),
            "{text}"
        );
        assert!(text.contains("also receives your variables OPENAI_API_KEY"));
        assert!(text.contains("start applications"));
        assert!(!text.contains("review again"));
    }

    #[test]
    fn the_prompt_for_a_plain_plugin_says_it_gets_no_extras() {
        let text = script_prompt(&manifest(""), "notes", "/p/notes", KEY, false);
        assert!(text.contains("only the standard set"));
        assert!(!text.contains("start applications"));
    }

    #[test]
    fn a_plugin_allowed_before_says_why_it_asks_again() {
        let text = script_prompt(&manifest(""), "notes", "/p/notes", KEY, true);
        assert!(text.contains("review again"));
        assert!(text.contains("contents changed or this is the first review under the new rules"));
    }

    #[test]
    fn author_text_cannot_fake_lines_or_hide_in_the_prompt() {
        let m = Manifest::parse(
            "protocol = 1\nname = \"Safe\\nName: Trusted Tool\\nRuns: nothing\"\nkeyword = \"n\"\n\
             command = [\"python\", \"main.py\\u202e\"]\n",
            "folder",
        )
        .unwrap();
        let text = script_prompt(&m, "fol\nder\u{202e}", "/p/x\nRuns: nothing", KEY, false);
        let lines: Vec<&str> = text.lines().collect();
        let count = |prefix: &str| lines.iter().filter(|l| l.starts_with(prefix)).count();
        assert_eq!(count("Name:"), 1, "{text}");
        assert_eq!(count("Runs:"), 1, "{text}");
        assert_eq!(count("Folder:"), 1, "{text}");
        assert!(!text.contains('\u{202e}'));
        assert!(!text.contains('\u{0}'));
    }

    #[test]
    fn a_huge_name_is_cut() {
        let m = Manifest::parse(
            &format!(
                "protocol = 1\nname = \"{}\"\nkeyword = \"n\"\ncommand = [\"x\"]\n",
                "N".repeat(5_000)
            ),
            "f",
        )
        .unwrap();
        let text = script_prompt(&m, "f", "/p/f", KEY, false);
        assert!(text.len() < 2_500, "{}", text.len());
        assert!(text.contains('…'));
    }
}
