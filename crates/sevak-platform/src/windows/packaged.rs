//! Packaged (UWP / MSIX / Store) app enumeration through `shell:AppsFolder`.

use sevak_core::{AppEntry, IconSource, LaunchTarget};
use windows::core::HSTRING;
use windows::Win32::UI::Shell::{
    BHID_EnumItems, IEnumShellItems, IShellItem, SHCreateItemFromParsingName, SIGDN,
    SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};

use super::shortcuts::take_pwstr;

/// Enumerates packaged apps. COM must be initialized on the calling thread.
pub(crate) fn enumerate() -> windows::core::Result<Vec<AppEntry>> {
    // SAFETY: the parsing name is a NUL-terminated HSTRING and no bind context
    // is passed.
    let folder: IShellItem =
        unsafe { SHCreateItemFromParsingName(&HSTRING::from("shell:AppsFolder"), None::<&_>)? };
    // SAFETY: `folder` is a live shell item; the handler GUID is a static.
    let items: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems)? };

    let mut entries = Vec::new();
    loop {
        let mut batch: [Option<IShellItem>; 16] = Default::default();
        let mut fetched = 0u32;
        // SAFETY: `batch` and `fetched` are exclusively borrowed and valid for
        // the call. S_FALSE (fewer items than requested) comes back as `Ok`.
        let result = unsafe { items.Next(&mut batch, Some(&mut fetched)) };
        if let Err(err) = result {
            tracing::debug!(%err, "IEnumShellItems::Next failed; stopping enumeration");
            break;
        }
        if fetched == 0 {
            break;
        }
        for item in batch.iter().take(fetched as usize).flatten() {
            if let Some(entry) = entry_for(item) {
                entries.push(entry);
            }
        }
    }
    Ok(entries)
}

fn display_name(item: &IShellItem, form: SIGDN) -> Option<String> {
    // SAFETY: `item` is a live shell item; the returned string is freed by
    // `take_pwstr`.
    let pwstr = unsafe { item.GetDisplayName(form) }.ok()?;
    Some(take_pwstr(pwstr))
}

fn entry_for(item: &IShellItem) -> Option<AppEntry> {
    let name = display_name(item, SIGDN_NORMALDISPLAY)?;
    let parsing = display_name(item, SIGDN_PARENTRELATIVEPARSING)?;
    if name.trim().is_empty() || !is_aumid(&parsing) {
        return None;
    }
    Some(AppEntry {
        id: format!("aumid:{parsing}"),
        name: name.trim().to_owned(),
        description: None,
        keywords: Vec::new(),
        icon: Some(IconSource::Shell {
            parsing_name: format!(r"shell:AppsFolder\{parsing}"),
        }),
        target: LaunchTarget::PackagedApp {
            app_user_model_id: parsing,
        },
    })
}

/// `PackageFamilyName!AppId`: a package family name, a bang, and an app id.
/// Classic desktop items have paths or GUID-based names and are rejected.
pub(crate) fn is_aumid(parsing_name: &str) -> bool {
    match parsing_name.split_once('!') {
        Some((family, app)) => {
            !family.is_empty() && !app.is_empty() && !family.contains(['\\', '/', ':', '{'])
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_aumids() {
        assert!(is_aumid("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        assert!(is_aumid("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"));
        assert!(!is_aumid("Microsoft.Windows.Explorer"));
        assert!(!is_aumid(
            r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\notepad.exe"
        ));
        assert!(!is_aumid("Pkg_abc!"));
        assert!(!is_aumid(""));
    }
}
