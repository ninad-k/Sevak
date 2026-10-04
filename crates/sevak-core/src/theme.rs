//! Turns the `[appearance]` settings into the CSS the UI applies.
//!
//! Every value is validated here, not when the config is parsed: a bad accent
//! or an out-of-range size falls back to its default with a warning (logged,
//! and shown in the settings window) instead of failing the whole file.
//!
//! The result is two stylesheets the frontend injects in this order, after the
//! built-in theme: [`ResolvedAppearance::css`] (the settings above) and
//! [`ResolvedAppearance::custom_css`] (the user's own file), so the user's file
//! can override both. A theme file (`appearance.theme_file`, see
//! [`crate::theme_file`]) is applied first, so the settings above win over it
//! whenever they differ from their default. See `docs/themes.md` for the
//! variable names.

use std::fs::File;
use std::io::Read;
use std::ops::RangeInclusive;
use std::path::{Component, Path};

use serde::Serialize;

use crate::config::{AppearanceConfig, WindowConfig};
use crate::theme_file::ThemeSpec;
use crate::theme_store;

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
    /// The launcher width the theme file asks for, if it does.
    pub window_width: Option<u32>,
    /// Whether the window gets a blurred backdrop (the setting, minus
    /// platforms that cannot draw one).
    pub blur: bool,
    /// The corner radius in pixels the card ends up with.
    pub radius: u32,
}

/// Windows rounds a backdrop window's corners itself, with a fixed radius.
#[cfg(windows)]
const BLUR_RADIUS: Option<u32> = Some(8);
#[cfg(not(windows))]
const BLUR_RADIUS: Option<u32> = None;

/// Platforms that can blur what is behind the window.
const BLUR_SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

impl ResolvedAppearance {
    /// The launcher width to use, given `window.width` from the config. The
    /// theme's width applies while the config still has the default width, so
    /// a width chosen in Settings wins.
    pub fn window_width_or(&self, configured: u32) -> u32 {
        match self.window_width {
            Some(width) if configured == WindowConfig::default().width => width,
            _ => configured,
        }
    }
}

/// The value a setting takes: what the user configured when it differs from
/// the default, else what the theme file says, else the default. A configured
/// value out of range is reported and replaced the same way.
fn pick(
    name: &str,
    configured: u32,
    default: u32,
    range: RangeInclusive<u32>,
    themed: Option<u32>,
    warn: &mut impl FnMut(String),
) -> u32 {
    let fallback = themed.unwrap_or(default);
    if !range.contains(&configured) {
        warn(format!(
            "{name} {configured} is outside {}-{}; using {fallback}",
            range.start(),
            range.end()
        ));
        fallback
    } else if configured != default {
        configured
    } else {
        fallback
    }
}

/// Validates `appearance` and builds its CSS. `config_dir` is where
/// `theme_file` and `custom_css` are looked up.
pub fn resolve(appearance: &AppearanceConfig, config_dir: &Path) -> ResolvedAppearance {
    let mut warnings = Vec::new();
    let mut warn = |message: String| {
        tracing::warn!("appearance: {message}");
        warnings.push(message);
    };
    let mut rules: Vec<String> = Vec::new();

    let theme_name = appearance.theme_file.trim();
    let themed: Option<ThemeSpec> = if theme_name.is_empty() {
        None
    } else {
        match theme_store::load(config_dir, theme_name) {
            Ok(parsed) => {
                for warning in parsed.warnings {
                    warn(format!("theme_file \"{theme_name}\": {warning}"));
                }
                Some(parsed.spec)
            }
            Err(reason) => {
                warn(format!(
                    "theme_file \"{theme_name}\" was not loaded: {reason}"
                ));
                None
            }
        }
    };
    let theme_css = themed.as_ref().map(ThemeSpec::css).unwrap_or_default();
    let layout = themed.as_ref().map(|spec| spec.layout.clone());

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

    let font_size = pick(
        "font_size",
        appearance.font_size,
        DEFAULT_FONT_SIZE,
        MIN_FONT_SIZE..=MAX_FONT_SIZE,
        themed.as_ref().and_then(|spec| spec.font.size),
        &mut warn,
    );
    rules.push(format!("--font-size: {font_size}px"));
    rules.push(format!(
        "--font-scale: {:.3}",
        f64::from(font_size) / f64::from(DEFAULT_FONT_SIZE)
    ));

    let blur = appearance.blur && BLUR_SUPPORTED;
    let radius = pick(
        "radius",
        appearance.radius,
        DEFAULT_RADIUS,
        0..=MAX_RADIUS,
        layout.as_ref().and_then(|l| l.radius),
        &mut warn,
    );
    let radius = match BLUR_RADIUS {
        Some(fixed) if blur => fixed,
        _ => radius,
    };
    rules.push(format!("--radius: {radius}px"));

    let opacity = pick(
        "opacity",
        appearance.opacity,
        MAX_OPACITY,
        MIN_OPACITY..=MAX_OPACITY,
        layout.as_ref().and_then(|l| l.opacity),
        &mut warn,
    );
    rules.push(format!("--card-opacity: {}", f64::from(opacity) / 100.0));

    let configured_family = appearance.font_family.trim();
    let family = if configured_family.is_empty() {
        themed.as_ref().map_or("", |spec| spec.font.family.as_str())
    } else {
        configured_family
    };
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
        css: format!("{theme_css}:root {{\n  {};\n}}\n", rules.join(";\n  ")),
        custom_css,
        warnings,
        window_width: layout.and_then(|l| l.window_width),
        blur,
        radius,
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
    let theme_file = appearance.theme_file.trim();
    if !theme_file.is_empty() {
        check_css_path(theme_file).map_err(|reason| format!("theme_file: {reason}"))?;
    }
    let mut without_file = appearance.clone();
    without_file.custom_css.clear();
    without_file.theme_file.clear();
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
pub(crate) fn font_family_css(list: &str) -> Option<String> {
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
/// Theme files are read the same way.
pub(crate) fn load_custom_css(config_dir: &Path, name: &str) -> Result<String, String> {
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

    fn with_theme(text: &str) -> (tempfile::TempDir, AppearanceConfig) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("themes")).unwrap();
        std::fs::write(dir.path().join("themes").join("t.toml"), text).unwrap();
        let config = AppearanceConfig {
            theme_file: "themes/t.toml".to_owned(),
            ..appearance()
        };
        (dir, config)
    }

    const THEME: &str = "\
[font]
family = \"Fira Sans, sans-serif\"
size = 18
[layout]
radius = 4
opacity = 80
row_height = 60
window_width = 900
[dark]
background = \"#101010\"
accent = \"#88c0d0\"
";

    #[test]
    fn a_theme_file_comes_before_the_settings_and_supplies_defaults() {
        let (dir, config) = with_theme(THEME);
        let resolved = resolve(&config, dir.path());
        assert!(resolved.warnings.is_empty(), "{:?}", resolved.warnings);
        let css = &resolved.css;
        // The theme's palette and sizes come first, the settings' block last.
        let palette = css.find("--bg: #101010").expect(css);
        let settings = css.find("--font-size").expect(css);
        assert!(palette < settings, "{css}");
        assert!(css.contains("--row-h: 60px"));
        // Settings at their defaults let the theme's values through.
        assert!(css.contains("--font-size: 18px"));
        assert!(css.contains("--font-scale: 1.200"));
        assert!(css.contains("--radius: 4px"));
        assert!(css.contains("--card-opacity: 0.8;"));
        assert!(css.contains("font-family: \"Fira Sans\", sans-serif"));
        assert_eq!(resolved.window_width, Some(900));
    }

    #[test]
    fn settings_that_differ_from_the_default_win_over_the_theme() {
        let (dir, mut config) = with_theme(THEME);
        config.font_size = 13;
        config.radius = 20;
        config.opacity = 55;
        config.font_family = "Georgia".to_owned();
        config.accent = "#ff0000".to_owned();
        let css = resolve(&config, dir.path()).css;
        assert!(css.contains("--font-size: 13px"));
        assert!(css.contains("--radius: 20px"));
        assert!(css.contains("--card-opacity: 0.55;"));
        assert!(css.contains("font-family: \"Georgia\""));
        assert!(!css.contains("Fira"));
        // The accent setting is emitted after the theme's accent, so it wins.
        let theme_accent = css.find("--accent: #88c0d0").expect(&css);
        let setting = css.find("--accent: #ff0000").expect(&css);
        assert!(theme_accent < setting);
    }

    #[test]
    fn an_out_of_range_setting_falls_back_to_the_theme() {
        let (dir, mut config) = with_theme(THEME);
        config.radius = 99;
        let resolved = resolve(&config, dir.path());
        assert_eq!(resolved.warnings.len(), 1);
        assert!(
            resolved.warnings[0].contains("using 4"),
            "{:?}",
            resolved.warnings
        );
        assert!(resolved.css.contains("--radius: 4px"));
    }

    #[test]
    fn problems_with_a_theme_file_are_warnings_not_failures() {
        let (dir, mut config) = with_theme("[dark]\nbackground = \"nope\"\ntext = \"#fff\"\n");
        let resolved = resolve(&config, dir.path());
        assert_eq!(resolved.warnings.len(), 1);
        assert!(resolved.warnings[0].starts_with("theme_file \"themes/t.toml\":"));
        assert!(resolved.css.contains("--fg: #ffffff"));
        assert!(!resolved.css.contains("nope"));

        for bad in ["themes/missing.toml", "../t.toml", "/etc/passwd"] {
            config.theme_file = bad.to_owned();
            let resolved = resolve(&config, dir.path());
            assert_eq!(resolved.warnings.len(), 1, "{bad}");
            assert!(resolved.css.starts_with(":root {"), "{bad}");
            assert_eq!(resolved.window_width, None);
        }
        std::fs::write(dir.path().join("themes").join("t.toml"), "name = ").unwrap();
        config.theme_file = "themes/t.toml".to_owned();
        assert_eq!(resolve(&config, dir.path()).warnings.len(), 1);
    }

    #[test]
    fn custom_css_still_comes_after_the_theme() {
        let (dir, mut config) = with_theme(THEME);
        std::fs::write(dir.path().join("theme.css"), ":root { --bg: red; }").unwrap();
        config.custom_css = "theme.css".to_owned();
        let resolved = resolve(&config, dir.path());
        assert_eq!(resolved.custom_css, ":root { --bg: red; }");
        assert!(resolved.css.contains("--bg: #101010"));
    }

    #[test]
    fn blur_is_reported_only_where_the_platform_can_draw_it() {
        let mut config = appearance();
        assert!(!resolve(&config, Path::new("")).blur);
        config.blur = true;
        config.radius = 20;
        let resolved = resolve(&config, Path::new(""));
        assert_eq!(resolved.blur, cfg!(any(windows, target_os = "macos")));
        // Windows rounds a blurred window itself, with a fixed radius.
        let expected = if cfg!(windows) { 8 } else { 20 };
        assert_eq!(resolved.radius, expected);
        assert!(resolved.css.contains(&format!("--radius: {expected}px")));
    }

    #[test]
    fn the_themes_window_width_yields_to_a_chosen_one() {
        let resolved = ResolvedAppearance {
            window_width: Some(900),
            ..resolve(&appearance(), Path::new(""))
        };
        assert_eq!(resolved.window_width_or(720), 900);
        assert_eq!(resolved.window_width_or(800), 800);
        let none = resolve(&appearance(), Path::new(""));
        assert_eq!(none.window_width_or(720), 720);
    }

    #[test]
    fn validate_checks_the_theme_file_path_but_not_its_existence() {
        let mut config = appearance();
        config.theme_file = "themes/not-yet.toml".to_owned();
        assert_eq!(validate(&config), Ok(()));
        config.theme_file = "../x.toml".to_owned();
        assert!(validate(&config).unwrap_err().starts_with("theme_file:"));
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
