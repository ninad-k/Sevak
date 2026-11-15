//! The user's preferred languages as the operating system reports them, for
//! the strings the shell draws itself (the tray menu). The windows ask their
//! webview instead (`navigator.languages`). Matching against the shipped
//! languages is `sevak_core::language::resolve`.

/// Locale tags, most preferred first, such as `["de-DE", "en-US"]`. Empty when
/// nothing is known; the caller then falls back to English.
#[must_use]
pub fn preferred_languages() -> Vec<String> {
    let mut tags = os_languages();
    tags.extend(env_languages(|name| std::env::var(name).ok()));
    tags.retain(|tag| !tag.is_empty() && tag != "C" && tag != "POSIX");
    tags
}

/// The POSIX locale variables in the order glibc consults them for messages.
/// `LANGUAGE` may hold a colon-separated list.
fn env_languages(var: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let mut tags = Vec::new();
    for name in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
        if let Some(value) = var(name) {
            tags.extend(
                value
                    .split(':')
                    .map(|tag| tag.trim().to_owned())
                    .filter(|tag| !tag.is_empty()),
            );
        }
    }
    tags
}

/// Windows: the display language of the user's account, then the format locale.
#[cfg(windows)]
fn os_languages() -> Vec<String> {
    use windows::Win32::Globalization::{
        GetUserDefaultLocaleName, GetUserDefaultUILanguage, LCIDToLocaleName,
    };
    let text = |buf: &[u16], len: i32| {
        let len = usize::try_from(len).unwrap_or(0).saturating_sub(1);
        String::from_utf16_lossy(&buf[..len.min(buf.len())])
    };
    let mut tags = Vec::new();
    let mut buf = [0u16; 85];
    // SAFETY: plain Win32 calls writing at most `buf.len()` UTF-16 units.
    let ui = unsafe { LCIDToLocaleName(u32::from(GetUserDefaultUILanguage()), Some(&mut buf), 0) };
    if ui > 0 {
        tags.push(text(&buf, ui));
    }
    // SAFETY: as above.
    let format = unsafe { GetUserDefaultLocaleName(&mut buf) };
    if format > 0 {
        tags.push(text(&buf, format));
    }
    tags
}

/// macOS: GUI apps rarely get `LANG`, so read the language list from the
/// user's global defaults (`AppleLanguages`, e.g. `( "de-DE", "en-US" )`).
#[cfg(target_os = "macos")]
fn os_languages() -> Vec<String> {
    let Ok(output) = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .split(|c: char| c == '"' || c == ',' || c == '(' || c == ')' || c.is_whitespace())
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Linux: the POSIX variables are the OS setting.
#[cfg(target_os = "linux")]
fn os_languages() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_languages_follow_the_posix_order_and_split_language_lists() {
        let vars = |name: &str| match name {
            "LC_MESSAGES" => Some("fr_FR.UTF-8".to_owned()),
            "LANGUAGE" => Some("de:en".to_owned()),
            "LANG" => Some("es_ES.UTF-8".to_owned()),
            _ => None,
        };
        assert_eq!(
            env_languages(vars),
            ["fr_FR.UTF-8", "de", "en", "es_ES.UTF-8"]
        );
    }

    #[test]
    fn preferred_languages_never_lists_the_c_locale() {
        let tags = preferred_languages();
        assert!(tags
            .iter()
            .all(|tag| tag != "C" && tag != "POSIX" && !tag.is_empty()));
    }
}
