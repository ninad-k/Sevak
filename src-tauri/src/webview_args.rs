//! WebView2 browser arguments.
//!
//! wry hands every webview its own defaults as `AdditionalBrowserArguments`.
//! WebView2 157 merges those with the `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`
//! environment variable, but 156 drops the variable whenever the app names
//! arguments. msedgedriver asks for `--remote-debugging-port` through that
//! variable, so on 156 it never gets a session ("DevToolsActivePort file
//! doesn't exist"). When the variable is set, Sevak merges the two itself, so
//! the result does not depend on the runtime version.
//!
//! Every webview sharing a data folder must use identical arguments, so the
//! launcher and the Settings window both take theirs from [`from_env`].

/// The variable WebView2 reads extra browser switches from.
const ENV: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";

/// What wry passes when the app names no arguments (Sevak sets no proxy, and
/// Tauri turns autoplay on).
const WRY_DEFAULT: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required";

/// Switches whose values are comma-separated lists, merged rather than replaced.
const LIST_SWITCHES: [&str; 2] = ["--disable-features", "--enable-features"];

/// The arguments every webview gets, or `None` to keep wry's own defaults.
/// Only Windows reads the variable; elsewhere the arguments are unused.
pub fn from_env() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let extra = std::env::var(ENV).ok()?;
    let extra = extra.trim();
    (!extra.is_empty()).then(|| merge(WRY_DEFAULT, extra))
}

/// `base` followed by `extra`, with each feature list combined into one switch
/// (Chromium reads only the last `--disable-features`) and repeated switches
/// written once.
fn merge(base: &str, extra: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for token in base.split_whitespace().chain(extra.split_whitespace()) {
        let list = LIST_SWITCHES.iter().find_map(|name| {
            token
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix('='))
                .map(|values| (*name, values))
        });
        match list {
            Some((name, values)) => {
                let prefix = format!("{name}=");
                match out.iter_mut().find(|t| t.starts_with(&prefix)) {
                    Some(existing) => {
                        for value in values.split(',').filter(|v| !v.is_empty()) {
                            if !existing[prefix.len()..].split(',').any(|v| v == value) {
                                existing.push(',');
                                existing.push_str(value);
                            }
                        }
                    }
                    None => out.push(token.to_owned()),
                }
            }
            None if out.iter().any(|t| t == token) => {}
            None => out.push(token.to_owned()),
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_wry_defaults_and_adds_the_drivers_switches() {
        // What msedgedriver 156 sets, shortened.
        let merged = merge(
            WRY_DEFAULT,
            "--disable-features=IgnoreDuplicateNavs,Prewarm --enable-automation --remote-debugging-port=0",
        );
        assert_eq!(
            merged,
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IgnoreDuplicateNavs,Prewarm \
             --autoplay-policy=no-user-gesture-required --enable-automation --remote-debugging-port=0"
        );
    }

    #[test]
    fn writes_each_switch_and_feature_once() {
        assert_eq!(
            merge(
                "--a --disable-features=X,Y",
                "--a  --disable-features=Y,Z --enable-features=P --enable-features=P,Q"
            ),
            "--a --disable-features=X,Y,Z --enable-features=P,Q"
        );
    }

    #[test]
    fn a_switch_that_only_shares_a_prefix_is_not_a_feature_list() {
        assert_eq!(
            merge("--disable-features=X", "--disable-features-extra=1"),
            "--disable-features=X --disable-features-extra=1"
        );
    }

    #[test]
    fn whitespace_only_extra_leaves_the_base() {
        assert_eq!(merge(WRY_DEFAULT, "   "), WRY_DEFAULT);
    }
}
