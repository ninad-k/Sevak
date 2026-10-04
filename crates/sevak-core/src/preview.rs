//! What the preview pane (Shift / Ctrl+Y) and the Text View (Ctrl+T) show.
//!
//! The shell calls [`produce`] on a worker thread, when the user asks, for the
//! one result they have selected. Everything here is bounded and defensive:
//!
//! - **Only what the result refers to is read.** The path comes from the
//!   result's own preview hint or action, never from the UI. Relative paths,
//!   network locations (`\\server\share`: opening one would be a network
//!   request) and anything that is not a regular file or folder are refused.
//! - **Sizes are capped.** Text previews read at most [`MAX_TEXT_BYTES`],
//!   images at most [`MAX_IMAGE_BYTES`] (they travel to the page as a `data:`
//!   URL, so the page's content security policy needs no new rule), folder
//!   listings at most [`MAX_FOLDER_ENTRIES`] names.
//! - **No network.** A link shows its address, title and host; nothing is
//!   fetched.
//!
//! Failures never panic or error: a file that cannot be read becomes a
//! [`PreviewContent`] with a `note` saying why.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::model::{Action, ClipContent, LaunchTarget, PreviewHint, ResultItem, ViewHint};

/// Most bytes of a text file shown in the pane.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
/// Most bytes of text shown in the Text View.
pub const MAX_VIEW_BYTES: usize = 512 * 1024;
/// What an encrypted image file may be larger than its picture.
const SEALED_IMAGE_OVERHEAD: usize = 4096;

/// Largest image (file size) sent to the page.
pub const MAX_IMAGE_BYTES: u64 = 4 * 1024 * 1024;
/// Most names listed for a folder.
pub const MAX_FOLDER_ENTRIES: usize = 100;
/// A folder with more entries than this is listed from its first entries only.
const FOLDER_SCAN_LIMIT: usize = 5_000;
/// How much of a file is looked at to tell text from binary data.
const SNIFF_BYTES: usize = 8 * 1024;
/// Rows kept from a `Details` hint, and characters kept per value.
const MAX_DETAIL_ROWS: usize = 24;
const MAX_DETAIL_CHARS: usize = 500;
/// Plain text of this many characters (or any text with a line break) can be
/// opened in the Text View.
pub const TEXT_VIEW_MIN_CHARS: usize = 160;
/// Largest `Info.plist` read for an application's version.
const MAX_PLIST_BYTES: u64 = 256 * 1024;

/// One labelled fact in the pane's details table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetaRow {
    pub label: String,
    pub value: String,
}

impl MetaRow {
    fn new(label: &str, value: impl Into<String>) -> Self {
        Self {
            label: label.to_owned(),
            value: value.into(),
        }
    }
}

/// An entry of a folder listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FolderEntry {
    pub name: String,
    pub dir: bool,
}

/// The main content of the pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviewBody {
    /// Only the details table and the note, if any.
    None,
    /// Text for a monospace view. `truncated`: there is more than is shown.
    Text { text: String, truncated: bool },
    /// A picture as a `data:` URL.
    Image { src: String },
    /// A folder's children, directories first.
    Folder {
        entries: Vec<FolderEntry>,
        truncated: bool,
    },
    /// A link's address.
    Url { url: String },
}

/// Everything the pane draws for one result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewContent {
    pub title: String,
    pub subtitle: String,
    pub body: PreviewBody,
    pub meta: Vec<MetaRow>,
    /// Seconds since the unix epoch; the page formats it in the user's locale.
    pub modified: Option<u64>,
    /// Why there is no (or only a partial) preview: "Too large to preview".
    pub note: Option<String>,
}

impl PreviewContent {
    fn new(item: &ResultItem) -> Self {
        Self {
            title: item.title.clone(),
            subtitle: item.subtitle.clone(),
            body: PreviewBody::None,
            meta: Vec::new(),
            modified: None,
            note: None,
        }
    }
}

/// The full text for the Text View.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextViewContent {
    pub title: String,
    pub text: String,
    pub truncated: bool,
}

/// What a result is a preview *of*, once the hints are resolved.
enum Subject<'a> {
    Hint(PreviewHint),
    App(&'a LaunchTarget),
    Nothing,
}

/// The preview of `item`. `hint` is what its plugin says about it when asked
/// ([`crate::Plugin::preview`]); it wins over the hint carried by the item,
/// which wins over what the item's action refers to.
pub fn produce(item: &ResultItem, hint: Option<PreviewHint>) -> PreviewContent {
    let mut content = PreviewContent::new(item);
    match subject(item, hint) {
        Subject::Hint(PreviewHint::Text { text }) => {
            let (text, truncated) = cut(&text, MAX_TEXT_BYTES);
            content.body = PreviewBody::Text { text, truncated };
        }
        Subject::Hint(PreviewHint::Path { path }) => path_content(&mut content, &path),
        Subject::Hint(PreviewHint::Url { url, title }) => {
            url_content(&mut content, &url, title.as_deref());
        }
        Subject::Hint(PreviewHint::Details { rows }) => {
            content.meta = rows
                .into_iter()
                .take(MAX_DETAIL_ROWS)
                .map(|(label, value)| MetaRow::new(&label, cut(&value, MAX_DETAIL_CHARS).0))
                .collect();
        }
        Subject::App(target) => app_content(&mut content, target),
        Subject::Nothing => {}
    }
    content
}

fn subject(item: &ResultItem, hint: Option<PreviewHint>) -> Subject<'_> {
    if let Some(hint) = hint.or_else(|| item.preview.clone()) {
        return Subject::Hint(hint);
    }
    if let Some(ViewHint::Text { text, .. }) = &item.view {
        return Subject::Hint(PreviewHint::Text { text: text.clone() });
    }
    match &item.action {
        Action::OpenPath { path } | Action::RevealPath { path } => {
            Subject::Hint(PreviewHint::Path { path: path.clone() })
        }
        Action::Launch { target } | Action::RunAsAdmin { target } => Subject::App(target),
        Action::OpenUrl { url } => Subject::Hint(PreviewHint::Url {
            url: url.clone(),
            title: Some(item.title.clone()),
        }),
        Action::CopyText { text } | Action::PasteText { text, .. } => {
            Subject::Hint(PreviewHint::Text { text: text.clone() })
        }
        // A clipboard image or a single copied file previews like the file it
        // is; several files list their paths.
        Action::PasteClip { content, .. } | Action::CopyClip { content } => match content {
            ClipContent::Image { path } => Subject::Hint(PreviewHint::Path { path: path.clone() }),
            ClipContent::Files { paths } => match paths.as_slice() {
                [] => Subject::Nothing,
                [one] => Subject::Hint(PreviewHint::Path { path: one.clone() }),
                _ => content.copy_text().map_or(Subject::Nothing, |text| {
                    Subject::Hint(PreviewHint::Text { text })
                }),
            },
        },
        Action::Custom { .. } => Subject::Nothing,
    }
}

/// Whether `item` can be opened in the Text View: it carries a text view, or
/// the text it copies or pastes is long or has several lines.
pub fn has_text_view(item: &ResultItem) -> bool {
    if matches!(item.view, Some(ViewHint::Text { .. })) {
        return true;
    }
    match &item.action {
        Action::CopyText { text } | Action::PasteText { text, .. } => {
            text.contains('\n')
                || text.chars().take(TEXT_VIEW_MIN_CHARS).count() >= TEXT_VIEW_MIN_CHARS
        }
        _ => false,
    }
}

/// The text the Text View shows for `item`: the plugin's own account of it
/// (`hint`, e.g. a snippet with its placeholders filled in), else the item's
/// text view, else the text its action copies or pastes.
pub fn text_view(item: &ResultItem, hint: Option<PreviewHint>) -> Option<TextViewContent> {
    let text = match hint {
        Some(PreviewHint::Text { text }) => text,
        _ => match (&item.view, &item.action) {
            (Some(ViewHint::Text { text, .. }), _) => text.clone(),
            (_, Action::CopyText { text } | Action::PasteText { text, .. }) => text.clone(),
            _ => return None,
        },
    };
    let (text, truncated) = cut(&text, MAX_VIEW_BYTES);
    Some(TextViewContent {
        title: item.title.clone(),
        text,
        truncated,
    })
}

// ---- files and folders ---------------------------------------------------

fn path_content(content: &mut PreviewContent, path: &Path) {
    if let Some(reason) = refuse(path) {
        content.note = Some(reason.to_owned());
        return;
    }
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) => {
            content.note = Some(match err.kind() {
                std::io::ErrorKind::NotFound => "This item no longer exists".to_owned(),
                std::io::ErrorKind::PermissionDenied => {
                    "No permission to read this item".to_owned()
                }
                _ => "This item cannot be read".to_owned(),
            });
            return;
        }
    };
    content
        .meta
        .push(MetaRow::new("Path", path.to_string_lossy()));
    content.modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|age| age.as_secs());

    if metadata.is_dir() {
        folder_content(content, path);
    } else if metadata.is_file() {
        file_content(content, path, metadata.len());
    } else {
        content.note = Some("Not a regular file".to_owned());
    }
}

/// Why `path` must not be read for a preview, if so.
fn refuse(path: &Path) -> Option<&'static str> {
    if is_network_path(&path.to_string_lossy()) {
        return Some("Network locations are not previewed");
    }
    if !path.is_absolute() {
        return Some("Only absolute paths are previewed");
    }
    None
}

/// A UNC path (`\\server\share`, `//server/share`, `\\?\UNC\server\share`):
/// opening it makes the system contact another machine. Local device paths
/// (`\\?\C:\`, `\\.\`) are not network paths.
pub fn is_network_path(path: &str) -> bool {
    let unified = path.replace('/', "\\");
    if let Some(rest) = unified.strip_prefix("\\\\") {
        let device = rest.starts_with("?\\") || rest.starts_with(".\\");
        if device {
            return rest
                .get(2..6)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("UNC\\"));
        }
        return true;
    }
    false
}

fn folder_content(content: &mut PreviewContent, path: &Path) {
    content.meta.insert(0, MetaRow::new("Kind", "Folder"));
    let reader = match fs::read_dir(path) {
        Ok(reader) => reader,
        Err(_) => {
            content.note = Some("This folder cannot be read".to_owned());
            return;
        }
    };
    let mut entries: Vec<FolderEntry> = Vec::new();
    let mut scanned = 0;
    let mut cut_short = false;
    for entry in reader.flatten() {
        if scanned >= FOLDER_SCAN_LIMIT {
            cut_short = true;
            break;
        }
        scanned += 1;
        let dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        entries.push(FolderEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            dir,
        });
    }
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let count = if cut_short {
        format!("more than {scanned}")
    } else {
        scanned.to_string()
    };
    content.meta.insert(1, MetaRow::new("Items", count));
    let truncated = entries.len() > MAX_FOLDER_ENTRIES;
    entries.truncate(MAX_FOLDER_ENTRIES);
    content.body = PreviewBody::Folder { entries, truncated };
}

fn file_content(content: &mut PreviewContent, path: &Path, size: u64) {
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    content
        .meta
        .insert(0, MetaRow::new("Kind", describe_extension(&extension)));
    content
        .meta
        .insert(1, MetaRow::new("Size", format_size(size)));

    if let Some(mime) = image_mime(&extension) {
        if size > MAX_IMAGE_BYTES {
            content.note = Some(format!(
                "Too large to preview (limit {})",
                format_size(MAX_IMAGE_BYTES)
            ));
            return;
        }
        // An image of the clipboard history can be encrypted: a little more is
        // read, for the encryption's own overhead.
        let read = read_limited(path, MAX_IMAGE_BYTES as usize + SEALED_IMAGE_OVERHEAD)
            .and_then(crate::sealed::open_global);
        match read {
            Ok(bytes) if !bytes.is_empty() && bytes.len() as u64 <= MAX_IMAGE_BYTES => {
                content.body = PreviewBody::Image {
                    src: format!("data:{mime};base64,{}", base64(&bytes)),
                };
            }
            _ => content.note = Some("This image cannot be read".to_owned()),
        }
        return;
    }
    if extension == "pdf" {
        content.note = Some("PDF pages are not previewed; Enter opens the file".to_owned());
        return;
    }

    match read_limited(path, MAX_TEXT_BYTES + 1) {
        Ok(bytes) => match decode_text(&bytes, MAX_TEXT_BYTES) {
            Some((text, truncated)) => content.body = PreviewBody::Text { text, truncated },
            None => content.note = Some("No preview for this kind of file".to_owned()),
        },
        Err(_) => content.note = Some("This file cannot be read".to_owned()),
    }
}

/// Reads at most `limit` bytes of the file.
fn read_limited(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The text of `bytes` (the start of a file), or `None` when they look like
/// binary data. At most `max` bytes are kept, cut at a character boundary;
/// the flag says whether more was left out.
fn decode_text(bytes: &[u8], max: usize) -> Option<(String, bool)> {
    let sample = &bytes[..bytes.len().min(SNIFF_BYTES)];
    if sample.contains(&0) {
        return None;
    }
    let truncated = bytes.len() > max;
    let kept = &bytes[..bytes.len().min(max)];
    let text = match std::str::from_utf8(kept) {
        Ok(text) => text.to_owned(),
        // The cut may split the last character; that is not an invalid file.
        Err(err) if err.error_len().is_none() => {
            String::from_utf8_lossy(&kept[..err.valid_up_to()]).into_owned()
        }
        Err(_) => {
            // Other encodings (Latin-1): fine if only a tenth is damaged, else binary.
            let lossy = String::from_utf8_lossy(kept).into_owned();
            let damaged = lossy.chars().filter(|&c| c == '\u{fffd}').count();
            if damaged * 10 > lossy.chars().count() {
                return None;
            }
            lossy
        }
    };
    Some((text.trim_start_matches('\u{feff}').to_owned(), truncated))
}

/// `text` cut to at most `max` bytes at a character boundary, and whether
/// anything was cut.
fn cut(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_owned(), false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

fn image_mime(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        _ => return None,
    })
}

fn describe_extension(extension: &str) -> String {
    let known = match extension {
        "" => return "File".to_owned(),
        "txt" => "Text document",
        "md" | "markdown" => "Markdown document",
        "pdf" => "PDF document",
        "png" => "PNG image",
        "jpg" | "jpeg" => "JPEG image",
        "gif" => "GIF image",
        "webp" => "WebP image",
        "svg" => "SVG image",
        "bmp" => "Bitmap image",
        "ico" => "Icon",
        "json" => "JSON file",
        "toml" => "TOML file",
        "yaml" | "yml" => "YAML file",
        "csv" => "CSV file",
        "html" | "htm" => "HTML document",
        "css" => "Stylesheet",
        "js" | "mjs" => "JavaScript file",
        "ts" => "TypeScript file",
        "rs" => "Rust source",
        "py" => "Python script",
        "sh" => "Shell script",
        "zip" => "ZIP archive",
        "docx" | "doc" => "Word document",
        "xlsx" | "xls" => "Excel workbook",
        "pptx" | "ppt" => "PowerPoint presentation",
        "exe" => "Application",
        "mp3" | "wav" | "flac" | "m4a" => "Audio file",
        "mp4" | "mkv" | "mov" | "avi" | "webm" => "Video file",
        _ => return format!("{} file", extension.to_uppercase()),
    };
    known.to_owned()
}

/// `1536` as `1.5 KB`.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

// ---- links and applications ------------------------------------------------

fn url_content(content: &mut PreviewContent, url: &str, title: Option<&str>) {
    if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
        content.title = title.to_owned();
    }
    if let Some(host) = url_host(url) {
        content.meta.push(MetaRow::new("Site", host));
    }
    content.body = PreviewBody::Url {
        url: cut(url, MAX_DETAIL_CHARS * 4).0,
    };
}

/// The host of an `http(s)`-style URL, without credentials or port.
fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next()?,
        None => host.split(':').next()?,
    };
    (!host.is_empty()).then(|| host.to_owned())
}

fn app_content(content: &mut PreviewContent, target: &LaunchTarget) {
    content.meta.push(MetaRow::new("Kind", "Application"));
    match target {
        LaunchTarget::Shortcut { path } => {
            content
                .meta
                .push(MetaRow::new("Shortcut", path.to_string_lossy()));
        }
        LaunchTarget::PackagedApp { app_user_model_id } => {
            content.meta[0].value = "Store app".to_owned();
            content
                .meta
                .push(MetaRow::new("App ID", app_user_model_id.as_str()));
        }
        LaunchTarget::DesktopEntry {
            path,
            exec,
            terminal,
            ..
        } => {
            content
                .meta
                .push(MetaRow::new("Entry", path.to_string_lossy()));
            content.meta.push(MetaRow::new(
                "Command",
                cut(&exec.join(" "), MAX_DETAIL_CHARS).0,
            ));
            if *terminal {
                content.meta.push(MetaRow::new("Runs in", "a terminal"));
            }
        }
        LaunchTarget::Executable { args, .. } => {
            if let Some(bundle) = target.path() {
                content
                    .meta
                    .push(MetaRow::new("Path", bundle.to_string_lossy()));
                if let Some(version) = bundle_version(bundle) {
                    content.meta.push(MetaRow::new("Version", version));
                }
            }
            // `open -a <bundle>` is how macOS apps start; its arguments say nothing.
            let bundle_launch = args.len() == 2 && args[0] == "-a";
            if !args.is_empty() && !bundle_launch {
                content.meta.push(MetaRow::new(
                    "Arguments",
                    cut(&args.join(" "), MAX_DETAIL_CHARS).0,
                ));
            }
        }
    }
    if let Some(modified) = target.path().and_then(|path| fs::metadata(path).ok()) {
        content.modified = modified
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|age| age.as_secs());
    }
}

/// The version of a macOS application bundle from its XML `Info.plist`. Cheap
/// and best effort: a binary plist, or anything else unexpected, gives `None`.
pub fn bundle_version(bundle: &Path) -> Option<String> {
    let plist = bundle.join("Contents").join("Info.plist");
    let mut text = String::new();
    File::open(plist)
        .ok()?
        .take(MAX_PLIST_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    ["CFBundleShortVersionString", "CFBundleVersion"]
        .iter()
        .find_map(|key| plist_string(&text, key))
}

fn plist_string(plist: &str, key: &str) -> Option<String> {
    let after = plist.split_once(&format!("<key>{key}</key>"))?.1;
    let value = after.split_once("<string>")?.1.split_once("</string>")?.0;
    let value = value.trim();
    (!value.is_empty() && value.len() <= 64).then(|| value.to_owned())
}

// ---- encoding ---------------------------------------------------------------

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(char::from(ALPHABET[(n >> 18) as usize & 63]));
        out.push(char::from(ALPHABET[(n >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 {
            char::from(ALPHABET[(n >> 6) as usize & 63])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(ALPHABET[n as usize & 63])
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn item(action: Action) -> ResultItem {
        ResultItem::new("t", "k", "Title", action).with_subtitle("Sub")
    }

    fn open(path: &Path) -> ResultItem {
        item(Action::OpenPath {
            path: path.to_path_buf(),
        })
    }

    fn meta<'a>(content: &'a PreviewContent, label: &str) -> Option<&'a str> {
        content
            .meta
            .iter()
            .find(|row| row.label == label)
            .map(|row| row.value.as_str())
    }

    #[test]
    fn clipboard_images_and_files_preview_as_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copied.txt");
        fs::write(&path, "copied file\n").unwrap();
        let one = item(Action::CopyClip {
            content: ClipContent::Files {
                paths: vec![path.clone()],
            },
        });
        assert!(matches!(
            produce(&one, None).body,
            PreviewBody::Text { ref text, .. } if text == "copied file\n"
        ));
        let several = item(Action::PasteClip {
            content: ClipContent::Files {
                paths: vec![path.clone(), dir.path().join("b.txt")],
            },
            restore_clipboard: false,
        });
        assert!(matches!(
            subject(&several, None),
            Subject::Hint(PreviewHint::Text { ref text }) if text.lines().count() == 2
        ));
        let image = item(Action::PasteClip {
            content: ClipContent::Image { path: path.clone() },
            restore_clipboard: false,
        });
        assert!(matches!(
            subject(&image, None),
            Subject::Hint(PreviewHint::Path { path: ref p }) if *p == path
        ));
    }

    #[test]
    fn text_files_are_read_up_to_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        fs::write(&path, "hello\nworld\n").unwrap();
        let content = produce(&open(&path), None);
        assert_eq!(
            content.body,
            PreviewBody::Text {
                text: "hello\nworld\n".into(),
                truncated: false
            }
        );
        assert_eq!(meta(&content, "Kind"), Some("Text document"));
        assert_eq!(meta(&content, "Size"), Some("12 bytes"));
        assert!(content.modified.is_some());
        assert_eq!(content.note, None);
        assert_eq!(content.title, "Title");
        assert_eq!(content.subtitle, "Sub");

        let big = dir.path().join("big.log");
        fs::write(&big, "x".repeat(MAX_TEXT_BYTES * 3)).unwrap();
        let PreviewBody::Text { text, truncated } = produce(&open(&big), None).body else {
            panic!("expected text");
        };
        assert!(truncated);
        assert_eq!(text.len(), MAX_TEXT_BYTES);
    }

    #[test]
    fn a_cut_inside_a_multibyte_character_is_not_binary() {
        // 3-byte characters, cut at an offset that splits one.
        let text = "€".repeat(MAX_TEXT_BYTES);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("euros.txt");
        fs::write(&path, &text).unwrap();
        let PreviewBody::Text { text, truncated } = produce(&open(&path), None).body else {
            panic!("expected text");
        };
        assert!(truncated);
        assert!(text.len() <= MAX_TEXT_BYTES);
        assert!(text.chars().all(|c| c == '€'));
    }

    #[test]
    fn binary_files_get_details_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        fs::write(&path, [0x7f, b'E', b'L', b'F', 0, 1, 2, 3]).unwrap();
        let content = produce(&open(&path), None);
        assert_eq!(content.body, PreviewBody::None);
        assert_eq!(
            content.note.as_deref(),
            Some("No preview for this kind of file")
        );
        assert_eq!(meta(&content, "Kind"), Some("BIN file"));
    }

    #[test]
    fn latin1_text_is_shown_and_garbage_is_not() {
        assert_eq!(
            decode_text(b"caf\xe9 au lait, merci beaucoup pour tout ce temps", 100)
                .map(|(t, _)| t.contains("caf")),
            Some(true)
        );
        let garbage: Vec<u8> = (0x80..0xff).collect();
        assert_eq!(decode_text(&garbage, 1000), None);
        assert_eq!(decode_text(b"a\0b", 10), None);
        // A byte order mark is not part of the text.
        assert_eq!(decode_text("\u{feff}hi".as_bytes(), 10).unwrap().0, "hi");
    }

    #[test]
    fn images_become_data_urls_within_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dot.png");
        fs::write(&path, [0x89, b'P', b'N', b'G']).unwrap();
        let content = produce(&open(&path), None);
        assert_eq!(
            content.body,
            PreviewBody::Image {
                src: "data:image/png;base64,iVBORw==".into()
            }
        );
        assert_eq!(meta(&content, "Kind"), Some("PNG image"));

        let svg = dir.path().join("a.SVG");
        fs::write(&svg, "<svg/>").unwrap();
        let PreviewBody::Image { src } = produce(&open(&svg), None).body else {
            panic!("expected an image");
        };
        assert!(src.starts_with("data:image/svg+xml;base64,"));

        let huge = dir.path().join("huge.jpg");
        let file = File::create(&huge).unwrap();
        file.set_len(MAX_IMAGE_BYTES + 1).unwrap();
        let content = produce(&open(&huge), None);
        assert_eq!(content.body, PreviewBody::None);
        assert!(content.note.unwrap().starts_with("Too large to preview"));
    }

    #[test]
    fn pdfs_show_details_not_pages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paper.pdf");
        fs::write(&path, "%PDF-1.7 not really").unwrap();
        let content = produce(&open(&path), None);
        assert_eq!(content.body, PreviewBody::None);
        assert_eq!(meta(&content, "Kind"), Some("PDF document"));
        assert!(content.note.unwrap().contains("PDF"));
    }

    #[test]
    fn folders_list_directories_first_and_are_capped() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("zeta")).unwrap();
        fs::write(dir.path().join("Beta.txt"), "b").unwrap();
        fs::write(dir.path().join("alpha.txt"), "a").unwrap();
        let content = produce(&open(dir.path()), None);
        let PreviewBody::Folder { entries, truncated } = &content.body else {
            panic!("expected a folder");
        };
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["zeta", "alpha.txt", "Beta.txt"]);
        assert!(entries[0].dir && !entries[1].dir);
        assert!(!truncated);
        assert_eq!(meta(&content, "Kind"), Some("Folder"));
        assert_eq!(meta(&content, "Items"), Some("3"));

        let many = tempfile::tempdir().unwrap();
        for n in 0..MAX_FOLDER_ENTRIES + 20 {
            fs::write(many.path().join(format!("f{n:03}")), "").unwrap();
        }
        let PreviewBody::Folder { entries, truncated } = produce(&open(many.path()), None).body
        else {
            panic!("expected a folder");
        };
        assert!(truncated);
        assert_eq!(entries.len(), MAX_FOLDER_ENTRIES);
    }

    #[test]
    fn unusable_paths_are_refused_with_a_note() {
        let missing = produce(
            &open(&std::env::temp_dir().join("sevak-no-such-file-xyz")),
            None,
        );
        assert_eq!(missing.note.as_deref(), Some("This item no longer exists"));
        assert_eq!(missing.body, PreviewBody::None);

        let relative = produce(&open(Path::new("relative/file.txt")), None);
        assert_eq!(
            relative.note.as_deref(),
            Some("Only absolute paths are previewed")
        );

        let network = produce(&open(Path::new("\\\\server\\share\\a.txt")), None);
        assert_eq!(
            network.note.as_deref(),
            Some("Network locations are not previewed")
        );
        assert!(network.meta.is_empty());
    }

    #[test]
    fn network_paths_are_recognised() {
        for path in [
            r"\\server\share",
            "//server/share/file",
            r"\\?\UNC\server\share",
            r"\\?\unc\server\share",
        ] {
            assert!(is_network_path(path), "{path}");
        }
        for path in [
            r"C:\Users\a",
            r"\\?\C:\Users\a",
            r"\\.\C:",
            "/home/a",
            "relative",
        ] {
            assert!(!is_network_path(path), "{path}");
        }
    }

    #[test]
    fn the_hint_beats_the_action() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "from the file").unwrap();
        let from_action = item(Action::CopyText {
            text: "copied".into(),
        });
        assert_eq!(
            produce(&from_action, None).body,
            PreviewBody::Text {
                text: "copied".into(),
                truncated: false
            }
        );
        let with_hint = from_action
            .clone()
            .with_preview(PreviewHint::Path { path: path.clone() });
        let PreviewBody::Text { text, .. } = produce(&with_hint, None).body else {
            panic!("expected text");
        };
        assert_eq!(text, "from the file");
        // The plugin's answer to a lazy request beats the item's own hint.
        let lazy = PreviewHint::Text {
            text: "expanded".into(),
        };
        let PreviewBody::Text { text, .. } = produce(&with_hint, Some(lazy)).body else {
            panic!("expected text");
        };
        assert_eq!(text, "expanded");
    }

    #[test]
    fn urls_show_the_address_and_host_without_fetching() {
        let link = item(Action::OpenUrl {
            url: "https://user:pw@docs.example.com:8443/a/b?q=1#top".into(),
        });
        let content = produce(&link, None);
        assert_eq!(
            content.body,
            PreviewBody::Url {
                url: "https://user:pw@docs.example.com:8443/a/b?q=1#top".into()
            }
        );
        assert_eq!(meta(&content, "Site"), Some("docs.example.com"));
        assert_eq!(content.title, "Title");

        assert_eq!(url_host("http://[::1]:80/x").as_deref(), Some("::1"));
        assert_eq!(url_host("mailto:a@b.c"), None);
        assert_eq!(url_host("file:///etc/hosts"), None);
    }

    #[test]
    fn details_rows_are_kept_in_order_and_bounded() {
        let rows: Vec<(String, String)> = (0..40)
            .map(|n| (format!("L{n}"), "v".repeat(MAX_DETAIL_CHARS * 2)))
            .collect();
        let content = produce(
            &item(Action::Custom {
                payload: "p".into(),
            })
            .with_preview(PreviewHint::Details { rows }),
            None,
        );
        assert_eq!(content.meta.len(), MAX_DETAIL_ROWS);
        assert_eq!(content.meta[0].label, "L0");
        assert_eq!(content.meta[0].value.len(), MAX_DETAIL_CHARS);
    }

    #[test]
    fn applications_show_their_launch_details() {
        let desktop = item(Action::Launch {
            target: LaunchTarget::DesktopEntry {
                desktop_id: "firefox.desktop".into(),
                path: PathBuf::from("/usr/share/applications/firefox.desktop"),
                exec: vec!["firefox".into(), "--new-window".into()],
                terminal: false,
                working_dir: None,
            },
        });
        let content = produce(&desktop, None);
        assert_eq!(meta(&content, "Kind"), Some("Application"));
        assert_eq!(meta(&content, "Command"), Some("firefox --new-window"));

        let store = item(Action::Launch {
            target: LaunchTarget::PackagedApp {
                app_user_model_id: "Microsoft.Calc!App".into(),
            },
        });
        let content = produce(&store, None);
        assert_eq!(meta(&content, "Kind"), Some("Store app"));
        assert_eq!(meta(&content, "App ID"), Some("Microsoft.Calc!App"));
    }

    #[test]
    fn a_mac_bundle_shows_its_version() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Thing.app");
        fs::create_dir_all(bundle.join("Contents")).unwrap();
        fs::write(
            bundle.join("Contents/Info.plist"),
            "<plist><dict><key>CFBundleName</key><string>Thing</string>\
             <key>CFBundleShortVersionString</key>\n\t<string>4.2.1</string></dict></plist>",
        )
        .unwrap();
        assert_eq!(bundle_version(&bundle).as_deref(), Some("4.2.1"));
        assert_eq!(bundle_version(dir.path()), None);

        let launch = item(Action::Launch {
            target: LaunchTarget::Executable {
                path: PathBuf::from("open"),
                args: vec!["-a".into(), bundle.to_string_lossy().into_owned()],
                working_dir: None,
            },
        });
        let content = produce(&launch, None);
        assert_eq!(meta(&content, "Version"), Some("4.2.1"));
        assert_eq!(meta(&content, "Arguments"), None);
    }

    #[test]
    fn text_views_come_from_hints_then_the_action_text() {
        let long = "word ".repeat(60);
        let row = item(Action::PasteText {
            text: long.clone(),
            restore_clipboard: true,
        });
        assert!(has_text_view(&row));
        assert_eq!(text_view(&row, None).unwrap().text, long);
        let expanded = PreviewHint::Text {
            text: "expanded".into(),
        };
        assert_eq!(text_view(&row, Some(expanded)).unwrap().text, "expanded");

        let multi = item(Action::CopyText {
            text: "a\nb".into(),
        });
        assert!(has_text_view(&multi));
        let short = item(Action::CopyText { text: "8".into() });
        assert!(!has_text_view(&short));
        assert!(!has_text_view(&item(Action::OpenUrl {
            url: "https://a.b".into()
        })));

        let shown = item(Action::Custom {
            payload: "x".into(),
        })
        .with_view(ViewHint::Text {
            text: "output".into(),
            on_enter: true,
        });
        assert!(has_text_view(&shown));
        assert_eq!(text_view(&shown, None).unwrap().text, "output");
        // ... and the pane previews it as text too.
        assert_eq!(
            produce(&shown, None).body,
            PreviewBody::Text {
                text: "output".into(),
                truncated: false
            }
        );
        assert_eq!(
            text_view(
                &item(Action::Custom {
                    payload: "x".into()
                }),
                None
            ),
            None
        );
    }

    #[test]
    fn the_text_view_is_capped() {
        let row = item(Action::CopyText {
            text: "é".repeat(MAX_VIEW_BYTES),
        });
        let view = text_view(&row, None).unwrap();
        assert!(view.truncated);
        assert!(view.text.len() <= MAX_VIEW_BYTES);
    }

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(format_size(0), "0 bytes");
        assert_eq!(format_size(1023), "1023 bytes");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
        assert!(format_size(u64::MAX).ends_with("TB"));
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn an_item_with_nothing_to_show_has_an_empty_body() {
        let content = produce(
            &item(Action::Custom {
                payload: "p".into(),
            }),
            None,
        );
        assert_eq!(content.body, PreviewBody::None);
        assert!(content.meta.is_empty());
        assert_eq!(content.note, None);
    }
}
