//! Theme files: a named set of colors, fonts and sizes kept in
//! `<config dir>/themes/<name>.toml`.
//!
//! A theme file is validated value by value, like the `[appearance]` settings
//! (see [`crate::theme`]): a value that is not valid is dropped with a warning
//! and the rest of the file still applies. Only a file that is not TOML at all
//! is refused. The same validation runs on the JSON the theme editor sends, so
//! one code path decides what a theme may contain.
//!
//! ```toml
//! name = "Nord"
//! author = "Sevak"
//! description = "Arctic dark colors."
//!
//! [font]
//! family = "Fira Sans, sans-serif"   # optional
//! size = 15
//!
//! [layout]
//! radius = 14
//! opacity = 100
//! row_height = 48
//! search_size = 22
//! icon_size = 32
//! window_width = 720
//!
//! [dark]          # and/or [light]; the variant used follows the light/dark mode
//! background = "#2e3440"
//! text = "#eceff4"
//! ```
//!
//! A theme with one palette applies in every mode; with both, the light one
//! applies in light mode and the dark one in dark mode. Colors a palette leaves
//! out keep the Sevak Light / Sevak Dark value of that mode. See
//! `docs/themes.md`.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::{MAX_WINDOW_WIDTH, MIN_WINDOW_WIDTH};
use crate::theme::{
    font_family_css, parse_color, MAX_FONT_SIZE, MAX_OPACITY, MAX_RADIUS, MIN_FONT_SIZE,
    MIN_OPACITY,
};

pub const MIN_ROW_HEIGHT: u32 = 32;
pub const MAX_ROW_HEIGHT: u32 = 96;
pub const MIN_SEARCH_SIZE: u32 = 14;
pub const MAX_SEARCH_SIZE: u32 = 40;
pub const MIN_ICON_SIZE: u32 = 16;
pub const MAX_ICON_SIZE: u32 = 64;
const MAX_NAME_CHARS: usize = 60;
const MAX_DESCRIPTION_CHARS: usize = 200;
const MAX_SHADOW_LAYERS: usize = 6;
const MAX_SHADOW_LENGTH: f64 = 200.0;

/// WCAG 2.x AA contrast for normal-size text.
pub const AA_NORMAL_TEXT: f64 = 4.5;

/// The light or dark variant of a theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Light => "light",
            Mode::Dark => "dark",
        }
    }
}

/// How a palette entry is validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()` or `rgba()`.
    Color,
    /// A `box-shadow` list, or `none`.
    Shadow,
}

/// One palette entry: its key in the theme file and the CSS variable it sets.
#[derive(Debug, Clone, Copy)]
pub struct ColorKey {
    pub key: &'static str,
    pub var: &'static str,
    pub kind: ValueKind,
}

const fn color(key: &'static str, var: &'static str) -> ColorKey {
    ColorKey {
        key,
        var,
        kind: ValueKind::Color,
    }
}

/// Every palette entry, in the order theme files list them.
pub const COLOR_KEYS: [ColorKey; 20] = [
    color("background", "--bg"),
    color("text", "--fg"),
    color("subtext", "--muted"),
    color("border", "--border"),
    color("accent", "--accent"),
    color("accent_strong", "--accent-strong"),
    color("on_accent", "--on-accent"),
    color("selection", "--selected"),
    color("selection_text", "--selected-fg"),
    color("tile", "--tile"),
    color("kbd_background", "--kbd-bg"),
    color("kbd_border", "--kbd-border"),
    ColorKey {
        key: "shadow",
        var: "--shadow",
        kind: ValueKind::Shadow,
    },
    color("surface", "--surface"),
    color("input_background", "--input-bg"),
    color("input_border", "--input-border"),
    color("switch_off", "--switch-off"),
    color("warn", "--warn"),
    color("error", "--error"),
    color("ok", "--ok"),
];

/// Palette entry key -> normalized value (`#rrggbb`, `rgba(r, g, b, a)` or a
/// shadow list).
pub type Palette = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeFont {
    /// Comma-separated families, as in `appearance.font_family`; empty is the system font.
    pub family: String,
    pub size: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeLayout {
    pub radius: Option<u32>,
    pub opacity: Option<u32>,
    pub row_height: Option<u32>,
    pub search_size: Option<u32>,
    pub icon_size: Option<u32>,
    pub window_width: Option<u32>,
}

/// A validated theme.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeSpec {
    pub name: String,
    pub author: String,
    pub description: String,
    pub font: ThemeFont,
    pub layout: ThemeLayout,
    pub light: Option<Palette>,
    pub dark: Option<Palette>,
}

/// A theme and what was wrong with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParsedTheme {
    pub spec: ThemeSpec,
    /// Why a value was dropped, one sentence each.
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Parsing and validation
// ---------------------------------------------------------------------------

/// Parses the text of a theme file. Fails only when it is not TOML.
pub fn parse(text: &str) -> Result<ParsedTheme, String> {
    let table: toml::Table = text
        .trim_start_matches('\u{feff}')
        .parse()
        .map_err(|err: toml::de::Error| format!("it is not valid TOML: {}", err.message()))?;
    let value = serde_json::to_value(table).map_err(|err| err.to_string())?;
    Ok(from_value(&value))
}

/// Validates a theme given as loosely typed data (a parsed file, or the JSON of
/// the theme editor): every bad value is dropped with a warning.
pub fn from_value(value: &Value) -> ParsedTheme {
    let mut warnings = Vec::new();
    let empty = Map::new();
    let root = value.as_object().unwrap_or(&empty);

    let mut spec = ThemeSpec {
        name: text_field(root, "name", MAX_NAME_CHARS),
        author: text_field(root, "author", MAX_NAME_CHARS),
        description: text_field(root, "description", MAX_DESCRIPTION_CHARS),
        ..ThemeSpec::default()
    };

    if let Some(font) = table(root, "font", &mut warnings) {
        let family = text_field(font, "family", usize::MAX);
        if family.is_empty() {
            // the system font
        } else if font_family_css(&family).is_some() {
            spec.font.family = family;
        } else {
            warnings.push(format!(
                "[font] family \"{family}\" has characters that are not allowed in a font name; ignored"
            ));
        }
        spec.font.size = int(
            font,
            "font",
            "size",
            MIN_FONT_SIZE..=MAX_FONT_SIZE,
            &mut warnings,
        );
    }

    if let Some(layout) = table(root, "layout", &mut warnings) {
        let mut get = |key: &str, range: RangeInclusive<u32>| {
            int(layout, "layout", key, range, &mut warnings)
        };
        spec.layout = ThemeLayout {
            radius: get("radius", 0..=MAX_RADIUS),
            opacity: get("opacity", MIN_OPACITY..=MAX_OPACITY),
            row_height: get("row_height", MIN_ROW_HEIGHT..=MAX_ROW_HEIGHT),
            search_size: get("search_size", MIN_SEARCH_SIZE..=MAX_SEARCH_SIZE),
            icon_size: get("icon_size", MIN_ICON_SIZE..=MAX_ICON_SIZE),
            window_width: get("window_width", MIN_WINDOW_WIDTH..=MAX_WINDOW_WIDTH),
        };
    }

    spec.light = palette(root, "light", &mut warnings);
    spec.dark = palette(root, "dark", &mut warnings);
    ParsedTheme { spec, warnings }
}

/// The sub-table `key`, or `None` (with a warning when it is something else).
fn table<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    warnings: &mut Vec<String>,
) -> Option<&'a Map<String, Value>> {
    match parent.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::Object(map)) => Some(map),
        Some(_) => {
            warnings.push(format!("[{key}] must be a table; ignored"));
            None
        }
    }
}

/// A free-text field: control characters removed, trimmed, cut to `max` characters.
fn text_field(map: &Map<String, Value>, key: &str, max: usize) -> String {
    let text = map.get(key).and_then(Value::as_str).unwrap_or_default();
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    clean.trim().chars().take(max).collect()
}

fn int(
    map: &Map<String, Value>,
    section: &str,
    key: &str,
    range: RangeInclusive<u32>,
    warnings: &mut Vec<String>,
) -> Option<u32> {
    let value = map.get(key)?;
    if value.is_null() {
        return None;
    }
    let Some(number) = value.as_i64() else {
        warnings.push(format!("[{section}] {key} must be a whole number; ignored"));
        return None;
    };
    match u32::try_from(number) {
        Ok(n) if range.contains(&n) => Some(n),
        _ => {
            warnings.push(format!(
                "[{section}] {key} {number} is outside {}-{}; ignored",
                range.start(),
                range.end()
            ));
            None
        }
    }
}

fn palette(root: &Map<String, Value>, name: &str, warnings: &mut Vec<String>) -> Option<Palette> {
    let entries = table(root, name, warnings)?;
    let mut palette = Palette::new();
    for (key, value) in entries {
        let Some(spec) = COLOR_KEYS.iter().find(|spec| spec.key == key) else {
            warnings.push(format!("[{name}] {key} is not a known color; ignored"));
            continue;
        };
        let normalized = value.as_str().and_then(|text| match spec.kind {
            ValueKind::Color => parse_css_color(text).map(format_color),
            ValueKind::Shadow => normalize_shadow(text),
        });
        match normalized {
            Some(text) => {
                palette.insert(key.clone(), text);
            }
            None => warnings.push(match spec.kind {
                ValueKind::Color => format!(
                    "[{name}] {key} is not a color (use #rrggbb, #rrggbbaa, rgb() or rgba()); ignored"
                ),
                ValueKind::Shadow => format!(
                    "[{name}] {key} is not a shadow (use \"none\" or e.g. \"0 8px 28px rgba(0, 0, 0, 0.5)\"); ignored"
                ),
            }),
        }
    }
    Some(palette)
}

// ---------------------------------------------------------------------------
// Colors and shadows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub rgb: [u8; 3],
    /// 0.0 transparent to 1.0 opaque.
    pub alpha: f64,
}

/// Parses `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(r, g, b)` and
/// `rgba(r, g, b, a)` (commas or spaces; alpha 0-1 or a percentage).
pub fn parse_css_color(text: &str) -> Option<Rgba> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('#') {
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        return match hex.len() {
            3 | 6 => parse_color(text).map(opaque),
            4 | 8 => {
                let (color, alpha) = hex.split_at(hex.len() - hex.len() / 4);
                let rgb = parse_color(&format!("#{color}"))?;
                let alpha = if alpha.len() == 1 {
                    u8::from_str_radix(alpha, 16).ok()? * 17
                } else {
                    u8::from_str_radix(alpha, 16).ok()?
                };
                Some(Rgba {
                    rgb,
                    alpha: f64::from(alpha) / 255.0,
                })
            }
            _ => None,
        };
    }

    let lower = text.to_ascii_lowercase();
    let inner = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = inner
        .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    match parts.as_slice() {
        [r, g, b] => parse_color(&format!("rgb({r},{g},{b})")).map(opaque),
        [r, g, b, a] => {
            let rgb = parse_color(&format!("rgb({r},{g},{b})"))?;
            let alpha = match a.strip_suffix('%') {
                Some(percent) => percent.parse::<f64>().ok()? / 100.0,
                None => a.parse::<f64>().ok()?,
            };
            (alpha.is_finite() && (0.0..=1.0).contains(&alpha)).then_some(Rgba { rgb, alpha })
        }
        _ => None,
    }
}

fn opaque(rgb: [u8; 3]) -> Rgba {
    Rgba { rgb, alpha: 1.0 }
}

/// `#rrggbb` when opaque, otherwise `rgba(r, g, b, a)`.
pub fn format_color(color: Rgba) -> String {
    let [r, g, b] = color.rgb;
    if color.alpha >= 0.9995 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("rgba({r}, {g}, {b}, {})", trim_number(color.alpha, 3))
    }
}

/// `value` with at most `decimals` decimals and no trailing zeros.
fn trim_number(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// Splits at `separator` outside parentheses; `None` when they do not balance.
fn split_top_level(text: &str, separator: impl Fn(char) -> bool) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut depth = 0_u32;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1)?,
            c if depth == 0 && separator(c) => {
                parts.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    parts.push(&text[start..]);
    Some(parts)
}

fn shadow_length(token: &str) -> Option<f64> {
    let number = token.strip_suffix("px").unwrap_or(token);
    let value: f64 = number.parse().ok()?;
    (value.is_finite() && value.abs() <= MAX_SHADOW_LENGTH).then_some(value)
}

fn format_length(value: f64) -> String {
    if value == 0.0 {
        "0".to_owned()
    } else {
        format!("{}px", trim_number(value, 2))
    }
}

/// Re-spells a `box-shadow` value (`none`, or up to 6 comma-separated layers of
/// `[inset] x y [blur [spread]] color`) in a canonical form, or `None` if any
/// part is not understood. Nothing else can get through, so a shadow can never
/// end the declaration it is written into.
pub fn normalize_shadow(text: &str) -> Option<String> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("none") {
        return Some("none".to_owned());
    }
    let layers = split_top_level(text, |c| c == ',')?;
    if layers.len() > MAX_SHADOW_LAYERS {
        return None;
    }
    let mut out = Vec::new();
    for layer in layers {
        let mut inset = false;
        let mut lengths = Vec::new();
        let mut color = None;
        for token in split_top_level(layer.trim(), char::is_whitespace)?
            .into_iter()
            .filter(|token| !token.is_empty())
        {
            if token.eq_ignore_ascii_case("inset") && !inset {
                inset = true;
            } else if let Some(length) = shadow_length(token) {
                lengths.push(length);
            } else if color.is_none() {
                color = Some(parse_css_color(token)?);
            } else {
                return None;
            }
        }
        if !(2..=4).contains(&lengths.len()) {
            return None;
        }
        let lengths: Vec<String> = lengths.into_iter().map(format_length).collect();
        out.push(format!(
            "{}{} {}",
            if inset { "inset " } else { "" },
            lengths.join(" "),
            format_color(color?)
        ));
    }
    Some(out.join(", "))
}

// ---------------------------------------------------------------------------
// Contrast (WCAG 2.x)
// ---------------------------------------------------------------------------

fn linear(channel: u8) -> f64 {
    let c = f64::from(channel) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn relative_luminance([r, g, b]: [u8; 3]) -> f64 {
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/// The WCAG contrast ratio of two opaque colors, 1.0 to 21.0.
pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `top` painted over the opaque `bottom`.
pub fn blend(top: Rgba, bottom: [u8; 3]) -> [u8; 3] {
    let mix =
        |t: u8, b: u8| (top.alpha * f64::from(t) + (1.0 - top.alpha) * f64::from(b)).round() as u8;
    [
        mix(top.rgb[0], bottom[0]),
        mix(top.rgb[1], bottom[1]),
        mix(top.rgb[2], bottom[2]),
    ]
}

/// One text/background pair of a palette and whether it reaches WCAG AA.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ContrastCheck {
    pub id: &'static str,
    pub label: &'static str,
    pub ratio: f64,
    pub aa: bool,
}

/// The pairs the editor warns about, for `palette` in `mode` (missing colors
/// take the Sevak default of the mode):
/// text on background, selected-row text on the selection, subtext on
/// background, and the text on an accent-colored button.
pub fn contrast_checks(palette: &Palette, mode: Mode) -> Vec<ContrastCheck> {
    let get = |key: &str| -> Rgba {
        palette
            .get(key)
            .and_then(|text| parse_css_color(text))
            .or_else(|| {
                default_palette(mode)
                    .get(key)
                    .and_then(|text| parse_css_color(text))
            })
            .unwrap_or(opaque([0, 0, 0]))
    };
    let background = get("background").rgb;
    let text = get("text");
    let selection = blend(get("selection"), background);
    let selection_text = if palette.contains_key("selection_text") {
        get("selection_text")
    } else {
        text
    };
    let check = |id, label, foreground: Rgba, behind: [u8; 3]| {
        let ratio = contrast_ratio(blend(foreground, behind), behind);
        ContrastCheck {
            id,
            label,
            ratio,
            aa: ratio >= AA_NORMAL_TEXT,
        }
    };
    vec![
        check("text", "Text on background", text, background),
        check(
            "selection",
            "Selected row text on selection",
            selection_text,
            selection,
        ),
        check(
            "subtext",
            "Subtext on background",
            get("subtext"),
            background,
        ),
        check(
            "on_accent",
            "Text on accent buttons",
            get("on_accent"),
            get("accent").rgb,
        ),
    ]
}

// ---------------------------------------------------------------------------
// Built-in themes
// ---------------------------------------------------------------------------

/// The themes that ship with Sevak. They are also in the online gallery
/// (`gallery/themes.json`), which points at these same files.
const BUILTIN_SOURCES: [&str; 8] = [
    include_str!("../../../gallery/themes/Sevak-Light.toml"),
    include_str!("../../../gallery/themes/Sevak-Dark.toml"),
    include_str!("../../../gallery/themes/Nord.toml"),
    include_str!("../../../gallery/themes/Dracula.toml"),
    include_str!("../../../gallery/themes/Solarized-Light.toml"),
    include_str!("../../../gallery/themes/Solarized-Dark.toml"),
    include_str!("../../../gallery/themes/Gruvbox.toml"),
    include_str!("../../../gallery/themes/High-Contrast.toml"),
];

/// The built-in themes, parsed once.
pub fn builtin_themes() -> &'static [ParsedTheme] {
    static THEMES: OnceLock<Vec<ParsedTheme>> = OnceLock::new();
    THEMES.get_or_init(|| {
        BUILTIN_SOURCES
            .iter()
            .map(|source| parse(source).expect("built-in theme files are valid TOML"))
            .collect()
    })
}

pub fn builtin_by_name(name: &str) -> Option<&'static ParsedTheme> {
    builtin_themes()
        .iter()
        .find(|theme| theme.spec.name.eq_ignore_ascii_case(name.trim()))
}

/// The complete palette of Sevak Light or Sevak Dark: what a palette's missing
/// colors fall back to.
pub fn default_palette(mode: Mode) -> &'static Palette {
    let name = match mode {
        Mode::Light => "Sevak Light",
        Mode::Dark => "Sevak Dark",
    };
    let spec = &builtin_by_name(name)
        .expect("Sevak Light and Sevak Dark are built in")
        .spec;
    match mode {
        Mode::Light => spec.light.as_ref(),
        Mode::Dark => spec.dark.as_ref(),
    }
    .expect("Sevak Light and Sevak Dark carry their own palette")
}

// ---------------------------------------------------------------------------
// Output: TOML and CSS
// ---------------------------------------------------------------------------

fn quoted(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}

impl ThemeSpec {
    /// The theme as the text of a theme file.
    pub fn to_toml(&self) -> String {
        let mut out = format!("name = {}\n", quoted(&self.name));
        if !self.author.is_empty() {
            out.push_str(&format!("author = {}\n", quoted(&self.author)));
        }
        if !self.description.is_empty() {
            out.push_str(&format!("description = {}\n", quoted(&self.description)));
        }

        let mut font = Vec::new();
        if !self.font.family.is_empty() {
            font.push(format!("family = {}", quoted(&self.font.family)));
        }
        if let Some(size) = self.font.size {
            font.push(format!("size = {size}"));
        }
        section(&mut out, "font", &font);

        let layout: Vec<String> = [
            ("radius", self.layout.radius),
            ("opacity", self.layout.opacity),
            ("row_height", self.layout.row_height),
            ("search_size", self.layout.search_size),
            ("icon_size", self.layout.icon_size),
            ("window_width", self.layout.window_width),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|v| format!("{key} = {v}")))
        .collect();
        section(&mut out, "layout", &layout);

        for (name, palette) in [("light", &self.light), ("dark", &self.dark)] {
            let Some(palette) = palette else { continue };
            let lines: Vec<String> = COLOR_KEYS
                .iter()
                .filter_map(|spec| {
                    palette
                        .get(spec.key)
                        .map(|value| format!("{} = {}", spec.key, quoted(value)))
                })
                .collect();
            if lines.is_empty() {
                out.push_str(&format!("\n[{name}]\n"));
            } else {
                section(&mut out, name, &lines);
            }
        }
        out
    }

    /// Both palettes that exist, with their mode.
    pub fn palettes(&self) -> Vec<(Mode, &Palette)> {
        [(Mode::Light, &self.light), (Mode::Dark, &self.dark)]
            .into_iter()
            .filter_map(|(mode, palette)| palette.as_ref().map(|palette| (mode, palette)))
            .collect()
    }

    /// The stylesheet for the palette(s) and the layout sizes the CSS can carry
    /// (`--row-h`, `--search-size`, `--icon-size`). Font size, family, radius and
    /// opacity are merged with the `[appearance]` settings by
    /// [`crate::theme::resolve`] instead.
    ///
    /// The selectors keep the specificity of a plain `:root` rule (see
    /// `ui/src/app.css`), so the accent setting and `custom_css`, which come
    /// later, still override them.
    pub fn css(&self) -> String {
        let mut layout = Vec::new();
        for (var, value) in [
            ("--row-h", self.layout.row_height),
            ("--search-size", self.layout.search_size),
            ("--icon-size", self.layout.icon_size),
        ] {
            if let Some(value) = value {
                layout.push(format!("{var}: {value}px"));
            }
        }

        let palettes = self.palettes();
        let mut out = String::new();
        match palettes.as_slice() {
            [] => {}
            [(mode, palette)] => {
                let mut rules = palette_rules(palette, *mode);
                rules.append(&mut layout);
                out.push_str(&block(":root", &rules));
            }
            _ => {
                out.push_str(&block(":root", &layout));
                layout.clear();
                for (mode, palette) in &palettes {
                    let rules = palette_rules(palette, *mode);
                    let name = mode.as_str();
                    out.push_str(&block(
                        &format!(":root:where([data-theme=\"{name}\"])"),
                        &rules,
                    ));
                    out.push_str(&format!(
                        "@media (prefers-color-scheme: {name}) {{\n{}}}\n",
                        block(":root:where(:not([data-theme]))", &rules)
                            .lines()
                            .map(|line| format!("  {line}\n"))
                            .collect::<String>()
                    ));
                }
            }
        }
        if !layout.is_empty() {
            out.push_str(&block(":root", &layout));
        }
        out
    }
}

fn section(out: &mut String, name: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    out.push_str(&format!("\n[{name}]\n"));
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
}

fn block(selector: &str, rules: &[String]) -> String {
    if rules.is_empty() {
        return String::new();
    }
    format!("{selector} {{\n  {};\n}}\n", rules.join(";\n  "))
}

/// The declarations of one palette: its own colors, the rest from the Sevak
/// theme of the same mode, and `color-scheme`.
fn palette_rules(palette: &Palette, mode: Mode) -> Vec<String> {
    let defaults = default_palette(mode);
    let mut rules: Vec<String> = COLOR_KEYS
        .iter()
        .filter_map(|spec| {
            palette
                .get(spec.key)
                .or_else(|| defaults.get(spec.key))
                .map(|value| format!("{}: {value}", spec.var))
        })
        .collect();
    rules.push(format!("color-scheme: {}", mode.as_str()));
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette_of(pairs: &[(&str, &str)]) -> Palette {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn colors_parse_in_every_supported_spelling() {
        let c = |text: &str| parse_css_color(text).map(format_color);
        assert_eq!(c("#7C3AED").as_deref(), Some("#7c3aed"));
        assert_eq!(c("#fa0").as_deref(), Some("#ffaa00"));
        assert_eq!(c("#ffaa0080").as_deref(), Some("rgba(255, 170, 0, 0.502)"));
        assert_eq!(c("#fa08").as_deref(), Some("rgba(255, 170, 0, 0.533)"));
        assert_eq!(c("rgb(1 2 3)").as_deref(), Some("#010203"));
        assert_eq!(
            c("RGBA(28, 27, 46, 0.12)").as_deref(),
            Some("rgba(28, 27, 46, 0.12)")
        );
        assert_eq!(c("rgba(0,0,0,50%)").as_deref(), Some("rgba(0, 0, 0, 0.5)"));
        assert_eq!(
            c("rgb(0 0 0 / 0.25)").as_deref(),
            Some("rgba(0, 0, 0, 0.25)")
        );
        assert_eq!(c("rgba(0,0,0,1)").as_deref(), Some("#000000"));
    }

    #[test]
    fn bad_colors_are_refused() {
        for bad in [
            "",
            "red",
            "#12",
            "#12345",
            "#ggg",
            "rgb(1,2)",
            "rgb(256,0,0)",
            "rgba(0,0,0,1.5)",
            "rgba(0,0,0,-1)",
            "rgba(0,0,0,nan)",
            "rgba(0,0,0,x)",
            "rgba(0,0,0,0.5",
            "url(x)",
            "#fff; } body { x",
        ] {
            assert_eq!(parse_css_color(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn shadows_are_respelled_and_never_carry_other_syntax() {
        assert_eq!(
            normalize_shadow("0 8px 28px rgba(28,27,46,0.22),0 1px 3px rgba(28, 27, 46, 0.12)")
                .as_deref(),
            Some("0 8px 28px rgba(28, 27, 46, 0.22), 0 1px 3px rgba(28, 27, 46, 0.12)")
        );
        assert_eq!(normalize_shadow("NONE").as_deref(), Some("none"));
        assert_eq!(
            normalize_shadow("inset 0 0 0 2px #fff").as_deref(),
            Some("inset 0 0 0 2px #ffffff")
        );
        for bad in [
            "",
            "0 8px",
            "0 8px 28px",
            "0 8px 28px red",
            "0 8px 28px #000; color: red",
            "0 8px 28px #000 } body { x",
            "0 8px 28px url(x)",
            "0 0 0 0 0 #000",
            "0 999px 0 #000",
            "0 1px 2px #000,",
            "0 1px 2px rgba(0,0,0,0.5",
            "0 1px 2px #000 #fff",
        ] {
            assert_eq!(normalize_shadow(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn contrast_matches_the_wcag_definition() {
        assert!((contrast_ratio([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio([255, 255, 255], [255, 255, 255]) - 1.0).abs() < 1e-9);
        // #767676 on white is the textbook smallest gray that reaches AA.
        let gray = contrast_ratio([0x76, 0x76, 0x76], [255, 255, 255]);
        assert!((4.5..4.6).contains(&gray), "{gray}");
        assert!(contrast_ratio([0x77, 0x77, 0x77], [255, 255, 255]) < 4.5);
        // The order of the two colors does not matter.
        assert_eq!(
            contrast_ratio([10, 20, 30], [200, 210, 220]),
            contrast_ratio([200, 210, 220], [10, 20, 30])
        );
    }

    #[test]
    fn translucent_colors_are_blended_before_they_are_compared() {
        let half = Rgba {
            rgb: [255, 255, 255],
            alpha: 0.5,
        };
        assert_eq!(blend(half, [0, 0, 0]), [128, 128, 128]);
        assert_eq!(blend(opaque([1, 2, 3]), [9, 9, 9]), [1, 2, 3]);

        // Black text on a white selection drawn at 50 % over black is gray.
        let palette = palette_of(&[
            ("background", "#000000"),
            ("text", "#000000"),
            ("selection", "rgba(255, 255, 255, 0.5)"),
        ]);
        let checks = contrast_checks(&palette, Mode::Dark);
        let selection = checks.iter().find(|c| c.id == "selection").unwrap();
        let expected = contrast_ratio([0, 0, 0], [128, 128, 128]);
        assert!((selection.ratio - expected).abs() < 1e-9);
        assert!(!checks.iter().find(|c| c.id == "text").unwrap().aa);
    }

    #[test]
    fn selection_text_defaults_to_the_text_color() {
        let mut palette = palette_of(&[
            ("background", "#ffffff"),
            ("text", "#000000"),
            ("selection", "#ffff00"),
        ]);
        let same = |palette: &Palette| {
            contrast_checks(palette, Mode::Light)
                .into_iter()
                .find(|c| c.id == "selection")
                .unwrap()
                .ratio
        };
        let base = same(&palette);
        palette.insert("selection_text".into(), "#000000".into());
        assert_eq!(same(&palette), base);
        palette.insert("selection_text".into(), "#ffff00".into());
        assert!((same(&palette) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_theme_file_parses_and_round_trips() {
        let text = r##"
name = "  Mine  "
author = "Me"
description = "Line\u0007 one"

[font]
family = "Fira Sans, sans-serif"
size = 17

[layout]
radius = 8
opacity = 90
row_height = 52
search_size = 24
icon_size = 36
window_width = 800

[dark]
background = "#101010"
text = "#FFF"
selection = "rgb(255 0 0 / 20%)"
shadow = "0 4px 8px #0008"
"##;
        let parsed = parse(text).unwrap();
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        let spec = &parsed.spec;
        assert_eq!(spec.name, "Mine");
        assert_eq!(spec.description, "Line one");
        assert_eq!(spec.font.size, Some(17));
        assert_eq!(spec.layout.window_width, Some(800));
        let dark = spec.dark.as_ref().unwrap();
        assert_eq!(dark["text"], "#ffffff");
        assert_eq!(dark["selection"], "rgba(255, 0, 0, 0.2)");
        assert_eq!(dark["shadow"], "0 4px 8px rgba(0, 0, 0, 0.533)");
        assert!(spec.light.is_none());

        let again = parse(&spec.to_toml()).unwrap();
        assert_eq!(again.spec, *spec);
        assert!(again.warnings.is_empty());
    }

    #[test]
    fn bad_values_are_dropped_one_by_one() {
        let text = r##"
name = "Broken"
[font]
family = "Evil; } body { x"
size = 99
[layout]
radius = "round"
opacity = 5
row_height = -3
icon_size = 40
window_width = 100000
[light]
background = "white"
text = "#222"
frobnicate = "#fff"
shadow = "0 0 0 red"
accent = 7
"##;
        let parsed = parse(text).unwrap();
        let spec = &parsed.spec;
        // font family, font size, radius, opacity, row height, window width,
        // background, frobnicate, shadow, accent
        assert_eq!(parsed.warnings.len(), 10, "{:#?}", parsed.warnings);
        assert_eq!(spec.font, ThemeFont::default());
        assert_eq!(
            spec.layout,
            ThemeLayout {
                icon_size: Some(40),
                ..ThemeLayout::default()
            }
        );
        assert_eq!(spec.light.as_ref().unwrap().len(), 1);
        assert_eq!(spec.light.as_ref().unwrap()["text"], "#222222");
    }

    #[test]
    fn only_non_toml_is_refused() {
        assert!(parse("name = ").is_err());
        assert!(parse("[[[").is_err());
        let parsed = parse("").unwrap();
        assert_eq!(parsed.spec, ThemeSpec::default());
        let parsed = parse("light = 3\nfont = \"x\"").unwrap();
        assert_eq!(parsed.warnings.len(), 2);
        assert!(parsed.spec.light.is_none());
    }

    #[test]
    fn editor_json_goes_through_the_same_checks() {
        let value = serde_json::json!({
            "name": "Json",
            "layout": { "radius": 99, "icon_size": 20 },
            "dark": { "background": "#000", "text": "nope" },
            "light": null,
        });
        let parsed = from_value(&value);
        assert_eq!(parsed.warnings.len(), 2);
        assert_eq!(parsed.spec.layout.icon_size, Some(20));
        assert!(parsed.spec.light.is_none());
        assert_eq!(parsed.spec.dark.as_ref().unwrap().len(), 1);
        assert_eq!(from_value(&Value::Null).spec, ThemeSpec::default());
    }

    #[test]
    fn one_palette_applies_in_every_mode() {
        let spec = parse("[dark]\nbackground = \"#101010\"\ntext = \"#fff\"\n")
            .unwrap()
            .spec;
        let css = spec.css();
        assert!(css.starts_with(":root {"), "{css}");
        assert!(css.contains("--bg: #101010"));
        assert!(css.contains("--fg: #ffffff"));
        // Missing colors come from Sevak Dark.
        assert!(css.contains("--accent: #fbbf24"), "{css}");
        assert!(css.contains("color-scheme: dark"));
        assert!(!css.contains("@media"));
        assert!(!css.contains("--selected-fg"));
    }

    #[test]
    fn two_palettes_follow_the_mode() {
        let spec = parse("[light]\nbackground = \"#ffffff\"\n[dark]\nbackground = \"#000000\"\n")
            .unwrap()
            .spec;
        let css = spec.css();
        assert!(
            css.contains(":root:where([data-theme=\"light\"]) {"),
            "{css}"
        );
        assert!(css.contains(":root:where([data-theme=\"dark\"]) {"));
        assert!(css.contains("@media (prefers-color-scheme: dark) {"));
        assert!(css.contains(":root:where(:not([data-theme])) {"));
        assert_eq!(css.matches("--bg:").count(), 4);
    }

    #[test]
    fn layout_sizes_become_variables() {
        let spec =
            parse("[layout]\nrow_height = 60\nsearch_size = 30\nicon_size = 24\nradius = 3\n")
                .unwrap()
                .spec;
        let css = spec.css();
        assert!(css.contains("--row-h: 60px"));
        assert!(css.contains("--search-size: 30px"));
        assert!(css.contains("--icon-size: 24px"));
        // Radius is merged with the settings by theme::resolve, not emitted here.
        assert!(!css.contains("--radius"));
        assert_eq!(ThemeSpec::default().css(), "");
    }

    #[test]
    fn built_in_themes_are_valid_complete_and_distinct() {
        let themes = builtin_themes();
        assert_eq!(themes.len(), 8);
        let mut names: Vec<&str> = themes.iter().map(|t| t.spec.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 8);
        for expected in [
            "Sevak Light",
            "Sevak Dark",
            "Nord",
            "Dracula",
            "Solarized Light",
            "Solarized Dark",
            "Gruvbox",
            "High Contrast",
        ] {
            assert!(builtin_by_name(expected).is_some(), "{expected}");
        }
        for theme in themes {
            let name = &theme.spec.name;
            assert!(theme.warnings.is_empty(), "{name}: {:?}", theme.warnings);
            let palettes = theme.spec.palettes();
            assert!(!palettes.is_empty(), "{name}");
            for (mode, palette) in palettes {
                for key in COLOR_KEYS.iter().filter(|k| k.key != "selection_text") {
                    assert!(palette.contains_key(key.key), "{name}: missing {}", key.key);
                }
                // The file is already in canonical form.
                for value in palette.values() {
                    assert_eq!(
                        parse_css_color(value)
                            .map(format_color)
                            .as_deref()
                            .unwrap_or(value),
                        value
                    );
                }
                assert_eq!(
                    parse(&theme.spec.to_toml()).unwrap().spec,
                    theme.spec,
                    "{name} {mode:?}"
                );
            }
        }
    }

    #[test]
    fn built_in_themes_meet_wcag_aa_for_body_text() {
        for theme in builtin_themes() {
            for (mode, palette) in theme.spec.palettes() {
                for check in contrast_checks(palette, mode) {
                    assert!(
                        check.aa,
                        "{}: {} is {:.2}:1, below {AA_NORMAL_TEXT}",
                        theme.spec.name, check.label, check.ratio
                    );
                }
            }
        }
    }

    /// `ui/src/app.css` is what the launcher shows without any theme file; the
    /// Sevak Light and Sevak Dark files must say the same.
    #[test]
    fn sevak_light_and_dark_match_app_css() {
        let css =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/src/app.css"))
                .unwrap();
        let declarations = |opening: &str| -> BTreeMap<String, String> {
            let body = css.split(opening).nth(1).expect(opening);
            let body = &body[..body.find('}').unwrap()];
            body.lines()
                .filter_map(|line| line.trim().strip_suffix(';')?.split_once(": "))
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect()
        };
        for (mode, opening) in [
            (Mode::Light, "\n:root {\n"),
            (Mode::Dark, "\n:root:where([data-theme=\"dark\"]) {\n"),
        ] {
            let found = declarations(opening);
            let palette = default_palette(mode);
            for key in COLOR_KEYS {
                let Some(expected) = found.get(key.var) else {
                    continue;
                };
                let value = &palette[key.key];
                let normalized = match key.kind {
                    ValueKind::Color => parse_css_color(expected).map(format_color),
                    ValueKind::Shadow => normalize_shadow(expected),
                };
                assert_eq!(normalized.as_ref(), Some(value), "{mode:?} {}", key.key);
            }
        }
    }
}
