//! Loading icons that already exist as image files.

use std::path::Path;

use sevak_core::bounded_read::{read_capped, MAX_ICON_BYTES};
use sevak_core::IconData;

use crate::error::{PlatformError, Result};

/// Reads an image file the web view can display directly. XPM and other legacy
/// formats are rejected so the UI falls back to its generic glyph.
pub fn load(path: &Path) -> Result<IconData> {
    let mime = mime_for(path).ok_or_else(|| PlatformError::Os {
        operation: "load_icon",
        message: format!("unsupported icon format: {}", path.display()),
    })?;
    Ok(IconData {
        mime,
        bytes: read_capped(path, MAX_ICON_BYTES)?,
    })
}

/// MIME type for the image formats web views render, by file extension.
pub fn mime_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "svg" => Some("image/svg+xml"),
        "ico" => Some("image/x-icon"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_by_extension() {
        assert_eq!(mime_for(Path::new("a/b.PNG")), Some("image/png"));
        assert_eq!(mime_for(Path::new("x.svg")), Some("image/svg+xml"));
        assert_eq!(mime_for(Path::new("x.xpm")), None);
        assert_eq!(mime_for(Path::new("noext")), None);
    }
}
