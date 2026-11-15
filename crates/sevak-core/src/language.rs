//! The interface language setting (`[general] language`).
//!
//! The UI carries its own message catalogues (`ui/src/locales`); this module
//! only knows which languages exist, so the config can be normalized and the
//! shell can pick a language for the strings it draws itself (the tray menu).
//! See docs/localisation.md.

/// `language = "auto"`: follow the operating system's language.
pub const AUTO: &str = "auto";

/// The languages Sevak ships, as lowercase ISO 639-1 codes. English first: it
/// is the source language and the fallback.
pub const LANGUAGES: &[&str] = &["en", "es", "de", "fr", "hi"];

/// Normalizes a configured value: a shipped language code (any case, spaces
/// ignored) or `auto`. Anything else, including an empty string, is `auto`, so
/// a typo falls back to the OS language instead of failing the whole file.
#[must_use]
pub fn normalize(setting: &str) -> String {
    let wanted = setting.trim().to_ascii_lowercase();
    if LANGUAGES.contains(&wanted.as_str()) {
        wanted
    } else {
        AUTO.to_owned()
    }
}

/// The language to show for `setting`: the configured one when Sevak ships it,
/// else (for `auto`) the first of the OS's `preferred` locales that Sevak has,
/// matched on the primary subtag (`de-AT`, `de_DE.UTF-8` → `de`), else English.
#[must_use]
pub fn resolve<'a, I>(setting: &str, preferred: I) -> &'static str
where
    I: IntoIterator<Item = &'a str>,
{
    let wanted = normalize(setting);
    if let Some(lang) = shipped(&wanted) {
        return lang;
    }
    preferred
        .into_iter()
        .find_map(|tag| shipped(primary_subtag(tag)))
        .unwrap_or("en")
}

fn shipped(code: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .copied()
        .find(|lang| lang.eq_ignore_ascii_case(code))
}

/// `de-AT` → `de`; `pt_BR.UTF-8` → `pt`; `C` → `c`.
fn primary_subtag(tag: &str) -> &str {
    tag.trim()
        .split(['-', '_', '.', '@'])
        .next()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_keeps_shipped_codes_and_turns_the_rest_into_auto() {
        assert_eq!(normalize(" DE "), "de");
        assert_eq!(normalize("hi"), "hi");
        assert_eq!(normalize("auto"), "auto");
        assert_eq!(normalize(""), "auto");
        assert_eq!(normalize("klingon"), "auto");
    }

    #[test]
    fn resolve_prefers_the_setting_then_the_os_then_english() {
        assert_eq!(resolve("fr", ["de-DE"]), "fr");
        assert_eq!(resolve("auto", ["ja-JP", "es_MX.UTF-8", "de"]), "es");
        assert_eq!(resolve("auto", ["C", "POSIX"]), "en");
        assert_eq!(resolve("xx", Vec::<&str>::new()), "en");
    }
}
