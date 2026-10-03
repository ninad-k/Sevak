//! macOS: `NSFileManager`'s `trashItemAtURL`, the call Finder's own "Move to
//! Trash" uses. No permission prompt, and the item can be put back from the
//! Trash.

use std::path::Path;

use objc2_foundation::{NSFileManager, NSString, NSURL};

use crate::error::{PlatformError, Result};

pub(crate) fn move_to_trash(path: &Path) -> Result<()> {
    let text = path.to_str().ok_or_else(|| PlatformError::Os {
        operation: "move to Trash",
        message: format!("path is not valid UTF-8: {}", path.display()),
    })?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(text));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|err| PlatformError::Os {
            operation: "move to Trash",
            message: err.localizedDescription().to_string(),
        })
}
