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
        Action::RevealPath { path } => platform.reveal_path(path).map_err(PluginError::other),
        Action::RunAsAdmin { target } => {
            platform.launch_as_admin(target).map_err(PluginError::other)
        }
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
        Action::PasteClip {
            content,
            restore_clipboard,
        } => {
            if let PasteOutcome::CopiedOnly(reason) = platform
                .paste_clip(content, *restore_clipboard)
                .map_err(PluginError::other)?
            {
                tracing::warn!(reason, "copied to the clipboard instead of pasting");
            }
            Ok(())
        }
        Action::CopyClip { content } => platform
            .set_clipboard_clip(content)
            .map_err(PluginError::other),
        Action::Custom { payload } => Err(PluginError::Unsupported(payload.clone())),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sevak_core::LaunchTarget;

    use super::*;
    use crate::test_util::MockPlatform;

    #[test]
    fn reveal_and_elevated_launch_reach_the_platform() {
        let platform = MockPlatform::with_admin();
        let path = PathBuf::from("/tmp/a.txt");
        execute_action(&*platform, &Action::RevealPath { path: path.clone() }).unwrap();
        assert_eq!(*platform.revealed.lock().unwrap(), vec![path.clone()]);

        let target = LaunchTarget::Shortcut { path };
        execute_action(
            &*platform,
            &Action::RunAsAdmin {
                target: target.clone(),
            },
        )
        .unwrap();
        assert_eq!(*platform.elevated.lock().unwrap(), vec![target]);
        assert!(platform.launched.lock().unwrap().is_empty());
    }
}
