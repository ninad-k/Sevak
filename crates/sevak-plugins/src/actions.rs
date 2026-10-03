//! Shared execution of the standard [`Action`]s.

use sevak_core::{Action, PluginError, PluginResult};
use sevak_platform::{PasteOutcome, PlatformProvider};

/// Performs `action` through the platform provider. [`Action::Custom`] is
/// plugin-defined and therefore [`PluginError::Unsupported`] here.
pub fn execute_action(platform: &dyn PlatformProvider, action: &Action) -> PluginResult<()> {
    match action {
        Action::Launch { target } => platform.launch(target).map_err(PluginError::other),
        Action::OpenPath { path } => platform.open_path(path).map_err(PluginError::other),
        Action::OpenUrl { url } => platform.open_url(url).map_err(PluginError::other),
        Action::CopyText { text } => platform
            .set_clipboard_text(text)
            .map_err(PluginError::other),
        Action::PasteText {
            text,
            restore_clipboard,
        } => {
            // Not pasting is not a failure: the text is on the clipboard and the
            // result's subtitle already said "Copies to clipboard".
            if let PasteOutcome::CopiedOnly(reason) = platform
                .paste_text(text, *restore_clipboard)
                .map_err(PluginError::other)?
            {
                tracing::warn!(reason, "copied to the clipboard instead of pasting");
            }
            Ok(())
        }
        Action::Custom { payload } => Err(PluginError::Unsupported(payload.clone())),
    }
}
