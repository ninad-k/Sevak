//! Shortcut text (`Super+Space`, `ctrl + alt + K`) as Sevak understands it.
//!
//! The global-shortcut plugin's own parser knows `Super`, `Cmd` and `Command`
//! but not `Win`, `Windows` or `Meta`, and it cannot say which Windows virtual
//! key a shortcut stands for. This module normalizes the spelling before the
//! plugin sees it ([`normalize`]), reads a shortcut as modifiers plus a Windows
//! virtual-key code for the keyboard hook ([`Combo::parse`]) and shows a
//! shortcut the way the current OS names its keys ([`display`]).
//!
//! Pure string handling, compiled and tested on every platform.

use std::fmt;

/// A shortcut as the keyboard hook matches it: which modifiers are held and
/// which (Windows virtual-key code) key is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The Windows key (`Super`, `Win`, `Cmd`, `Meta`).
    pub win: bool,
    /// Windows virtual-key code of the main key.
    pub vk: u32,
}

/// The modifier a token names, as `(ctrl, alt, shift, win)` flags; `None` if
/// it is not a modifier. `CmdOrCtrl` is Cmd on macOS and Ctrl elsewhere, like
/// the plugin's parser.
fn modifier(token: &str) -> Option<(bool, bool, bool, bool)> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some((true, false, false, false)),
        "alt" | "option" => Some((false, true, false, false)),
        "shift" => Some((false, false, true, false)),
        "super" | "win" | "windows" | "meta" | "cmd" | "command" => {
            Some((false, false, false, true))
        }
        "cmdorctrl" | "cmdorcontrol" | "commandorctrl" | "commandorcontrol" => Some((
            !cfg!(target_os = "macos"),
            false,
            false,
            cfg!(target_os = "macos"),
        )),
        _ => None,
    }
}

/// Windows names the plugin's parser does not know are rewritten to `Super`;
/// everything else is only trimmed. `"ctrl + Win + Space"` becomes
/// `"ctrl+Super+Space"`. Text that is not a shortcut stays as wrong as it was,
/// so the parser still reports it.
pub fn normalize(text: &str) -> String {
    text.split('+')
        .map(|token| {
            let token = token.trim();
            if matches!(
                token.to_ascii_lowercase().as_str(),
                "win" | "windows" | "meta"
            ) {
                "Super"
            } else {
                token
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Canonical key names with their Windows virtual-key codes. The names are the
/// ones the settings recorder writes and the plugin's parser accepts.
const KEYS: &[(&str, u32)] = &[
    ("Space", 0x20),
    ("Enter", 0x0D),
    ("Tab", 0x09),
    ("Escape", 0x1B),
    ("Backspace", 0x08),
    ("Delete", 0x2E),
    ("Insert", 0x2D),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("`", 0xC0),
    ("-", 0xBD),
    ("=", 0xBB),
    (",", 0xBC),
    (".", 0xBE),
    ("/", 0xBF),
    ("\\", 0xDC),
    (";", 0xBA),
    ("'", 0xDE),
    ("[", 0xDB),
    ("]", 0xDD),
    ("PrintScreen", 0x2C),
    ("ScrollLock", 0x91),
    ("Pause", 0x13),
    ("NumLock", 0x90),
    ("CapsLock", 0x14),
    ("NumpadMultiply", 0x6A),
    ("NumpadAdd", 0x6B),
    ("NumpadSubtract", 0x6D),
    ("NumpadDecimal", 0x6E),
    ("NumpadDivide", 0x6F),
];

/// Other spellings of the keys in [`KEYS`] (upper case, as compared).
const ALIASES: &[(&str, &str)] = &[
    ("ESC", "Escape"),
    ("RETURN", "Enter"),
    ("DEL", "Delete"),
    ("PGUP", "PageUp"),
    ("PGDN", "PageDown"),
    ("ARROWLEFT", "Left"),
    ("ARROWUP", "Up"),
    ("ARROWRIGHT", "Right"),
    ("ARROWDOWN", "Down"),
    ("BACKQUOTE", "`"),
    ("MINUS", "-"),
    ("EQUAL", "="),
    ("COMMA", ","),
    ("PERIOD", "."),
    ("SLASH", "/"),
    ("BACKSLASH", "\\"),
    ("SEMICOLON", ";"),
    ("QUOTE", "'"),
    ("BRACKETLEFT", "["),
    ("BRACKETRIGHT", "]"),
    ("PAUSEBREAK", "Pause"),
    ("NUMADD", "NumpadAdd"),
    ("NUMPADPLUS", "NumpadAdd"),
    ("NUMPLUS", "NumpadAdd"),
    ("NUMSUBTRACT", "NumpadSubtract"),
    ("NUMMULTIPLY", "NumpadMultiply"),
    ("NUMDIVIDE", "NumpadDivide"),
    ("NUMDECIMAL", "NumpadDecimal"),
];

/// The virtual-key code of a key name, or `None` if the hook cannot match it.
fn key_vk(token: &str) -> Option<u32> {
    let upper = token.to_ascii_uppercase();
    let name = ALIASES
        .iter()
        .find(|(alias, _)| *alias == upper)
        .map_or(upper.as_str(), |(_, canonical)| *canonical);

    // KeyA / A, Digit1 / 1: the code of the character itself.
    let single = |text: &str| {
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    };
    if let Some(c) = single(name.strip_prefix("KEY").unwrap_or(name)) {
        if c.is_ascii_uppercase() {
            return Some(u32::from(c));
        }
    }
    if let Some(c) = single(name.strip_prefix("DIGIT").unwrap_or(name)) {
        if c.is_ascii_digit() {
            return Some(u32::from(c));
        }
    }
    // Numpad5 / Num5.
    let numpad = name
        .strip_prefix("NUMPAD")
        .or_else(|| name.strip_prefix("NUM"));
    if let Some(c) = numpad.and_then(single) {
        if let Some(digit) = c.to_digit(10) {
            return Some(0x60 + digit);
        }
    }
    // F1 to F24.
    if let Some(number) = name.strip_prefix('F') {
        let plain = !number.is_empty()
            && !number.starts_with('0')
            && number.bytes().all(|b| b.is_ascii_digit());
        if plain {
            if let Ok(n) = number.parse::<u32>() {
                if (1..=24).contains(&n) {
                    return Some(0x6F + n);
                }
            }
        }
    }
    KEYS.iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, vk)| *vk)
}

/// The canonical name of a virtual-key code (the inverse of [`key_vk`]).
fn vk_name(vk: u32) -> Option<String> {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(|c| c.to_string()),
        0x60..=0x69 => Some(format!("Numpad{}", vk - 0x60)),
        0x70..=0x87 => Some(format!("F{}", vk - 0x6F)),
        _ => KEYS
            .iter()
            .find(|(_, code)| *code == vk)
            .map(|(name, _)| (*name).to_owned()),
    }
}

impl Combo {
    /// Reads a shortcut. Fails for text that is not a shortcut and for keys the
    /// hook cannot recognise (the plugin's parser has the final say on those).
    pub fn parse(text: &str) -> Result<Self, String> {
        let tokens: Vec<&str> = text.split('+').map(str::trim).collect();
        let Some((key, modifiers)) = tokens.split_last() else {
            return Err("no key given".to_owned());
        };
        let mut combo = Self {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            vk: 0,
        };
        for token in modifiers {
            let (ctrl, alt, shift, win) = modifier(token).ok_or_else(|| {
                if token.is_empty() {
                    "empty key name".to_owned()
                } else {
                    format!("\"{token}\" is not a modifier")
                }
            })?;
            combo.ctrl |= ctrl;
            combo.alt |= alt;
            combo.shift |= shift;
            combo.win |= win;
        }
        if key.is_empty() {
            return Err("no key given".to_owned());
        }
        if modifier(key).is_some() {
            return Err("only modifiers given, no key".to_owned());
        }
        combo.vk = key_vk(key).ok_or_else(|| format!("unknown key \"{key}\""))?;
        Ok(combo)
    }

    /// Whether the combination needs the keyboard hook: the OS reserves the
    /// Windows key combinations, so they cannot be registered the normal way.
    pub fn needs_hook(&self) -> bool {
        self.win
    }

    /// The canonical text of the shortcut, such as `Ctrl+Alt+Super+K`: what the
    /// plugin's parser reads back as the same shortcut.
    pub fn accelerator(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.alt {
            parts.push("Alt".to_owned());
        }
        if self.shift {
            parts.push("Shift".to_owned());
        }
        if self.win {
            parts.push("Super".to_owned());
        }
        parts.push(vk_name(self.vk).unwrap_or_else(|| format!("0x{:02X}", self.vk)));
        parts.join("+")
    }
}

impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.accelerator())
    }
}

/// The operating systems whose key names [`display`] knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyNames {
    Windows,
    MacOs,
    Linux,
}

impl KeyNames {
    /// The OS Sevak was built for.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// A shortcut with the OS's names for its keys: `Super` is `Win` on Windows and
/// `Cmd` on macOS, `Alt` is `Option` on macOS, and key names get their usual
/// capitals. Words it does not know are kept as typed.
pub fn display(text: &str, names: KeyNames) -> String {
    text.split('+')
        .map(|token| {
            let token = token.trim();
            match (modifier(token), token.to_ascii_lowercase().as_str(), names) {
                (Some((false, false, false, true)), _, KeyNames::Windows) => "Win".to_owned(),
                (Some((false, false, false, true)), _, KeyNames::MacOs) => "Cmd".to_owned(),
                (Some((false, false, false, true)), _, KeyNames::Linux) => "Super".to_owned(),
                (Some((false, true, false, false)), _, KeyNames::MacOs) => "Option".to_owned(),
                (Some((true, false, false, false)), _, _) => "Ctrl".to_owned(),
                (Some((false, true, false, false)), _, _) => "Alt".to_owned(),
                (Some((false, false, true, false)), _, _) => "Shift".to_owned(),
                _ => key_vk(token)
                    .and_then(vk_name)
                    .unwrap_or_else(|| token.to_owned()),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Whether two spellings are the same shortcut (`Win+Space`, `super + space`).
/// Text the hook cannot read is compared with whitespace and case ignored.
pub fn same(a: &str, b: &str) -> bool {
    match (Combo::parse(a), Combo::parse(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => squash(a) == squash(b),
    }
}

fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// Whether the text names Super+Space (Win+Space, Cmd+Space, Meta+Space): the
/// shortcut the OS usually reserves for itself.
pub fn is_super_space(text: &str) -> bool {
    Combo::parse(text).is_ok_and(|combo| {
        combo
            == Combo {
                ctrl: false,
                alt: false,
                shift: false,
                win: true,
                vk: 0x20,
            }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(text: &str) -> Combo {
        Combo::parse(text).unwrap_or_else(|err| panic!("{text}: {err}"))
    }

    #[test]
    fn the_windows_key_has_many_names() {
        for name in [
            "Super", "Win", "Windows", "Meta", "Cmd", "Command", "SUPER", "win",
        ] {
            let parsed = combo(&format!("{name}+Space"));
            assert!(parsed.win && parsed.needs_hook(), "{name}");
            assert_eq!(parsed.vk, 0x20);
        }
    }

    #[test]
    fn normalize_rewrites_only_what_the_plugin_does_not_know() {
        assert_eq!(normalize("Win+Space"), "Super+Space");
        assert_eq!(normalize("ctrl + Windows + K"), "ctrl+Super+K");
        assert_eq!(normalize("Meta+F9"), "Super+F9");
        assert_eq!(normalize("Super+Space"), "Super+Space");
        assert_eq!(normalize("Cmd+Space"), "Cmd+Space");
        assert_eq!(normalize("Alt+"), "Alt+");
        assert_eq!(normalize("Banana+K"), "Banana+K");
    }

    #[test]
    fn modifiers_and_keys_parse() {
        assert_eq!(
            combo("Ctrl+Alt+Shift+Super+K"),
            Combo {
                ctrl: true,
                alt: true,
                shift: true,
                win: true,
                vk: u32::from(b'K'),
            }
        );
        assert_eq!(combo("alt + space").vk, 0x20);
        assert!(!combo("alt + space").needs_hook());
        assert_eq!(combo("Ctrl+KeyA").vk, u32::from(b'A'));
        assert_eq!(combo("Ctrl+Digit7").vk, u32::from(b'7'));
        assert_eq!(combo("Ctrl+7").vk, u32::from(b'7'));
        assert_eq!(combo("F9").vk, 0x78);
        assert_eq!(combo("Alt+F24").vk, 0x87);
        assert_eq!(combo("Ctrl+Numpad5").vk, 0x65);
        assert_eq!(combo("Ctrl+Esc").vk, 0x1B);
        assert_eq!(combo("Ctrl+ArrowUp").vk, 0x26);
        assert_eq!(combo("Ctrl+Comma").vk, 0xBC);
        assert_eq!(combo("Ctrl+,").vk, 0xBC);
        assert_eq!(combo("Ctrl+BracketLeft").vk, 0xDB);
    }

    #[test]
    fn cmd_or_ctrl_follows_the_platform() {
        let parsed = combo("CmdOrCtrl+K");
        if cfg!(target_os = "macos") {
            assert!(parsed.win && !parsed.ctrl);
        } else {
            assert!(parsed.ctrl && !parsed.win);
        }
    }

    #[test]
    fn repeated_modifiers_are_one() {
        assert_eq!(combo("Ctrl+Control+K"), combo("Ctrl+K"));
    }

    #[test]
    fn bad_shortcuts_are_rejected() {
        for bad in [
            "",
            "Alt+",
            "+K",
            "Banana+K",
            "Ctrl+Shift",
            "Super",
            "Ctrl+Banana",
            "F25",
            "F0",
            "F01",
            "Ctrl+Space+K",
        ] {
            assert!(Combo::parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn the_canonical_text_reads_back_as_the_same_shortcut() {
        for text in [
            "Super+Space",
            "Ctrl+Alt+Shift+Super+K",
            "Alt+F12",
            "Ctrl+Numpad3",
            "Ctrl+,",
            "Shift+Backspace",
            "Super+PageDown",
            "Ctrl+Alt+9",
            "Ctrl+NumpadAdd",
            "Ctrl+Escape",
            "Alt+Up",
        ] {
            let parsed = combo(text);
            assert_eq!(combo(&parsed.accelerator()), parsed, "{text}");
        }
        assert_eq!(combo("win+ctrl+space").accelerator(), "Ctrl+Super+Space");
        assert_eq!(combo("win+Space").to_string(), "Super+Space");
    }

    #[test]
    fn display_uses_the_names_of_the_os() {
        assert_eq!(display("Super+Space", KeyNames::Windows), "Win+Space");
        assert_eq!(display("Super+Space", KeyNames::MacOs), "Cmd+Space");
        assert_eq!(display("Super+Space", KeyNames::Linux), "Super+Space");
        assert_eq!(display("Win + Alt + K", KeyNames::MacOs), "Cmd+Option+K");
        assert_eq!(display("alt+space", KeyNames::Windows), "Alt+Space");
        assert_eq!(display("alt+space", KeyNames::MacOs), "Option+Space");
        assert_eq!(display("control+shift+K", KeyNames::Linux), "Ctrl+Shift+K");
        assert_eq!(display("Banana", KeyNames::Linux), "Banana");
    }

    #[test]
    fn spellings_of_one_shortcut_are_the_same() {
        assert!(same("Win+Space", "super + SPACE"));
        assert!(same("Ctrl+Alt+T", "alt+control+t"));
        assert!(!same("Super+Space", "Alt+Space"));
        // Unreadable text still compares sensibly.
        assert!(same("Banana+K", "banana + k"));
        assert!(!same("Banana+K", "Banana+L"));
    }

    #[test]
    fn super_space_is_recognised_in_any_spelling() {
        for text in ["Super+Space", "Win+Space", "cmd + space", "Meta+Space"] {
            assert!(is_super_space(text), "{text}");
        }
        for text in ["Alt+Space", "Super+Shift+Space", "Super+K", "", "Banana"] {
            assert!(!is_super_space(text), "{text}");
        }
    }
}
