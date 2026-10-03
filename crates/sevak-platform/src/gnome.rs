//! Global-hotkey setup for Wayland sessions via GNOME custom keybindings.
//!
//! Wayland applications cannot grab global keys, so Sevak lets the desktop own
//! the shortcut: it registers a custom shortcut that runs `sevak --toggle`, and
//! the already-running instance shows or hides its window.
//!
//! Everything except [`install_shortcut`] is pure string handling, compiled on
//! every platform so it can be unit-tested anywhere.

use std::collections::BTreeSet;

use crate::error::{PlatformError, Result};

/// dconf path under which GNOME stores custom shortcuts.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const CUSTOM_KEYBINDINGS_BASE: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/";

/// A GNOME shortcut that already uses the key combination Sevak was bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutConflict {
    pub schema: String,
    pub key: String,
}

impl ShortcutConflict {
    /// Shell command that unbinds the conflicting shortcut.
    pub fn clear_command(&self) -> String {
        format!("gsettings set {} {} \"[]\"", self.schema, self.key)
    }
}

/// What [`install_shortcut`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GnomeSetupReport {
    /// dconf path of the custom keybinding entry.
    pub path: String,
    /// The GNOME accelerator that was bound, e.g. `<Alt>space`.
    pub accelerator: String,
    /// The command the shortcut runs.
    pub command: String,
    /// True when an earlier Sevak entry was updated instead of a new one added.
    pub reused_existing: bool,
    /// Other GNOME shortcuts using the same keys. They may take precedence.
    pub conflicts: Vec<ShortcutConflict>,
}

impl GnomeSetupReport {
    /// Multi-line, human-readable summary (CLI output and the settings window).
    pub fn describe(&self) -> String {
        let action = if self.reused_existing {
            "reused"
        } else {
            "created"
        };
        let mut text = format!(
            "GNOME shortcut {action}: {} -> {}\n  dconf path: {}\n",
            self.accelerator, self.command, self.path
        );
        for conflict in &self.conflicts {
            text.push_str(&format!(
                "warning: GNOME already binds this key to {} {}; \
                 GNOME may give that binding precedence.\n  \
                 To free it, run: {}\n",
                conflict.schema,
                conflict.key,
                conflict.clear_command()
            ));
        }
        text
    }
}

fn invalid_hotkey(hotkey: &str, reason: impl Into<String>) -> PlatformError {
    PlatformError::InvalidHotkey {
        hotkey: hotkey.to_owned(),
        reason: reason.into(),
    }
}

/// Converts a Tauri-style accelerator (`Alt+Space`) to GNOME syntax (`<Alt>space`).
pub fn to_gnome_accelerator(hotkey: &str) -> Result<String> {
    let tokens: Vec<&str> = hotkey.split('+').map(str::trim).collect();
    let (key_token, modifier_tokens) = tokens
        .split_last()
        .expect("split always yields at least one token");

    if key_token.is_empty() {
        return Err(invalid_hotkey(hotkey, "no key given"));
    }

    // A set keeps the output order stable (Control, Alt, Shift, Super) and
    // silently dedupes repeated modifiers.
    let mut modifiers = BTreeSet::new();
    for token in modifier_tokens {
        match modifier_rank(token) {
            Some(rank) => {
                modifiers.insert(rank);
            }
            None if token.is_empty() => return Err(invalid_hotkey(hotkey, "empty key name")),
            None => {
                return Err(invalid_hotkey(
                    hotkey,
                    format!("\"{token}\" is not a modifier"),
                ))
            }
        }
    }

    if modifier_rank(key_token).is_some() {
        return Err(invalid_hotkey(hotkey, "only modifiers given, no key"));
    }
    let key = gnome_key_name(key_token)
        .ok_or_else(|| invalid_hotkey(hotkey, format!("unknown key \"{key_token}\"")))?;

    const NAMES: [&str; 4] = ["<Control>", "<Alt>", "<Shift>", "<Super>"];
    let mut out: String = modifiers.into_iter().map(|rank| NAMES[rank]).collect();
    out.push_str(&key);
    Ok(out)
}

/// Index into the Control, Alt, Shift, Super order.
fn modifier_rank(token: &str) -> Option<usize> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "cmdorctrl" | "commandorcontrol" => Some(0),
        "alt" | "option" => Some(1),
        "shift" => Some(2),
        "super" | "meta" | "cmd" | "command" | "win" | "windows" => Some(3),
        _ => None,
    }
}

/// Maps a key name to its X11/GDK keysym name as GNOME expects it.
fn gnome_key_name(token: &str) -> Option<String> {
    let lower = token.to_ascii_lowercase();

    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphanumeric() {
            return Some(c.to_string());
        }
        return punctuation_keysym(c).map(str::to_owned);
    }

    // DOM-style names: KeyA, Digit1.
    if let Some(rest) = lower.strip_prefix("key") {
        let mut rest_chars = rest.chars();
        if let (Some(c), None) = (rest_chars.next(), rest_chars.next()) {
            if c.is_ascii_alphabetic() {
                return Some(c.to_string());
            }
        }
    }
    if let Some(rest) = lower.strip_prefix("digit") {
        let mut rest_chars = rest.chars();
        if let (Some(c), None) = (rest_chars.next(), rest_chars.next()) {
            if c.is_ascii_digit() {
                return Some(c.to_string());
            }
        }
    }

    if let Some(number) = lower.strip_prefix('f') {
        if let Ok(n) = number.parse::<u8>() {
            if (1..=24).contains(&n) && !number.starts_with('0') {
                return Some(format!("F{n}"));
            }
        }
    }

    let name = match lower.as_str() {
        "space" => "space",
        "enter" | "return" => "Return",
        "tab" => "Tab",
        "esc" | "escape" => "Escape",
        "backspace" => "BackSpace",
        "delete" | "del" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "Page_Up",
        "pagedown" => "Page_Down",
        "up" | "arrowup" => "Up",
        "down" | "arrowdown" => "Down",
        "left" | "arrowleft" => "Left",
        "right" | "arrowright" => "Right",
        "plus" => "plus",
        "backquote" => "grave",
        "minus" => "minus",
        "equal" => "equal",
        "comma" => "comma",
        "period" => "period",
        "slash" => "slash",
        "backslash" => "backslash",
        "semicolon" => "semicolon",
        "quote" => "apostrophe",
        "bracketleft" => "bracketleft",
        "bracketright" => "bracketright",
        _ => return None,
    };
    Some(name.to_owned())
}

fn punctuation_keysym(c: char) -> Option<&'static str> {
    Some(match c {
        '`' => "grave",
        '-' => "minus",
        '=' => "equal",
        ',' => "comma",
        '.' => "period",
        '/' => "slash",
        '\\' => "backslash",
        ';' => "semicolon",
        '\'' => "apostrophe",
        '[' => "bracketleft",
        ']' => "bracketright",
        _ => return None,
    })
}

/// Parses a quoted GVariant string starting at the iterator's next character.
fn parse_quoted(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let quote = chars.next().filter(|c| matches!(c, '\'' | '"'))?;
    let mut out = String::new();
    loop {
        match chars.next()? {
            c if c == quote => return Some(out),
            '\\' => out.push(match chars.next()? {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                other => other,
            }),
            c => out.push(c),
        }
    }
}

/// Parses `gsettings` array-of-strings output: `@as []`, `[]`, `['a', 'b']`.
pub fn parse_gvariant_strv(text: &str) -> Option<Vec<String>> {
    let text = text.trim();
    let text = text.strip_prefix("@as").map_or(text, str::trim_start);
    let mut chars = text.chars().peekable();

    if chars.next()? != '[' {
        return None;
    }
    let skip_ws = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
    };

    let mut items = Vec::new();
    skip_ws(&mut chars);
    if chars.next_if_eq(&']').is_some() {
        skip_ws(&mut chars);
        return chars.next().is_none().then_some(items);
    }
    loop {
        skip_ws(&mut chars);
        items.push(parse_quoted(&mut chars)?);
        skip_ws(&mut chars);
        match chars.next()? {
            ',' => {}
            ']' => break,
            _ => return None,
        }
    }
    skip_ws(&mut chars);
    chars.next().is_none().then_some(items)
}

/// Parses a single quoted GVariant string (`'abc'` or `"abc"`).
pub fn parse_gvariant_string(text: &str) -> Option<String> {
    let mut chars = text.trim().chars().peekable();
    let value = parse_quoted(&mut chars)?;
    chars.next().is_none().then_some(value)
}

/// Formats a string as a single-quoted GVariant literal.
pub fn format_gvariant_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if matches!(c, '\\' | '\'') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// Formats a string array as a GVariant literal; empty arrays need the `@as` type tag.
pub fn format_gvariant_strv(items: &[String]) -> String {
    if items.is_empty() {
        return "@as []".to_owned();
    }
    let joined: Vec<String> = items.iter().map(|s| format_gvariant_string(s)).collect();
    format!("[{}]", joined.join(", "))
}

/// Parses a GNOME accelerator such as `<Primary><Alt>space` into a normalized
/// `(modifiers, key)` pair so differently spelled equivalents compare equal.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn normalize_accelerator(accel: &str) -> Option<(BTreeSet<String>, String)> {
    let mut rest = accel.trim();
    let mut modifiers = BTreeSet::new();
    while let Some(after) = rest.strip_prefix('<') {
        let (name, tail) = after.split_once('>')?;
        let name = name.to_ascii_lowercase();
        modifiers.insert(match name.as_str() {
            "primary" | "ctrl" | "control" => "control".to_owned(),
            "mod1" => "alt".to_owned(),
            "mod4" => "super".to_owned(),
            _ => name,
        });
        rest = tail;
    }
    let key = rest.trim().to_ascii_lowercase();
    (!key.is_empty()).then_some((modifiers, key))
}

/// Quotes `s` for a POSIX shell. GNOME runs custom-shortcut commands through
/// `g_shell_parse_argv`, which follows the same rules.
pub fn shell_quote(s: &str) -> String {
    let plain = !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '/' | '.' | '_' | '+' | '-' | ':' | '=' | '@' | ',')
        });
    if plain {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The command a desktop shortcut should run to toggle Sevak.
///
/// Inside an AppImage `current_exe` points into a mount that changes on every
/// run, so the stable `$APPIMAGE` path is preferred.
pub fn toggle_command() -> Result<String> {
    let exe = match std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        Some(appimage) => std::path::PathBuf::from(appimage),
        None => std::env::current_exe()?,
    };
    Ok(format!("{} --toggle", shell_quote(&exe.to_string_lossy())))
}

/// Human-readable steps for binding `command` by hand on any desktop.
pub fn manual_instructions(hotkey: &str, command: &str) -> String {
    format!(
        "Bind {hotkey} to this command in your desktop environment:\n\
         \n\
         \x20   {command}\n\
         \n\
         GNOME:\n\
         \x20 Settings -> Keyboard -> View and Customize Shortcuts -> Custom Shortcuts -> +\n\
         \x20 Name: Sevak, Command: {command}, Shortcut: {hotkey}\n\
         \n\
         KDE Plasma 6:\n\
         \x20 System Settings -> Keyboard -> Shortcuts -> Add New -> Command or Script...\n\
         \x20 Command: {command}, then assign {hotkey}\n\
         \n\
         Sway / i3 (config file; example for Alt+Space, adjust to your key):\n\
         \x20 bindsym Mod1+space exec {command}\n\
         \n\
         Hyprland (hyprland.conf; example for Alt+Space, adjust to your key):\n\
         \x20 bind = ALT, SPACE, exec, {command}\n"
    )
}

/// Collects conflicts from one `gsettings list-recursively <schema>` output.
///
/// Each line is `schema key value`; array values (and single strings) are
/// compared against `accel` after normalization.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn find_conflicts(output: &str, accel: &str) -> Vec<ShortcutConflict> {
    let Some(target) = normalize_accelerator(accel) else {
        return Vec::new();
    };
    let mut conflicts = Vec::new();
    for line in output.lines() {
        let mut parts = line.splitn(3, ' ');
        let (Some(schema), Some(key), Some(value)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let accels = parse_gvariant_strv(value)
            .or_else(|| parse_gvariant_string(value).map(|s| vec![s]))
            .unwrap_or_default();
        if accels
            .iter()
            .any(|a| normalize_accelerator(a).as_ref() == Some(&target))
        {
            let conflict = ShortcutConflict {
                schema: schema.to_owned(),
                key: key.to_owned(),
            };
            if !conflicts.contains(&conflict) {
                conflicts.push(conflict);
            }
        }
    }
    conflicts
}

/// Picks `customN` with the smallest N not used by `existing`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn first_free_path(existing: &[String]) -> String {
    (0..)
        .map(|n| format!("{CUSTOM_KEYBINDINGS_BASE}custom{n}/"))
        .find(|candidate| !existing.contains(candidate))
        .expect("an unbounded range always yields a free path")
}

/// Registers (or updates) a GNOME custom shortcut running `command`.
///
/// Idempotent: an existing entry whose command mentions `sevak` and ends in
/// `--toggle` is updated instead of adding a duplicate. Other GNOME shortcuts
/// that use the same keys are reported but never changed.
#[cfg(target_os = "linux")]
pub fn install_shortcut(hotkey: &str, command: &str) -> Result<GnomeSetupReport> {
    const LIST_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const LIST_KEY: &str = "custom-keybindings";
    const ENTRY_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
    const CONFLICT_SCHEMAS: [&str; 4] = [
        "org.gnome.desktop.wm.keybindings",
        "org.gnome.shell.keybindings",
        "org.gnome.mutter.keybindings",
        LIST_SCHEMA,
    ];

    let accelerator = to_gnome_accelerator(hotkey)?;

    let raw_list = gsettings(&["get", LIST_SCHEMA, LIST_KEY])?;
    let mut paths =
        parse_gvariant_strv(&raw_list).ok_or_else(|| PlatformError::UnexpectedOutput {
            command: format!("gsettings get {LIST_SCHEMA} {LIST_KEY}"),
            output: raw_list.trim().to_owned(),
        })?;

    let existing = paths.iter().find(|path| {
        let schema_path = format!("{ENTRY_SCHEMA}:{path}");
        match gsettings(&["get", &schema_path, "command"]) {
            Ok(out) => parse_gvariant_string(&out).is_some_and(|cmd| {
                cmd.to_ascii_lowercase().contains("sevak") && cmd.trim_end().ends_with("--toggle")
            }),
            Err(err) => {
                tracing::debug!(%path, %err, "could not read custom keybinding");
                false
            }
        }
    });
    let reused_existing = existing.is_some();
    let path = existing.cloned().unwrap_or_else(|| first_free_path(&paths));

    // Fill in the entry before listing it so GNOME never sees a half-built one.
    let schema_path = format!("{ENTRY_SCHEMA}:{path}");
    gsettings(&[
        "set",
        &schema_path,
        "name",
        &format_gvariant_string("Sevak"),
    ])?;
    gsettings(&[
        "set",
        &schema_path,
        "command",
        &format_gvariant_string(command),
    ])?;
    gsettings(&[
        "set",
        &schema_path,
        "binding",
        &format_gvariant_string(&accelerator),
    ])?;

    if !reused_existing {
        paths.push(path.clone());
        gsettings(&["set", LIST_SCHEMA, LIST_KEY, &format_gvariant_strv(&paths)])?;
    }

    let mut conflicts = Vec::new();
    for schema in CONFLICT_SCHEMAS {
        match gsettings(&["list-recursively", schema]) {
            Ok(output) => {
                for conflict in find_conflicts(&output, &accelerator) {
                    if !conflicts.contains(&conflict) {
                        conflicts.push(conflict);
                    }
                }
            }
            // Older or non-GNOME-flavoured setups lack some schemas.
            Err(err) => tracing::debug!(schema, %err, "skipping conflict check"),
        }
    }

    Ok(GnomeSetupReport {
        path,
        accelerator,
        command: command.to_owned(),
        reused_existing,
        conflicts,
    })
}

/// The settings window's "Set up GNOME shortcut" button.
///
/// `Ok` carries the text to show: the install report on GNOME, or manual
/// instructions on other Linux desktops. `Err` carries the failure followed by
/// the manual instructions. Off Linux there is nothing to set up.
pub fn setup_for_ui(hotkey: &str) -> std::result::Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let command = toggle_command()
            .map_err(|err| format!("cannot determine the sevak executable: {err}"))?;
        if !crate::session::is_gnome() {
            return Ok(format!(
                "This is not a GNOME session, so the shortcut cannot be installed \
                 automatically.\n\n{}",
                manual_instructions(hotkey, &command)
            ));
        }
        match install_shortcut(hotkey, &command) {
            Ok(report) => Ok(report.describe()),
            Err(err) => Err(format!(
                "{err}\n\n{}",
                manual_instructions(hotkey, &command)
            )),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = hotkey;
        Err("Desktop shortcuts are only managed on Linux.".to_owned())
    }
}

/// Runs `gsettings` with `args` (no shell involved) and returns its stdout.
#[cfg(target_os = "linux")]
fn gsettings(args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("gsettings")
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(PlatformError::CommandFailed {
            command: format!("gsettings {}", args.join(" ")),
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accel(hotkey: &str) -> String {
        to_gnome_accelerator(hotkey).unwrap()
    }

    #[test]
    fn report_describes_conflicts_with_their_fix() {
        let report = GnomeSetupReport {
            path: "/org/x/custom0/".to_owned(),
            accelerator: "<Alt>space".to_owned(),
            command: "sevak --toggle".to_owned(),
            reused_existing: false,
            conflicts: vec![ShortcutConflict {
                schema: "org.gnome.desktop.wm.keybindings".to_owned(),
                key: "activate-window-menu".to_owned(),
            }],
        };
        let text = report.describe();
        assert!(text.starts_with("GNOME shortcut created: <Alt>space -> sevak --toggle\n"));
        assert!(text.contains("dconf path: /org/x/custom0/"));
        assert!(text.contains(
            "gsettings set org.gnome.desktop.wm.keybindings activate-window-menu \"[]\""
        ));
    }

    #[test]
    fn converts_common_accelerators() {
        assert_eq!(accel("Alt+Space"), "<Alt>space");
        assert_eq!(accel("Ctrl+Shift+K"), "<Control><Shift>k");
        assert_eq!(
            accel("Super+Alt+Ctrl+Shift+F12"),
            "<Control><Alt><Shift><Super>F12"
        );
        assert_eq!(accel("CmdOrCtrl+KeyA"), "<Control>a");
        assert_eq!(accel("shift+digit1"), "<Shift>1");
        assert_eq!(accel("ctrl + alt + Delete"), "<Control><Alt>Delete");
        assert_eq!(accel("Alt+PageUp"), "<Alt>Page_Up");
        assert_eq!(accel("Win+ArrowLeft"), "<Super>Left");
        assert_eq!(accel("Ctrl+`"), "<Control>grave");
        assert_eq!(accel("Ctrl+Backquote"), "<Control>grave");
        assert_eq!(accel("Ctrl+'"), "<Control>apostrophe");
        assert_eq!(accel("Alt+Return"), "<Alt>Return");
        assert_eq!(accel("F1"), "F1");
        assert_eq!(accel("Alt+Esc"), "<Alt>Escape");
    }

    #[test]
    fn dedupes_modifiers() {
        assert_eq!(accel("Ctrl+Control+CmdOrCtrl+A"), "<Control>a");
    }

    #[test]
    fn rejects_bad_accelerators() {
        for bad in [
            "",
            "   ",
            "Alt+",
            "Alt+Shift",
            "Ctrl+Frobnicate",
            "Foo+A",
            "A+B",
            "Alt++A",
            "F0",
            "F25",
            "F01",
        ] {
            assert!(
                matches!(
                    to_gnome_accelerator(bad),
                    Err(PlatformError::InvalidHotkey { .. })
                ),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn parses_strv() {
        assert_eq!(parse_gvariant_strv("@as []"), Some(vec![]));
        assert_eq!(parse_gvariant_strv("[]\n"), Some(vec![]));
        assert_eq!(
            parse_gvariant_strv("['/org/a/', '/org/b/']\n"),
            Some(vec!["/org/a/".to_owned(), "/org/b/".to_owned()])
        );
        assert_eq!(
            parse_gvariant_strv(r#"["a", 'it\'s', "q\"x", 'back\\slash']"#),
            Some(vec![
                "a".to_owned(),
                "it's".to_owned(),
                "q\"x".to_owned(),
                "back\\slash".to_owned()
            ])
        );
    }

    #[test]
    fn rejects_malformed_strv() {
        for bad in [
            "",
            "[",
            "['a'",
            "['a' 'b']",
            "['a',]",
            "'a'",
            "[1, 2]",
            "['a'] x",
            "['unterminated]",
        ] {
            assert_eq!(parse_gvariant_strv(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn strv_roundtrip() {
        let items = vec!["a".to_owned(), "it's \\ fine".to_owned(), String::new()];
        assert_eq!(
            parse_gvariant_strv(&format_gvariant_strv(&items)),
            Some(items)
        );
        assert_eq!(format_gvariant_strv(&[]), "@as []");
        assert_eq!(
            format_gvariant_strv(&["a".to_owned(), "b".to_owned()]),
            "['a', 'b']"
        );
    }

    #[test]
    fn string_parse_and_format() {
        assert_eq!(parse_gvariant_string("'abc'\n"), Some("abc".to_owned()));
        assert_eq!(parse_gvariant_string("\"a'b\""), Some("a'b".to_owned()));
        assert_eq!(
            parse_gvariant_string(r"'a\\b\'c'"),
            Some("a\\b'c".to_owned())
        );
        assert_eq!(parse_gvariant_string("abc"), None);
        assert_eq!(parse_gvariant_string("'abc"), None);
        assert_eq!(parse_gvariant_string("'a' 'b'"), None);
        assert_eq!(format_gvariant_string(r"it's a\b"), r"'it\'s a\\b'");
        let tricky = "x'\\y\"z";
        assert_eq!(
            parse_gvariant_string(&format_gvariant_string(tricky)),
            Some(tricky.to_owned())
        );
    }

    #[test]
    fn shell_quoting() {
        assert_eq!(shell_quote("/usr/bin/sevak"), "/usr/bin/sevak");
        assert_eq!(shell_quote("a-b_c.d:e=f@g,h+i"), "a-b_c.d:e=f@g,h+i");
        assert_eq!(
            shell_quote("/home/me/My Apps/sevak"),
            "'/home/me/My Apps/sevak'"
        );
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn normalizes_equivalent_accelerators() {
        assert_eq!(
            normalize_accelerator("<Primary><alt>Space"),
            normalize_accelerator("<Alt><Control>space")
        );
        assert_ne!(
            normalize_accelerator("<Alt>space"),
            normalize_accelerator("<Super>space")
        );
        assert_eq!(normalize_accelerator("<Alt>"), None);
        assert_eq!(normalize_accelerator("<Alt space"), None);
        assert_eq!(normalize_accelerator(""), None);
    }

    #[test]
    fn finds_conflicts_in_list_recursively_output() {
        let output = "\
org.gnome.desktop.wm.keybindings activate-window-menu ['<Alt>space']
org.gnome.desktop.wm.keybindings switch-input-source ['<Super>space', 'XF86Keyboard']
org.gnome.desktop.wm.keybindings close @as []
org.gnome.desktop.wm.keybindings weird uint32 3
";
        let conflicts = find_conflicts(output, "<Alt>space");
        assert_eq!(
            conflicts,
            vec![ShortcutConflict {
                schema: "org.gnome.desktop.wm.keybindings".to_owned(),
                key: "activate-window-menu".to_owned(),
            }]
        );
        assert_eq!(
            conflicts[0].clear_command(),
            "gsettings set org.gnome.desktop.wm.keybindings activate-window-menu \"[]\""
        );
        assert_eq!(find_conflicts(output, "<Super>space").len(), 1);
        assert!(find_conflicts(output, "<Control>space").is_empty());
    }

    #[test]
    fn picks_smallest_free_custom_path() {
        let base = CUSTOM_KEYBINDINGS_BASE;
        assert_eq!(first_free_path(&[]), format!("{base}custom0/"));
        let existing = vec![format!("{base}custom0/"), format!("{base}custom2/")];
        assert_eq!(first_free_path(&existing), format!("{base}custom1/"));
    }

    #[test]
    fn manual_instructions_contain_command() {
        let command = "'/opt/My Apps/sevak' --toggle";
        let text = manual_instructions("Alt+Space", command);
        assert!(text.contains(command));
        assert!(text.contains("bindsym Mod1+space exec '/opt/My Apps/sevak' --toggle"));
        assert!(text.contains("bind = ALT, SPACE, exec, '/opt/My Apps/sevak' --toggle"));
        for desktop in ["GNOME", "KDE Plasma 6", "Sway", "Hyprland"] {
            assert!(text.contains(desktop), "missing {desktop}");
        }
    }

    #[test]
    fn toggle_command_ends_with_toggle() {
        let command = toggle_command().unwrap();
        assert!(command.ends_with(" --toggle"));
    }
}
