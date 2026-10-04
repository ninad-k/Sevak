//! The themes folder, the built-in themes and the online gallery's index.
//!
//! Nothing here touches the network: the shell downloads the bytes and this
//! module checks them ([`verify_download`]) before anything is written. A
//! theme is only ever written as the canonical text of its validated
//! [`ThemeSpec`], never as the bytes that arrived.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::checksum::verify_sha256;
use crate::gallery_source::Pin;
use crate::safe_names::is_reserved_device_name;
use crate::theme::{load_custom_css, MAX_CUSTOM_CSS_BYTES};
use crate::theme_file::{self, builtin_by_name, builtin_themes, ParsedTheme, ThemeSpec};

/// Folder of theme files inside the config folder.
pub const THEMES_DIR: &str = "themes";
/// The gallery index inside the repository's `gallery` folder, read from the
/// tag of the running build ([`crate::gallery_source::Pin`]). Fetched only when
/// the user asks for it.
pub const GALLERY_INDEX_FILE: &str = "themes.json";
/// The gallery index version this Sevak reads. Version 2 names theme files by a
/// path relative to the release (version 1 used absolute `main` addresses).
pub const GALLERY_INDEX_VERSION: u32 = 2;
/// Largest gallery index that is accepted.
pub const MAX_INDEX_BYTES: u64 = 256 * 1024;
/// Largest theme file that is read, imported or downloaded.
pub const MAX_THEME_BYTES: u64 = MAX_CUSTOM_CSS_BYTES;
const MAX_LISTED_THEMES: usize = 200;
const MAX_GALLERY_ENTRIES: usize = 500;

/// A theme file on disk (or a built-in that would be written to one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoredTheme {
    /// `themes/Nord.toml`, the value for `appearance.theme_file`.
    pub file: String,
    pub spec: ThemeSpec,
    pub warnings: Vec<String>,
}

/// A file name (without extension) for a theme called `name`: letters, digits,
/// spaces, `-`, `_` and parentheses, so it is safe on every file system.
pub fn file_stem(name: &str) -> String {
    let kept: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '(' | ')') {
                c
            } else {
                ' '
            }
        })
        .collect();
    let mut stem = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    stem = stem.chars().take(60).collect::<String>().trim().to_owned();
    if stem.is_empty() {
        return "theme".to_owned();
    }
    // Names Windows reserves, with or without an extension.
    if is_reserved_device_name(&stem) {
        stem.push_str(" theme");
    }
    stem
}

/// `themes/<stem>.toml` for a theme called `name`.
pub fn file_for(name: &str) -> String {
    format!("{THEMES_DIR}/{}.toml", file_stem(name))
}

/// Reads and validates the theme file `file` (a path inside `config_dir`, with
/// the same rules as `custom_css`).
pub fn load(config_dir: &Path, file: &str) -> Result<ParsedTheme, String> {
    let text = load_custom_css(config_dir, file)?;
    theme_file::parse(&text)
}

/// Every valid `*.toml` in the themes folder, by name. Files that are not
/// themes are skipped (and logged).
pub fn list(config_dir: &Path) -> Vec<StoredTheme> {
    let Ok(entries) = fs::read_dir(config_dir.join(THEMES_DIR)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|kind| kind.is_file() || kind.is_symlink())
        })
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.to_ascii_lowercase().ends_with(".toml"))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.truncate(MAX_LISTED_THEMES);

    names
        .into_iter()
        .filter_map(|name| {
            let file = format!("{THEMES_DIR}/{name}");
            match load(config_dir, &file) {
                Ok(parsed) => Some(stored(file, parsed)),
                Err(reason) => {
                    tracing::warn!("themes: skipping {file}: {reason}");
                    None
                }
            }
        })
        .collect()
}

fn stored(file: String, mut parsed: ParsedTheme) -> StoredTheme {
    if parsed.spec.name.is_empty() {
        let stem = Path::new(&file)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Theme");
        parsed.spec.name = stem.to_owned();
    }
    StoredTheme {
        file,
        spec: parsed.spec,
        warnings: parsed.warnings,
    }
}

/// The built-in themes, each with the file it is written to when chosen.
pub fn builtin() -> Vec<StoredTheme> {
    builtin_themes()
        .iter()
        .map(|theme| stored(file_for(&theme.spec.name), theme.clone()))
        .collect()
}

/// Writes `bytes` to `path` through a temporary file, so a crash never leaves
/// half a theme.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("the themes folder has no parent")?;
    fs::create_dir_all(dir).map_err(|err| format!("cannot create the themes folder: {err}"))?;
    let temp = path.with_extension("toml.tmp");
    let written = fs::write(&temp, bytes).and_then(|()| fs::rename(&temp, path));
    if let Err(err) = written {
        let _ = fs::remove_file(&temp);
        return Err(format!("cannot write the theme: {err}"));
    }
    Ok(())
}

/// Saves `spec` as `themes/<name>.toml`, replacing a theme of that name. A
/// built-in theme's name is refused: the built-ins stay as they are.
pub fn save(config_dir: &Path, spec: ThemeSpec) -> Result<StoredTheme, String> {
    let name = spec.name.trim().to_owned();
    if name.is_empty() {
        return Err("Give the theme a name first.".to_owned());
    }
    if builtin_by_name(&name).is_some() {
        return Err(format!(
            "\"{name}\" is the name of a built-in theme. Choose another name."
        ));
    }
    let spec = ThemeSpec { name, ..spec };
    let file = file_for(&spec.name);
    write_atomic(&config_dir.join(&file), spec.to_toml().as_bytes())?;
    // Read back what was written: exactly what will be applied.
    let parsed = load(config_dir, &file)?;
    Ok(stored(file, parsed))
}

/// Writes the built-in theme `name` to the themes folder unless a file of that
/// name is already there (which is then used as it is). Returns that file.
pub fn install_builtin(config_dir: &Path, name: &str) -> Result<StoredTheme, String> {
    let theme =
        builtin_by_name(name).ok_or_else(|| format!("There is no built-in theme \"{name}\"."))?;
    let file = file_for(&theme.spec.name);
    if !config_dir.join(&file).exists() {
        write_atomic(&config_dir.join(&file), theme.spec.to_toml().as_bytes())?;
    }
    Ok(stored(file.clone(), load(config_dir, &file)?))
}

/// What to do when the theme being installed has the name of one that is
/// already in the themes folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Existing {
    /// Replace it (importing a file the user picked).
    Replace,
    /// Leave it alone and say so (installing from the gallery).
    Refuse,
    /// Replace it only if it is this file (`themes/Nord.toml`): the user asked
    /// to reinstall that theme, and the download must not name another one.
    ReplaceOnly(String),
}

/// Validates the text of a theme file (imported or downloaded) and writes it
/// as `themes/<name>.toml`; `existing` says what happens to a theme of that
/// name. The name is the theme's own, or `fallback_name` for a file without
/// one.
pub fn install_text(
    config_dir: &Path,
    text: &str,
    fallback_name: &str,
    existing: &Existing,
) -> Result<StoredTheme, String> {
    if text.len() as u64 > MAX_THEME_BYTES {
        return Err(format!(
            "The theme file is larger than {} KiB.",
            MAX_THEME_BYTES / 1024
        ));
    }
    let mut parsed =
        theme_file::parse(text).map_err(|reason| format!("This is not a theme file: {reason}."))?;
    if parsed.spec.name.is_empty() {
        parsed.spec.name = fallback_name.trim().chars().take(60).collect();
    }
    if parsed.spec.name.is_empty() {
        return Err("The theme has no name.".to_owned());
    }
    if parsed.spec.palettes().is_empty()
        && parsed.spec.layout == theme_file::ThemeLayout::default()
        && parsed.spec.font == theme_file::ThemeFont::default()
    {
        return Err("The file defines no colors, fonts or sizes, so it is not a theme.".to_owned());
    }
    let file = file_for(&parsed.spec.name);
    if config_dir.join(&file).symlink_metadata().is_ok() {
        match existing {
            Existing::Replace => {}
            Existing::Refuse => {
                return Err(format!(
                    "A theme named \"{}\" is already in your themes folder, so it was not \
                     replaced. Remove it first, or use Reinstall to replace it.",
                    parsed.spec.name
                ));
            }
            Existing::ReplaceOnly(expected) if file.eq_ignore_ascii_case(expected) => {}
            Existing::ReplaceOnly(_) => {
                return Err(format!(
                    "The download is a theme called \"{}\", which is not the theme being \
                     reinstalled and is already in your themes folder, so nothing was replaced.",
                    parsed.spec.name
                ));
            }
        }
    }
    write_atomic(&config_dir.join(&file), parsed.spec.to_toml().as_bytes())?;
    Ok(stored(file.clone(), load(config_dir, &file)?))
}

// ---------------------------------------------------------------------------
// Gallery
// ---------------------------------------------------------------------------

/// One community theme in `gallery/themes.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GalleryEntry {
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
    /// `light` or `dark`: only a hint for the gallery's list.
    pub mode: String,
    /// Where the theme file is downloaded from: the full address at the
    /// release the index was read from (see [`parse_index`]).
    pub url: String,
    /// Lowercase hex SHA-256 of the file at `url`.
    pub sha256: String,
}

#[derive(Deserialize)]
struct RawIndex {
    version: u32,
    themes: Vec<serde_json::Value>,
}

/// Parses the gallery index, read from the release `pin`. Each entry's `url` is
/// a path relative to the repository root at that release (`gallery/themes/
/// Nord.toml`) and is turned into the full address; an address outside the
/// release (another host, branch or tag) is refused. Entries that are not valid
/// (no name, a bad id or path, a hash that is not 64 hex digits) are left out.
pub fn parse_index(text: &str, pin: &Pin) -> Result<Vec<GalleryEntry>, String> {
    let index: RawIndex = serde_json::from_str(text)
        .map_err(|err| format!("the gallery index is not valid: {err}"))?;
    if index.version != GALLERY_INDEX_VERSION {
        return Err(format!(
            "the gallery index has version {}, but this Sevak reads version \
             {GALLERY_INDEX_VERSION}",
            index.version
        ));
    }
    let mut entries: Vec<GalleryEntry> = Vec::new();
    for raw in index.themes.into_iter().take(MAX_GALLERY_ENTRIES) {
        let text = |key: &str| {
            raw.get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
                .trim()
                .to_owned()
        };
        let entry = GalleryEntry {
            id: text("id"),
            name: text("name").chars().take(60).collect(),
            author: text("author").chars().take(60).collect(),
            description: text("description").chars().take(200).collect(),
            mode: text("mode"),
            url: text("url"),
            sha256: text("sha256").to_ascii_lowercase(),
        };
        let mut entry = entry;
        if entry.id.is_empty() {
            entry.id = slug(&entry.name);
        }
        let resolved = pin.resolve(&entry.url);
        let valid = !entry.name.is_empty()
            && valid_id(&entry.id)
            && resolved.is_ok()
            && entry.sha256.len() == 64
            && entry.sha256.chars().all(|c| c.is_ascii_hexdigit());
        if !valid {
            tracing::warn!(
                "themes: ignoring an invalid gallery entry \"{}\"",
                entry.name
            );
            continue;
        }
        if let Ok(url) = resolved {
            entry.url = url;
        }
        if entries.iter().any(|other| other.id == entry.id) {
            continue;
        }
        entries.push(entry);
    }
    Ok(entries)
}

/// A gallery id: lower case letters, digits, `-` and `_`, at most 48, and not a
/// name Windows reserves.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
        && !is_reserved_device_name(id)
}

/// An id made from a name: `Solarized Dark` becomes `solarized-dark`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(48).collect()
}

/// Checks a downloaded file against the hash the index promised (with the
/// galleries' shared [`verify_sha256`]), and returns its text. Anything else
/// (wrong hash, too large, not UTF-8) is an error and nothing may be installed.
pub fn verify_download(bytes: &[u8], expected_sha256: &str) -> Result<String, String> {
    if bytes.len() as u64 > MAX_THEME_BYTES {
        return Err(format!(
            "The download is larger than {} KiB.",
            MAX_THEME_BYTES / 1024
        ));
    }
    if verify_sha256(bytes, expected_sha256).is_err() {
        return Err(
            "The downloaded theme does not match the checksum in the gallery, so it was not installed."
                .to_owned(),
        );
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|_| "The downloaded theme is not UTF-8 text.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::sha256_hex;

    fn nord() -> &'static ParsedTheme {
        builtin_by_name("Nord").unwrap()
    }

    #[test]
    fn file_names_are_safe_everywhere() {
        assert_eq!(file_stem("Nord"), "Nord");
        assert_eq!(file_stem("  My   Theme (2) "), "My Theme (2)");
        assert_eq!(file_stem("../../etc/passwd"), "etc passwd");
        assert_eq!(file_stem("a\\b:c*d?e\"f<g>h|i"), "a b c d e f g h i");
        assert_eq!(file_stem("..."), "theme");
        assert_eq!(file_stem(""), "theme");
        assert_eq!(file_stem("con"), "con theme");
        assert_eq!(file_stem("LPT3"), "LPT3 theme");
        assert_eq!(file_stem("com0"), "com0");
        assert_eq!(file_stem(&"x".repeat(100)).chars().count(), 60);
        assert_eq!(file_for("Sevak Light"), "themes/Sevak Light.toml");
    }

    #[test]
    fn save_writes_a_theme_that_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut spec = nord().spec.clone();
        spec.name = "My Nord".to_owned();
        spec.layout.radius = Some(6);
        let saved = save(dir.path(), spec.clone()).unwrap();
        assert_eq!(saved.file, "themes/My Nord.toml");
        assert_eq!(saved.spec, spec);
        assert!(saved.warnings.is_empty());
        assert_eq!(list(dir.path()), vec![saved]);
        assert_eq!(load(dir.path(), "themes/My Nord.toml").unwrap().spec, spec);
    }

    #[test]
    fn save_refuses_empty_and_built_in_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut spec = nord().spec.clone();
        spec.name = "  ".to_owned();
        assert!(save(dir.path(), spec.clone()).is_err());
        spec.name = "nord".to_owned();
        let err = save(dir.path(), spec).unwrap_err();
        assert!(err.contains("built-in"), "{err}");
        assert!(list(dir.path()).is_empty());
    }

    #[test]
    fn built_ins_are_written_once_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let first = install_builtin(dir.path(), "dracula").unwrap();
        assert_eq!(first.file, "themes/Dracula.toml");
        assert_eq!(first.spec, builtin_by_name("Dracula").unwrap().spec);

        std::fs::write(
            dir.path().join(&first.file),
            "name = \"Dracula\"\n[dark]\nbackground = \"#000\"\n",
        )
        .unwrap();
        let second = install_builtin(dir.path(), "Dracula").unwrap();
        assert_eq!(second.spec.dark.as_ref().unwrap()["background"], "#000000");
        assert!(install_builtin(dir.path(), "No such theme").is_err());
        assert_eq!(builtin().len(), 8);
    }

    #[test]
    fn listing_skips_what_is_not_a_theme() {
        let dir = tempfile::tempdir().unwrap();
        let themes = dir.path().join(THEMES_DIR);
        std::fs::create_dir(&themes).unwrap();
        std::fs::write(
            themes.join("b.toml"),
            "name = \"B\"\n[dark]\ntext = \"#fff\"\n",
        )
        .unwrap();
        std::fs::write(themes.join("a.toml"), "[light]\ntext = \"#000\"\n").unwrap();
        std::fs::write(themes.join("broken.toml"), "name = ").unwrap();
        std::fs::write(themes.join("notes.txt"), "hello").unwrap();
        std::fs::write(
            themes.join("big.toml"),
            vec![b'#'; MAX_THEME_BYTES as usize + 1],
        )
        .unwrap();
        std::fs::create_dir(themes.join("dir.toml")).unwrap();

        let listed = list(dir.path());
        let names: Vec<_> = listed
            .iter()
            .map(|t| (t.file.as_str(), t.spec.name.as_str()))
            .collect();
        // A file without a name is called after its file.
        assert_eq!(names, [("themes/a.toml", "a"), ("themes/b.toml", "B")]);
        assert!(list(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn imported_text_is_validated_and_rewritten_in_canonical_form() {
        let dir = tempfile::tempdir().unwrap();
        let text = "name = \"Imported\"\n[dark]\nbackground = \"#ABC\"\ntext = \"rubbish\"\nevil = \"x\"\n";
        let stored = install_text(dir.path(), text, "ignored", &Existing::Replace).unwrap();
        assert_eq!(stored.file, "themes/Imported.toml");
        assert_eq!(stored.spec.dark.as_ref().unwrap().len(), 1);
        let on_disk = std::fs::read_to_string(dir.path().join(&stored.file)).unwrap();
        assert!(on_disk.contains("background = \"#aabbcc\""), "{on_disk}");
        assert!(!on_disk.contains("rubbish"));

        let unnamed = install_text(
            dir.path(),
            "[light]\ntext = \"#111\"\n",
            "From File",
            &Existing::Replace,
        )
        .unwrap();
        assert_eq!(unnamed.spec.name, "From File");

        let install =
            |text: &str, name: &str| install_text(dir.path(), text, name, &Existing::Replace);
        assert!(install("name = ", "x").is_err());
        assert!(install("name = \"Empty\"\n", "x").is_err());
        assert!(install("[light]\ntext = \"#111\"\n", " ").is_err());
        let huge = format!("#{}", "a".repeat(MAX_THEME_BYTES as usize));
        assert!(install(&huge, "x").is_err());
    }

    const MINE: &str = "name = \"Mine\"\n[dark]\ntext = \"#111111\"\n";
    const THEIRS: &str = "name = \"Mine\"\n[dark]\ntext = \"#222222\"\n";

    #[test]
    fn a_gallery_install_never_replaces_a_theme_of_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let first = install_text(dir.path(), MINE, "x", &Existing::Refuse).unwrap();
        assert_eq!(first.file, "themes/Mine.toml");
        let before = std::fs::read(dir.path().join(&first.file)).unwrap();

        let err = install_text(dir.path(), THEIRS, "x", &Existing::Refuse).unwrap_err();
        assert!(err.contains("already in your themes folder"), "{err}");
        assert!(err.contains("Mine"), "{err}");
        assert_eq!(std::fs::read(dir.path().join(&first.file)).unwrap(), before);
        // Not even by another spelling of the name on a case-insensitive disk,
        // and no temporary file is left behind.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path().join(THEMES_DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "{leftovers:?}");
    }

    #[test]
    fn reinstalling_replaces_only_the_theme_that_was_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        install_text(dir.path(), MINE, "x", &Existing::Refuse).unwrap();
        let other = "name = \"Other\"\n[dark]\ntext = \"#333333\"\n";
        install_text(dir.path(), other, "x", &Existing::Refuse).unwrap();
        let other_before = std::fs::read(dir.path().join("themes/Other.toml")).unwrap();

        // The download turns out to be a different theme than the one the user
        // is reinstalling: refused.
        let wrong = Existing::ReplaceOnly("themes/Mine.toml".to_owned());
        let err = install_text(dir.path(), other, "x", &wrong).unwrap_err();
        assert!(err.contains("not the theme being reinstalled"), "{err}");
        assert_eq!(
            std::fs::read(dir.path().join("themes/Other.toml")).unwrap(),
            other_before
        );

        // The right one is replaced.
        let stored = install_text(dir.path(), THEIRS, "x", &wrong).unwrap();
        assert_eq!(stored.spec.dark.as_ref().unwrap()["text"], "#222222");

        // Importing a file the user picked still replaces, as before.
        let stored = install_text(dir.path(), MINE, "x", &Existing::Replace).unwrap();
        assert_eq!(stored.spec.dark.as_ref().unwrap()["text"], "#111111");
    }

    #[test]
    fn a_download_must_match_its_checksum() {
        let bytes = b"name = \"X\"\n";
        let good = sha256_hex(bytes);
        assert_eq!(verify_download(bytes, &good).unwrap(), "name = \"X\"\n");
        assert_eq!(
            verify_download(bytes, &good.to_uppercase()).unwrap(),
            "name = \"X\"\n"
        );
        let err = verify_download(b"name = \"Y\"\n", &good).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(verify_download(bytes, "").is_err());
        assert!(verify_download(bytes, &good[..63]).is_err());

        let big = vec![b'a'; MAX_THEME_BYTES as usize + 1];
        assert!(verify_download(&big, &sha256_hex(&big)).is_err());
        let binary = [0xff, 0xfe];
        assert!(verify_download(&binary, &sha256_hex(&binary)).is_err());
    }

    fn pin() -> Pin {
        Pin::new("v1.2.3").unwrap()
    }

    fn entry_json(name: &str, url: &str, sha: &str) -> String {
        format!(r#"{{"name":"{name}","url":"{url}","sha256":"{sha}"}}"#)
    }

    #[test]
    fn the_gallery_index_keeps_only_entries_inside_the_release() {
        let sha = "a".repeat(64);
        let raw = "https://raw.githubusercontent.com/ninad-k/Sevak";
        let index = format!(
            r#"{{"version":2,"themes":[{},{},{},{},{},{},{},{},{},{},"nonsense",{}]}}"#,
            entry_json("Good", "gallery/themes/Good.toml", &sha),
            entry_json("Plain http", "http://example.com/a.toml", &sha),
            entry_json("File URL", "file:///etc/passwd", &sha),
            entry_json("Other host", "https://example.com/b.toml", &sha),
            entry_json(
                "Main branch",
                &format!("{raw}/main/gallery/themes/m.toml"),
                &sha
            ),
            entry_json(
                "Other tag",
                &format!("{raw}/v1.2.4/gallery/themes/m.toml"),
                &sha
            ),
            entry_json("Dots", "gallery/../../main/x.toml", &sha),
            entry_json("Short hash", "gallery/themes/b.toml", "abc"),
            entry_json("Not hex", "gallery/themes/c.toml", &"z".repeat(64)),
            entry_json("", "gallery/themes/d.toml", &sha),
            entry_json("Good", "gallery/themes/dup.toml", &sha.to_uppercase()),
        );
        let entries = parse_index(&index, &pin()).unwrap();
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0].name, "Good");
        assert_eq!(entries[0].id, "good");
        assert_eq!(entries[0].sha256, sha);
        assert_eq!(
            entries[0].url,
            "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/themes/Good.toml"
        );
    }

    #[test]
    fn gallery_ids_are_plain_and_not_device_names() {
        let sha = "b".repeat(64);
        let with_id = |id: &str, name: &str| {
            format!(
                r#"{{"id":"{id}","name":"{name}","url":"gallery/themes/x.toml","sha256":"{sha}"}}"#
            )
        };
        let index = format!(
            r#"{{"version":2,"themes":[{},{},{},{},{},{},{}]}}"#,
            with_id("fine_id-2", "A"),
            with_id("Upper", "B"),
            with_id("has space", "C"),
            with_id("../x", "D"),
            with_id("nul", "E"),
            with_id("com1", "F"),
            // No id: made from the name, which may itself be a device name.
            with_id("", "CON"),
        );
        let entries = parse_index(&index, &pin()).unwrap();
        let ids: Vec<_> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["fine_id-2"]);
        // A name that is not usable as an id on its own gets a plain one.
        let named = parse_index(
            &format!(
                r#"{{"version":2,"themes":[{}]}}"#,
                entry_json("Solarized (Dark)", "gallery/x.toml", &sha)
            ),
            &pin(),
        )
        .unwrap();
        assert_eq!(named[0].id, "solarized-dark");
    }

    #[test]
    fn the_gallery_index_must_be_version_two_json() {
        assert!(parse_index("not json", &pin()).is_err());
        let err = parse_index(r#"{"version":1,"themes":[]}"#, &pin()).unwrap_err();
        assert!(err.contains("version 1"), "{err}");
        assert!(parse_index(r#"{"version":3,"themes":[]}"#, &pin()).is_err());
        assert!(parse_index(r#"{"themes":[]}"#, &pin()).is_err());
        assert_eq!(
            parse_index(r#"{"version":2,"themes":[]}"#, &pin()).unwrap(),
            vec![]
        );
    }

    #[test]
    fn theme_names_that_windows_reserves_get_a_safe_file() {
        for name in ["CON", "nul", "Com1", "LPT9", "COM\u{b9}", "aux"] {
            let stem = file_stem(name);
            assert!(stem.ends_with(" theme"), "{name:?} -> {stem:?}");
            assert!(!is_reserved_device_name(&stem), "{stem:?}");
        }
        // The extension does not save it either: dots become spaces.
        assert_eq!(file_stem("nul.txt"), "nul txt");
        assert_eq!(file_stem("Console"), "Console");
    }

    /// `ui/src/lib/builtin-themes.json` is what the browser preview of the
    /// editor (`npm run dev`, no backend) shows as the built-in themes. Update
    /// it with `SEVAK_UPDATE_FIXTURES=1 cargo test -p sevak-core ui_fixture`.
    #[test]
    fn ui_fixture_matches_the_built_in_themes() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/lib/builtin-themes.json");
        let current = serde_json::to_value(builtin()).unwrap();
        if std::env::var_os("SEVAK_UPDATE_FIXTURES").is_some() {
            let text = serde_json::to_string_pretty(&current).unwrap() + "\n";
            std::fs::write(&path, text).unwrap();
            return;
        }
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            on_disk, current,
            "regenerate with SEVAK_UPDATE_FIXTURES=1 cargo test -p sevak-core ui_fixture"
        );
    }

    /// `gallery/themes.json` in the repository lists the built-in themes with
    /// the real hashes of the files next to it.
    #[test]
    fn the_shipped_gallery_matches_the_theme_files() {
        let gallery = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gallery");
        let index = std::fs::read_to_string(gallery.join("themes.json")).unwrap();
        let entries = parse_index(&index, &pin()).unwrap();
        assert!(entries.len() >= 8);
        let raw_url = "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/themes/";
        for theme in builtin_themes() {
            let entry = entries
                .iter()
                .find(|e| e.name == theme.spec.name)
                .unwrap_or_else(|| panic!("{} is not in the gallery", theme.spec.name));
            let file = entry
                .url
                .strip_prefix(raw_url)
                .expect("a file in gallery/themes");
            let bytes = std::fs::read(gallery.join("themes").join(file)).unwrap();
            assert_eq!(entry.sha256, sha256_hex(&bytes), "{}", entry.name);
            // What the gallery serves is exactly the built-in.
            let text = verify_download(&bytes, &entry.sha256).unwrap();
            assert_eq!(theme_file::parse(&text).unwrap().spec, theme.spec);
        }
        // Every entry, community ones included, must be a valid theme file in
        // this repository.
        for entry in &entries {
            let file = entry
                .url
                .strip_prefix(raw_url)
                .unwrap_or_else(|| panic!("{} is not in gallery/themes", entry.url));
            let bytes = std::fs::read(gallery.join("themes").join(file)).unwrap();
            assert_eq!(entry.sha256, sha256_hex(&bytes), "{}", entry.name);
            assert!(theme_file::parse(&String::from_utf8(bytes).unwrap())
                .unwrap()
                .warnings
                .is_empty());
        }
    }

    /// Every theme in the gallery, built-in or not: the index and the file agree,
    /// the colors validate and reach WCAG AA for body text, nothing is listed
    /// twice and no theme file is left out of the index.
    #[test]
    fn every_gallery_theme_is_valid_accessible_and_listed_once() {
        use crate::theme_file::{contrast_checks, Mode};

        let gallery = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gallery");
        let text = fs::read_to_string(gallery.join("themes.json")).unwrap();
        let raw: serde_json::Value = serde_json::from_str(&text).unwrap();
        let listed = raw["themes"].as_array().unwrap().len();
        let entries = parse_index(&text, &pin()).unwrap();
        assert_eq!(entries.len(), listed, "an entry of themes.json is invalid");
        assert!(entries.len() >= 19, "the gallery lost themes");

        // The shipped paths are relative to the repository root; the pin joins
        // them to a release.
        let raw_url = "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/gallery/themes/";
        let mut files = Vec::new();
        let mut names = std::collections::HashSet::new();
        for entry in &entries {
            let file = entry
                .url
                .strip_prefix(raw_url)
                .unwrap_or_else(|| panic!("{}: the url must be gallery/themes/<file>", entry.id));
            assert!(
                file.ends_with(".toml") && !file.contains('/') && !file.contains('?'),
                "{}: {file}",
                entry.id
            );
            assert!(
                names.insert(entry.name.to_lowercase()),
                "{} twice",
                entry.name
            );
            let bytes = fs::read(gallery.join("themes").join(file))
                .unwrap_or_else(|err| panic!("{}: {file}: {err}", entry.id));
            assert!(
                (bytes.len() as u64) <= MAX_THEME_BYTES,
                "{}: the file is too large",
                entry.id
            );
            assert_eq!(entry.sha256, sha256_hex(&bytes), "{}: hash", entry.id);
            let text = String::from_utf8(bytes).unwrap();
            assert!(!text.contains('\r'), "{file} must use LF line endings");
            let parsed = theme_file::parse(&text).unwrap();
            assert!(parsed.warnings.is_empty(), "{file}: {:?}", parsed.warnings);

            let spec = &parsed.spec;
            assert_eq!(spec.name, entry.name, "{file}: name");
            assert_eq!(spec.author, entry.author, "{file}: author");
            assert_eq!(spec.description, entry.description, "{file}: description");
            assert!(!entry.description.is_empty() && entry.description.chars().count() <= 200);
            let palettes = spec.palettes();
            assert!(!palettes.is_empty(), "{file}: no palette");
            let mode = match palettes.as_slice() {
                [(Mode::Light, _)] => "light",
                [(Mode::Dark, _)] => "dark",
                _ => "",
            };
            assert_eq!(
                entry.mode, mode,
                "{file}: `mode` must say which single palette it has"
            );
            for (mode, palette) in palettes {
                for check in contrast_checks(palette, mode) {
                    assert!(
                        check.aa,
                        "{file}: {} is {:.2}:1, below AA",
                        check.label, check.ratio
                    );
                }
            }
            // The text the app installs is this theme in canonical form: the
            // file, minus leading `#` comments, already is that.
            let body: String = text
                .lines()
                .skip_while(|line| line.starts_with('#'))
                .map(|line| format!("{line}\n"))
                .collect();
            assert_eq!(body, spec.to_toml(), "{file} is not in canonical form");
            // A theme that is not built in must say where its palette is from.
            if builtin_by_name(&spec.name).is_none() {
                let first = text.lines().next().unwrap_or_default();
                assert!(
                    first.starts_with('#')
                        && (first.contains("MIT") || first.contains("Apache-2.0")),
                    "{file}: start the file with a comment naming the palette and its licence"
                );
            }
            files.push(file.to_owned());
        }
        // No theme file may sit in the folder without an entry.
        for item in fs::read_dir(gallery.join("themes")).unwrap() {
            let name = item.unwrap().file_name().to_string_lossy().into_owned();
            assert!(files.contains(&name), "{name} is not listed in themes.json");
        }
    }
}
