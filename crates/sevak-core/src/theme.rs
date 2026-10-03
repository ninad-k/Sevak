//! Turns the `[appearance]` settings into the CSS the UI applies.
//!
//! Every value is validated here, not when the config is parsed: a bad accent
//! or an out-of-range size falls back to its default with a warning (logged,
//! and shown in the settings window) instead of failing the whole file.
//!
//! The result is two stylesheets the frontend injects in this order, after the
//! built-in theme: [`ResolvedAppearance::css`] (the settings above) and
//! [`ResolvedAppearance::custom_css`] (the user's own file), so the user's file
//! can override both. See `docs/themes.md` for the variable names.

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

use serde::Serialize;

use crate::config::AppearanceConfig;

pub const MIN_FONT_SIZE: u32 = 12;
pub const MAX_FONT_SIZE: u32 = 22;
pub const DEFAULT_FONT_SIZE: u32 = 15;
pub const MIN_OPACITY: u32 = 30;
pub const MAX_OPACITY: u32 = 100;
pub const MAX_RADIUS: u32 = 32;
pub const DEFAULT_RADIUS: u32 = 14;
/// Largest custom stylesheet that is loaded.
pub const MAX_CUSTOM_CSS_BYTES: u64 = 64 * 1024;
const MAX_FONT_FAMILY_CHARS: usize = 200;

/// What the frontend needs to look the way the user configured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedAppearance {
    /// `:root { ... }` rules for the validated settings.
    pub css: String,
    /// The user's stylesheet; empty when none is configured or it was rejected.
    pub custom_css: String,
    /// Why a setting was ignored, one sentence each.
    pub warnings: Vec<String>,
}

/// Validates `appearance` and builds its CSS. `config_dir` is where
/// `custom_css` is looked up.
pub fn resolve(appearance: &AppearanceConfig, config_dir: &Path) -> ResolvedAppearance {
    let mut warnings = Vec::new();
    let mut warn = |message: String| {
        tracing::warn!("appearance: {message}");
        warnings.push(message);
    };
    let mut rules: Vec<String> = Vec::new();

    let accent = appearance.accent.trim();
    if !accent.is_empty() {
        match parse_color(accent) {
            Some(rgb) => rules.extend(accent_rules(rgb)),
            None => warn(format!(
                "accent \"{accent}\" is not a color (use #rrggbb, #rgb or rgb(r, g, b)); \
                 using the theme's accent"
            )),
        }
    }

    let font_size = if (MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&appearance.font_size) {
        appearance.font_size
    } else {
        warn(format!(
            "font_size {} is outside {MIN_FONT_SIZE}-{MAX_FONT_SIZE}; using {DEFAULT_FONT_SIZE}",
            appearance.font_size
        ));
        DEFAULT_FONT_SIZE
    };
    rules.push(format!("--font-size: {font_size}px"));
    rules.push(format!(
        "--font-scale: {:.3}",
        f64::from(font_size) / f64::from(DEFAULT_FONT_SIZE)
    ));

    let radius = if appearance.radius <= MAX_RADIUS {
        appearance.radius
    } else {
        warn(format!(
            "radius {} is outside 0-{MAX_RADIUS}; using {DEFAULT_RADIUS}",
            appearance.radius
        ));
        DEFAULT_RADIUS
    };
    rules.push(format!("--radius: {radius}px"));

    let opacity = if (MIN_OPACITY..=MAX_OPACITY).contains(&appearance.opacity) {
        appearance.opacity
    } else {
        warn(format!(
            "opacity {} is outside {MIN_OPACITY}-{MAX_OPACITY}; using {MAX_OPACITY}",
            appearance.opacity
        ));
        MAX_OPACITY
    };
    rules.push(format!("--card-opacity: {}", f64::from(opacity) / 100.0));

    let family = appearance.font_family.trim();
    if !family.is_empty() {
        match font_family_css(family) {
            Some(css) => rules.push(format!("font-family: {css}")),
            None => warn(format!(
                "font_family \"{family}\" has characters that are not allowed in a font \
                 name; using the system font"
            )),
        }
    }

    let custom = appearance.custom_css.trim();
    let custom_css = if custom.is_empty() {
        String::new()
    } else {
        load_custom_css(config_dir, custom).unwrap_or_else(|reason| {
            warn(format!("custom_css \"{custom}\" was not loaded: {reason}"));
            String::new()
        })
    };

    ResolvedAppearance {
        css: format!(":root {{\n  {};\n}}\n", rules.join(";\n  ")),
        custom_css,
        warnings,
    }
}

/// Checks the settings that can be checked without touching the disk, for the
/// settings window (a hand-crafted save must not write what would be ignored).
/// The custom stylesheet's path is checked for its shape, not its existence.
pub fn validate(appearance: &AppearanceConfig) -> Result<(), String> {
    let custom = appearance.custom_css.trim();
    if !custom.is_empty() {
        check_css_path(custom).map_err(|reason| format!("custom_css: {reason}"))?;
    }
    let mut without_file = appearance.clone();
    without_file.custom_css.clear();
    match resolve(&without_file, Path::new(""))
        .warnings
        .into_iter()
        .next()
    {
        Some(warning) => Err(warning),
        None => Ok(()),
    }
}

/// Parses `#rgb`, `#rrggbb` or `rgb(r, g, b)` (commas or spaces) into RGB.
pub fn parse_color(text: &str) -> Option<[u8; 3]> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('#') {
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let digit = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok();
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        return match hex.len() {
            3 => Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17]),
            6 => Some([byte(0)?, byte(2)?, byte(4)?]),
            _ => None,
        };
    }

    let inner = text
        .get(..4)
        .filter(|head| head.eq_ignore_ascii_case("rgb("))
        .and_then(|_| text[4..].strip_suffix(')'))?;
    let parts: Vec<&str> = inner
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    let [r, g, b] = parts.as_slice() else {
        return None;
    };
    // `u8::from_str` rejects 256 and above, signs aside from a leading `+`.
    let channel = |part: &str| part.parse::<u8>().ok();
    Some([channel(r)?, channel(g)?, channel(b)?])
}

/// The accent variables derived from one color.
fn accent_rules([r, g, b]: [u8; 3]) -> [String; 4] {
    let darker = |c: u8| (f64::from(c) * 0.82).round() as u8;
    // Relative luminance (sRGB weights, gamma ignored: good enough to pick
    // black or white text).
    let luma = 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
    let on_accent = if luma > 140.0 { "#1c1b2e" } else { "#ffffff" };
    [
        format!("--accent: #{r:02x}{g:02x}{b:02x}"),
        format!(
            "--accent-strong: #{:02x}{:02x}{:02x}",
            darker(r),
            darker(g),
            darker(b)
        ),
        format!("--on-accent: {on_accent}"),
        format!("--selected: rgba({r}, {g}, {b}, 0.17)"),
    ]
}

const GENERIC_FAMILIES: [&str; 12] = [
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
];

/// Builds a `font-family` value from a comma-separated list, quoting names
/// itself so nothing the user typed can end the declaration. `None` if a name
/// has characters beyond letters, digits, spaces, `-`, `_` and `.`.
fn font_family_css(list: &str) -> Option<String> {
    if list.chars().count() > MAX_FONT_FAMILY_CHARS {
        return None;
    }
    let mut families = Vec::new();
    for name in list.split(',') {
        let name = name.trim();
        let name = name
            .strip_prefix('"')
            .and_then(|n| n.strip_suffix('"'))
            .or_else(|| name.strip_prefix('\'').and_then(|n| n.strip_suffix('\'')))
            .unwrap_or(name)
            .trim();
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
        {
            return None;
        }
        families.push(
            if GENERIC_FAMILIES.contains(&name.to_ascii_lowercase().as_str()) {
                name.to_ascii_lowercase()
            } else {
                format!("\"{name}\"")
            },
        );
    }
    Some(families.join(", "))
}

/// The stylesheet name as a relative path without roots, prefixes or `..`.
fn check_css_path(name: &str) -> Result<&Path, String> {
    let relative = Path::new(name);
    if relative.has_root()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err("it must be a path inside the config folder, without \"..\"".to_owned());
    }
    Ok(relative)
}

/// Reads the stylesheet `name` from inside `config_dir`. The path must be
/// relative, free of `..`, and still inside the directory once symlinks are
/// resolved; the file must be UTF-8 and at most [`MAX_CUSTOM_CSS_BYTES`].
fn load_custom_css(config_dir: &Path, name: &str) -> Result<String, String> {
    let relative = check_css_path(name)?;
    let full = config_dir.join(relative);
    let inside = match (config_dir.canonicalize(), full.canonicalize()) {
        (Ok(dir), Ok(file)) => file.starts_with(&dir).then_some(file),
        (_, Err(err)) => return Err(format!("cannot read it: {err}")),
        (Err(err), _) => return Err(format!("cannot read the config folder: {err}")),
    };
    let Some(file) = inside else {
        return Err("it resolves to a place outside the config folder".to_owned());
    };

    let mut bytes = Vec::new();
    File::open(&file)
        .and_then(|f| f.take(MAX_CUSTOM_CSS_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|err| format!("cannot read it: {err}"))?;
    if bytes.len() as u64 > MAX_CUSTOM_CSS_BYTES {
        return Err(format!(
            "it is larger than {} KiB",
            MAX_CUSTOM_CSS_BYTES / 1024
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| "it is not valid UTF-8".to_owned())?;
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn appearance() -> AppearanceConfig {
        AppearanceConfig::default()
    }

    #[test]
    fn colors_parse_in_all_supported_forms() {
        assert_eq!(parse_color("#7c3aed"), Some([0x7c, 0x3a, 0xed]));
        assert_eq!(parse_color("#7C3AED"), Some([0x7c, 0x3a, 0xed]));
        assert_eq!(parse_color("#fa0"), Some([0xff, 0xaa, 0x00]));
        assert_eq!(parse_color("rgb(124, 58, 237)"), Some([124, 58, 237]));
        assert_eq!(parse_color("RGB(1 2 3)"), Some([1, 2, 3]));
        assert_eq!(parse_color(" rgb(0,0,0) "), Some([0, 0, 0]));
    }

    #[test]
    fn bad_colors_are_rejected() {
        for bad in [
            "",
            "#",
            "#12",
            "#1234",
            "#12345",
            "#1234567",
            "#ggg",
            "7c3aed",
            "red",
            "rgb(1,2)",
            "rgb(1,2,3,4)",
            "rgb(256,0,0)",
            "rgb(-1,0,0)",
            "rgb(1.5,0,0)",
            "rgb(1,2,3",
            "rgba(1,2,3,1)",
            "#é12",
        ] {
            assert_eq!(parse_color(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn defaults_set_only_the_numeric_variables() {
        let dir = tempfile::tempdir().unwrap();
        let resolved = resolve(&appearance(), dir.path());
        assert!(resolved.warnings.is_empty());
        assert!(resolved.custom_css.is_empty());
        assert!(resolved.css.contains("--font-size: 15px"));
        assert!(resolved.css.contains("--font-scale: 1.000"));
        assert!(resolved.css.contains("--radius: 14px"));
        assert!(resolved.css.contains("--card-opacity: 1;"));
        assert!(!resolved.css.contains("--accent"));
        assert!(!resolved.css.contains("font-family"));
    }

    #[test]
    fn accent_derives_the_related_variables() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = appearance();
        config.accent = "#7c3aed".to_owned();
        let css = resolve(&config, dir.path()).css;
        assert!(css.contains("--accent: #7c3aed"));
        assert!(css.contains("--accent-strong: #6630c2"), "{css}");
        assert!(css.contains("--on-accent: #ffffff"));
        assert!(css.contains("--selected: rgba(124, 58, 237, 0.17)"));

        // A light accent gets dark text.
        config.accent = "rgb(251, 191, 36)".to_owned();
        assert!(resolve(&config, dir.path())
            .css
            .contains("--on-accent: #1c1b2e"));
    }

    #[test]
    fn invalid_values_fall_back_with_a_warning_each() {
        let dir = tempfile::tempdir().unwrap();
        let config = AppearanceConfig {
            accent: "purple".to_owned(),
            font_size: 99,
            radius: 100,
            opacity: 5,
            font_family: "Evil; } body { x".to_owned(),
            custom_css: "missing.css".to_owned(),
            ..appearance()
        };
        let resolved = resolve(&config, dir.path());
        assert_eq!(resolved.warnings.len(), 6, "{:?}", resolved.warnings);
        assert!(resolved.css.contains("--font-size: 15px"));
        assert!(resolved.css.contains("--radius: 14px"));
        assert!(resolved.css.contains("--card-opacity: 1;"));
        assert!(!resolved.css.contains("--accent"));
        assert!(!resolved.css.contains("Evil"));
        assert!(resolved.custom_css.is_empty());
    }

    #[test]
    fn validate_reports_the_first_problem_and_ignores_missing_files() {
        assert_eq!(validate(&appearance()), Ok(()));
        let missing = AppearanceConfig {
            custom_css: "not-yet-created.css".to_owned(),
            ..appearance()
        };
        assert_eq!(validate(&missing), Ok(()));

        for bad in [
            AppearanceConfig {
                accent: "nope".to_owned(),
                ..appearance()
            },
            AppearanceConfig {
                font_size: 1,
                ..appearance()
            },
            AppearanceConfig {
                custom_css: "../x.css".to_owned(),
                ..appearance()
            },
        ] {
            assert!(validate(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn ranges_include_their_bounds() {
        let dir = tempfile::tempdir().unwrap();
        for (font_size, radius, opacity) in [(12, 0, 30), (22, 32, 100)] {
            let config = AppearanceConfig {
                font_size,
                radius,
                opacity,
                ..appearance()
            };
            assert!(resolve(&config, dir.path()).warnings.is_empty());
        }
    }

    #[test]
    fn font_families_are_quoted_by_us() {
        assert_eq!(
            font_family_css("Fira Sans, sans-serif").as_deref(),
            Some("\"Fira Sans\", sans-serif")
        );
        assert_eq!(
            font_family_css("'Segoe UI', \"Noto Sans\", MONOSPACE").as_deref(),
            Some("\"Segoe UI\", \"Noto Sans\", monospace")
        );
        for bad in [
            "a;b",
            "a{b}",
            "url(x)",
            "a,,b",
            "\"unclosed",
            "a\\b",
            "a/*b",
        ] {
            assert_eq!(font_family_css(bad), None, "{bad:?}");
        }
        assert_eq!(font_family_css(&"a".repeat(201)), None);
    }

    #[test]
    fn custom_css_is_read_from_the_config_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("theme.css"),
            "\u{feff}:root { --bg: #000; }\n",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("themes")).unwrap();
        std::fs::write(dir.path().join("themes").join("a.css"), "x{}").unwrap();

        let mut config = appearance();
        config.custom_css = "theme.css".to_owned();
        let resolved = resolve(&config, dir.path());
        assert!(resolved.warnings.is_empty(), "{:?}", resolved.warnings);
        assert_eq!(resolved.custom_css, ":root { --bg: #000; }\n");

        config.custom_css = "./themes/a.css".to_owned();
        assert_eq!(resolve(&config, dir.path()).custom_css, "x{}");
    }

    #[test]
    fn custom_css_cannot_leave_the_config_folder() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        std::fs::create_dir(&config_dir).unwrap();
        let outside = dir.path().join("secret.css");
        std::fs::write(&outside, "x{}").unwrap();

        for path in [
            "../secret.css",
            "sub/../../secret.css",
            outside.to_str().unwrap(),
        ] {
            let config = AppearanceConfig {
                custom_css: path.to_owned(),
                ..appearance()
            };
            let resolved = resolve(&config, &config_dir);
            assert!(resolved.custom_css.is_empty(), "{path}");
            assert_eq!(resolved.warnings.len(), 1, "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn custom_css_symlinks_out_of_the_folder_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        std::fs::create_dir(&config_dir).unwrap();
        let outside = dir.path().join("secret.css");
        std::fs::write(&outside, "x{}").unwrap();
        std::os::unix::fs::symlink(&outside, config_dir.join("link.css")).unwrap();

        let config = AppearanceConfig {
            custom_css: "link.css".to_owned(),
            ..appearance()
        };
        assert!(resolve(&config, &config_dir).custom_css.is_empty());
    }

    #[test]
    fn oversized_or_non_utf8_stylesheets_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("big.css"),
            vec![b'a'; MAX_CUSTOM_CSS_BYTES as usize + 1],
        )
        .unwrap();
        std::fs::write(dir.path().join("bin.css"), [0xff, 0xfe, 0x00]).unwrap();
        std::fs::write(
            dir.path().join("max.css"),
            vec![b'a'; MAX_CUSTOM_CSS_BYTES as usize],
        )
        .unwrap();

        for (name, loaded) in [("big.css", false), ("bin.css", false), ("max.css", true)] {
            let config = AppearanceConfig {
                custom_css: name.to_owned(),
                ..appearance()
            };
            let resolved = resolve(&config, dir.path());
            assert_eq!(!resolved.custom_css.is_empty(), loaded, "{name}");
            assert_eq!(resolved.warnings.is_empty(), loaded, "{name}");
        }
    }
}
