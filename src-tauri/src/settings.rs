//! The settings window and the commands behind it.

use std::collections::HashSet;

use serde::Serialize;
use sevak_core::config::{Config, Theme};
use sevak_core::theme;
use sevak_platform::accelerator;
use sevak_platform::{gnome, hotkey_hook, open, paths, session, HotkeyStrategy};
use sevak_plugins::keywords::{
    configurable_keywords, ConfigurableKeyword, KeywordOwners, FIXED_KEYWORDS,
};
use sevak_plugins::{PluginInfo, PluginRegistry};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::state::AppState;
use crate::{app, hotkey, takeover, window};

const WINDOW_TITLE: &str = "Sevak Settings";
/// Same frontend as the launcher; `main.ts` picks the view from the hash.
const WINDOW_URL: &str = "index.html#settings";

/// Opens the settings window, or brings the open one to the front.
/// The launcher steps aside first so it does not sit on top of it.
pub fn open(app: &AppHandle) {
    window::hide(app);
    // macOS: hiding the launcher hid the whole app; Settings must be visible.
    window::unhide_app(app);

    if let Some(existing) = app.get_webview_window(window::SETTINGS_LABEL) {
        tracing::info!("settings window already open; focusing it");
        if existing.is_minimized().unwrap_or(false) {
            let _ = existing.unminimize();
        }
        let _ = existing.show();
        if let Err(err) = existing.set_focus() {
            tracing::warn!("could not focus the settings window: {err}");
        }
        return;
    }

    tracing::info!("opening the settings window");
    let theme = app
        .try_state::<AppState>()
        .map(|state| state.config().appearance.theme)
        .and_then(window_theme);
    let result = WebviewWindowBuilder::new(
        app,
        window::SETTINGS_LABEL,
        WebviewUrl::App(WINDOW_URL.into()),
    )
    .title(WINDOW_TITLE)
    .inner_size(920.0, 680.0)
    .min_inner_size(560.0, 460.0)
    .resizable(true)
    .center()
    .decorations(true)
    .always_on_top(false)
    .skip_taskbar(false)
    .theme(theme)
    .build();
    if let Err(err) = result {
        tracing::error!("could not open the settings window: {err}");
    }
}

/// Everything the settings window needs to render.
#[derive(Debug, Serialize)]
pub struct SettingsDto {
    pub config: Config,
    pub catalog: Vec<PluginInfo>,
    /// `windows`, `macos`, `x11`, `wayland` or `unknown`.
    pub display: &'static str,
    pub is_gnome: bool,
    pub config_path: String,
    pub log_dir: String,
    /// `windows`, `macos` or `linux`.
    pub platform: &'static str,
}

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> SettingsDto {
    let state = app.state::<AppState>();
    let config = state.config();
    let mut catalog = PluginRegistry::builtin().catalog(&config, state.search.platform.clone());
    let owners = KeywordOwners::collect(&config, &state.search.scripts, &state.search.workflows);
    catalog.extend(state.search.scripts.catalog(&config, &owners));
    SettingsDto {
        config,
        catalog,
        display: state.display.as_str(),
        is_gnome: session::is_gnome(),
        config_path: state.paths.config_file.display().to_string(),
        log_dir: state.paths.log_dir.display().to_string(),
        platform: if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        },
    }
}

/// Parses a shortcut with the same parser the global-shortcut plugin uses
/// (after rewriting the key names it does not know, such as `Win`).
fn check_accelerator(accelerator: &str) -> Result<(), String> {
    hotkey::parse_shortcut(accelerator)
        .map(|_| ())
        .map_err(|reason| format!("\"{accelerator}\" is not a valid shortcut: {reason}"))
}

/// Writes the shortcuts the portable way: `Win`, `Windows` and `Meta` become
/// `Super`, which every OS's parser knows (and which shows as Win, Cmd or
/// Super on each). Everything else is left as typed.
fn normalize_hotkeys(config: &mut Config) {
    let fix = |key: &mut String| *key = accelerator::normalize(key);
    fix(&mut config.general.hotkey);
    if !config.general.actions_hotkey.trim().is_empty() {
        fix(&mut config.general.actions_hotkey);
    }
    for binding in &mut config.hotkeys {
        if !binding.key.trim().is_empty() {
            fix(&mut binding.key);
        }
    }
}

/// Checks a configuration the way the settings form does, so a hand-crafted
/// call cannot write something Sevak would silently drop on the next load.
pub fn validate(config: &Config, strategy: HotkeyStrategy) -> Result<(), String> {
    let hotkey = config.general.hotkey.trim();
    if hotkey.is_empty() {
        return Err("The shortcut cannot be empty.".to_owned());
    }
    // On Wayland the desktop owns the key, so Sevak never parses it.
    if strategy == HotkeyStrategy::InApp {
        check_accelerator(hotkey)?;
    }

    validate_hotkeys(config, strategy)?;
    theme::validate(&config.appearance).map_err(|reason| format!("Appearance: {reason}."))?;

    let mut keywords: HashSet<String> = HashSet::new();
    for engine in &config.web_search {
        let keyword = engine.keyword.trim();
        if keyword.is_empty() {
            return Err("A web search engine needs a keyword.".to_owned());
        }
        if keyword.chars().any(char::is_whitespace) {
            return Err(format!("The keyword \"{keyword}\" cannot contain spaces."));
        }
        if !keywords.insert(keyword.to_lowercase()) {
            return Err(format!("The keyword \"{keyword}\" is used twice."));
        }
        if engine.name.trim().is_empty() {
            return Err(format!("The engine \"{keyword}\" needs a name."));
        }
        let url = engine.url.trim().to_ascii_lowercase();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(format!(
                "The URL of \"{keyword}\" must start with http:// or https://."
            ));
        }
        if !engine.url.contains("{query}") {
            return Err(format!("The URL of \"{keyword}\" must contain {{query}}."));
        }
    }

    for fallback in config.search.fallback_web_search.keywords() {
        if !keywords.contains(&fallback.to_lowercase()) {
            return Err(format!(
                "The fallback search engine \"{fallback}\" is not defined."
            ));
        }
    }

    validate_builtin_keywords(config, &keywords)?;
    if config
        .files
        .directories
        .iter()
        .any(|dir| dir.trim().is_empty())
    {
        return Err("A files directory is empty.".to_owned());
    }
    let prefix = config.snippets.prefix.trim();
    if prefix.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("The snippet prefix cannot contain spaces.".to_owned());
    }
    Ok(())
}

/// The configurable keywords of the built-in plugins (`[files]`, `[bookmarks]`,
/// `[tasks]`, `[media]`, `[contacts]`, `[onepassword]`, `[dictionary]`) are one
/// word each, and no two searches share a keyword (the lists live in
/// `sevak_plugins::keywords`, which also finds workflow and script plugin
/// clashes, only a warning there): not two built-in ones, not a
/// built-in one and a fixed keyword (`>`, `cb`, `s`, `emoji`, `:`, `@`, `uuid`),
/// and not either of those and a web search engine (`web`, lowercased). An
/// empty keyword turns that keyword off and never clashes.
fn validate_builtin_keywords(config: &Config, web: &HashSet<String>) -> Result<(), String> {
    for (keyword, owner) in FIXED_KEYWORDS {
        if web.contains(*keyword) {
            return Err(format!(
                "The web search keyword \"{keyword}\" is already used by {owner}."
            ));
        }
    }
    let mut taken: Vec<(String, &str)> = FIXED_KEYWORDS
        .iter()
        .map(|(keyword, owner)| ((*keyword).to_owned(), *owner))
        .collect();
    for ConfigurableKeyword {
        label,
        keyword,
        owner,
    } in configurable_keywords(config)
    {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            continue;
        }
        if keyword.chars().any(char::is_whitespace) {
            return Err(format!("The {label} keyword cannot contain spaces."));
        }
        let lower = keyword.to_lowercase();
        if web.contains(&lower) {
            return Err(format!(
                "The {label} keyword \"{keyword}\" is already a web search keyword."
            ));
        }
        if let Some((_, other)) = taken.iter().find(|(used, _)| *used == lower) {
            return Err(format!(
                "The {label} keyword \"{keyword}\" is already used by {other}."
            ));
        }
        taken.push((lower, owner));
    }
    Ok(())
}

/// A key's identity for finding duplicates: spelling variants of one
/// shortcut (`ctrl+alt+t`, `Control + Alt + T`) must compare equal.
fn key_identity(accelerator: &str, strategy: HotkeyStrategy) -> String {
    match (strategy, hotkey::parse_shortcut(accelerator)) {
        (HotkeyStrategy::InApp, Ok(shortcut)) => shortcut.id().to_string(),
        _ => accelerator
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_lowercase(),
    }
}

/// The Universal Actions key (empty turns it off), the `[[hotkey]]` entries: a
/// usable key, one action, no key used twice.
fn validate_hotkeys(config: &Config, strategy: HotkeyStrategy) -> Result<(), String> {
    let mut seen = vec![key_identity(config.general.hotkey.trim(), strategy)];
    let actions = config.general.actions_hotkey.trim();
    if !actions.is_empty() {
        if strategy == HotkeyStrategy::InApp {
            check_accelerator(actions)?;
        }
        let identity = key_identity(actions, strategy);
        if seen.contains(&identity) {
            return Err(format!(
                "The Universal Actions shortcut \"{actions}\" is the same as the main shortcut."
            ));
        }
        seen.push(identity);
    }
    for binding in &config.hotkeys {
        let key = binding.key.trim();
        if key.is_empty() {
            return Err("A hotkey entry needs a shortcut.".to_owned());
        }
        if strategy == HotkeyStrategy::InApp {
            check_accelerator(key)?;
        }
        if let Err(reason) = binding.target() {
            return Err(format!("The hotkey \"{key}\": {reason}."));
        }
        let identity = key_identity(key, strategy);
        if seen.contains(&identity) {
            return Err(format!(
                "The shortcut \"{key}\" is used more than once (the main shortcut counts)."
            ));
        }
        seen.push(identity);
    }
    Ok(())
}

/// Live check for the shortcut field.
#[tauri::command]
pub fn validate_hotkey(state: State<'_, AppState>, hotkey: String) -> Result<(), String> {
    let hotkey = hotkey.trim();
    if hotkey.is_empty() {
        return Err("The shortcut cannot be empty.".to_owned());
    }
    match state.display.hotkey_strategy() {
        HotkeyStrategy::InApp => check_accelerator(hotkey),
        HotkeyStrategy::External => Ok(()),
    }
}

/// While the recorder listens for a key combination the real hotkey must not
/// fire (it would toggle the launcher instead of reaching the form).
#[tauri::command]
pub fn suspend_hotkey(app: AppHandle) {
    let in_app = app
        .try_state::<AppState>()
        .is_some_and(|state| state.display.hotkey_strategy() == HotkeyStrategy::InApp);
    if !in_app {
        return;
    }
    if let Err(err) = app.global_shortcut().unregister_all() {
        tracing::warn!("could not suspend the hotkey: {err}");
    }
    // The keyboard hook's keys too (Win+Space would toggle the launcher instead
    // of reaching the form).
    hotkey_hook::set_suspended(true);
}

/// The recorder asks the keyboard hook for the next shortcut, because Windows
/// never lets the form see Win+Space (or a key another app owns). The answer
/// comes as the `sevak:hotkey-recorded` event. Fails where there is no hook:
/// the form then records with the keys it sees itself.
#[tauri::command]
pub fn start_hotkey_recording() -> Result<(), String> {
    hotkey_hook::start_recording()
}

#[tauri::command]
pub fn stop_hotkey_recording() {
    hotkey_hook::stop_recording();
}

#[tauri::command]
pub fn resume_hotkey(app: AppHandle) {
    hotkey::apply(&app);
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, config: Config) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || save(&app, config))
        .await
        .map_err(|err| format!("saving did not finish: {err}"))?
}

fn save(app: &AppHandle, mut config: Config) -> Result<(), String> {
    let state = app.state::<AppState>();
    normalize_hotkeys(&mut config);
    validate(&config, state.display.hotkey_strategy())?;
    let config = config.normalized();
    config
        .save_to(&state.paths.config_file)
        .map_err(|err| err.to_string())?;
    tracing::info!(path = %state.paths.config_file.display(), "settings saved");
    // Re-reads the file just written, so what runs is exactly what is on disk.
    app::reload(app);
    Ok(())
}

/// Opens a native folder picker over the settings window. `None` when the
/// user cancels. The path comes back `~`-relative where possible.
#[tauri::command]
pub async fn pick_directory(app: AppHandle, window: WebviewWindow) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app
            .dialog()
            .file()
            .set_title("Choose a folder to search")
            .set_parent(&window)
            .blocking_pick_folder()?;
        match picked.into_path() {
            Ok(path) => Some(paths::home_relative(&path)),
            Err(err) => {
                tracing::warn!("the picked folder is not a local path: {err}");
                None
            }
        }
    })
    .await
    .unwrap_or_else(|err| {
        tracing::warn!("folder picker failed: {err}");
        None
    })
}

/// "Set up GNOME shortcut": see [`gnome::setup_for_ui`]. The saved `[[hotkey]]`
/// entries are bound along with the typed shortcut.
#[tauri::command]
pub async fn setup_wayland_hotkey(app: AppHandle, hotkey: String) -> Result<String, String> {
    let customs = hotkey::custom_shortcuts(&app.state::<AppState>().config());
    tauri::async_runtime::spawn_blocking(move || {
        // Super+Space is GNOME's input-source switcher: offer to move it first.
        let moved = takeover::offer_gnome_for_setup(&app, hotkey.trim());
        gnome::setup_for_ui(hotkey.trim(), &customs)
            .map(|text| format!("{moved}{text}"))
            .map_err(|text| format!("{moved}{text}"))
    })
    .await
    .map_err(|err| format!("the setup did not finish: {err}"))?
}

#[tauri::command]
pub async fn open_config_file(state: State<'_, AppState>) -> Result<(), String> {
    let path = state.paths.config_file.clone();
    open::open_in_editor(&path).map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn open_log_dir(state: State<'_, AppState>) -> Result<(), String> {
    let dir = state.paths.log_dir.clone();
    // Nothing is logged to disk until the first run has written something.
    let _ = std::fs::create_dir_all(&dir);
    open::open_path(&dir).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn close_settings(app: AppHandle) {
    if let Some(window) = app.get_webview_window(window::SETTINGS_LABEL) {
        if let Err(err) = window.close() {
            tracing::warn!("could not close the settings window: {err}");
        }
    }
}

/// The theme as the OS-level window theme (`None` follows the system).
pub fn window_theme(theme: Theme) -> Option<tauri::Theme> {
    match theme {
        Theme::System => None,
        Theme::Light => Some(tauri::Theme::Light),
        Theme::Dark => Some(tauri::Theme::Dark),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sevak_core::config::{FallbackSearch, HotkeyBinding, WebSearchEngine};

    fn engine(keyword: &str, url: &str) -> WebSearchEngine {
        WebSearchEngine {
            keyword: keyword.to_owned(),
            name: "Name".to_owned(),
            url: url.to_owned(),
        }
    }

    fn check(config: &Config) -> Result<(), String> {
        validate(config, HotkeyStrategy::InApp)
    }

    #[test]
    fn defaults_are_valid() {
        assert_eq!(check(&Config::default()), Ok(()));
    }

    #[test]
    fn the_snippet_prefix_is_one_word() {
        let mut config = Config::default();
        for good in ["", ";", ";;", "//", " ; "] {
            config.snippets.prefix = good.to_owned();
            assert_eq!(check(&config), Ok(()), "{good:?}");
        }
        for bad in ["; ;", "a b", ";\tx"] {
            config.snippets.prefix = bad.to_owned();
            assert!(check(&config).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn hotkeys_are_parsed_like_the_plugin_does() {
        let mut config = Config::default();
        for good in [
            "Alt+Space",
            "Ctrl+Shift+K",
            "Super+F12",
            "CmdOrCtrl+Comma",
            "Super+Space",
            "Win+Space",
            "windows + space",
            "Meta+F9",
            "Cmd+Space",
        ] {
            config.general.hotkey = good.to_owned();
            assert_eq!(check(&config), Ok(()), "{good}");
        }
        for bad in ["", "  ", "Alt+", "Banana+K", "Ctrl+Shift", "Ctrl+Space+K"] {
            config.general.hotkey = bad.to_owned();
            assert!(check(&config).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn the_default_super_space_validates_and_round_trips() {
        let mut config = Config::default();
        assert_eq!(config.general.hotkey, "Super+Space");
        assert_eq!(check(&config), Ok(()));
        // Spelled the way Windows users say it, it is saved the portable way
        // and is still the same key.
        config.general.hotkey = " Win + Space ".to_owned();
        normalize_hotkeys(&mut config);
        assert_eq!(config.general.hotkey, "Super+Space");
        assert_eq!(check(&config), Ok(()));
        assert_eq!(
            config.clone().normalized().general.hotkey,
            "Super+Space",
            "saving keeps it"
        );
    }

    #[test]
    fn normalizing_rewrites_every_kind_of_key_but_leaves_the_rest() {
        let mut config = Config::default();
        config.general.actions_hotkey = "Ctrl+Windows+A".to_owned();
        config.hotkeys = vec![
            entry("Meta+T", Some("> "), None),
            entry("ctrl+alt+k", Some("x"), None),
            entry("", Some("x"), None),
        ];
        normalize_hotkeys(&mut config);
        assert_eq!(config.general.actions_hotkey, "Ctrl+Super+A");
        assert_eq!(config.hotkeys[0].key, "Super+T");
        // Only spellings are touched, not case or order.
        assert_eq!(config.hotkeys[1].key, "ctrl+alt+k");
        assert_eq!(config.hotkeys[2].key, "");
        // An off Universal Actions key stays off.
        config.general.actions_hotkey = "  ".to_owned();
        normalize_hotkeys(&mut config);
        assert_eq!(config.general.actions_hotkey, "  ");
    }

    #[test]
    fn wayland_does_not_parse_the_hotkey_but_needs_one() {
        let mut config = Config::default();
        config.general.hotkey = "Banana+K".to_owned();
        assert_eq!(validate(&config, HotkeyStrategy::External), Ok(()));
        config.general.hotkey = " ".to_owned();
        assert!(validate(&config, HotkeyStrategy::External).is_err());
    }

    #[test]
    fn web_engines_are_validated() {
        let ok = engine("ddg", "https://duckduckgo.com/?q={query}");
        let cases = [
            engine("", "https://x.test/?q={query}"),
            engine("a b", "https://x.test/?q={query}"),
            engine("x", "ftp://x.test/?q={query}"),
            engine("x", "https://x.test/"),
        ];
        for bad in cases {
            let config = Config {
                web_search: vec![ok.clone(), bad.clone()],
                ..Config::default()
            };
            assert!(check(&config).is_err(), "{bad:?}");
        }

        let config = Config {
            web_search: vec![ok.clone(), engine("DDG", "https://x.test/?q={query}")],
            ..Config::default()
        };
        assert!(check(&config).unwrap_err().contains("twice"));
    }

    #[test]
    fn fallback_must_exist_or_be_empty() {
        let mut config = Config::default();
        config.search.fallback_web_search = FallbackSearch::single("nope");
        assert!(check(&config).is_err());
        config.search.fallback_web_search = FallbackSearch::single("");
        assert_eq!(check(&config), Ok(()));
        config.search.fallback_web_search = FallbackSearch::single("yt");
        assert_eq!(check(&config), Ok(()));
        config.search.fallback_web_search = FallbackSearch::list(["g", "yt"]);
        assert_eq!(check(&config), Ok(()));
        config.search.fallback_web_search = FallbackSearch::list(["g", "nope"]);
        assert!(check(&config).unwrap_err().contains("nope"));
    }

    #[test]
    fn files_keyword_must_not_clash_with_a_web_keyword() {
        let mut config = Config::default();
        config.files.keyword = "g".to_owned();
        assert!(check(&config).is_err());
        config.files.keyword = "find me".to_owned();
        assert!(check(&config).is_err());
    }

    #[test]
    fn built_in_keywords_cannot_collide() {
        assert!(check(&Config::default()).is_ok());
        type Set = fn(&mut Config, &str);
        let setters: [(&str, Set); 7] = [
            ("tasks", |c, k| c.tasks.keyword = k.into()),
            ("media", |c, k| c.media.keyword = k.into()),
            ("contacts", |c, k| c.contacts.keyword = k.into()),
            ("1password", |c, k| c.onepassword.keyword = k.into()),
            ("define", |c, k| c.dictionary.define_keyword = k.into()),
            ("spell", |c, k| c.dictionary.spell_keyword = k.into()),
            ("bookmarks", |c, k| c.bookmarks.keyword = k.into()),
        ];
        for (name, set) in setters {
            // A fixed keyword, another built-in one, a web engine's (any
            // case), or two words.
            for taken in [
                ">", "cb", "S", ":", "@", "emoji", "uuid", "ff", "in", "g", "YT", "a b",
            ] {
                let mut config = Config::default();
                set(&mut config, taken);
                assert!(check(&config).is_err(), "{name} = {taken:?}");
            }
            // A free word is fine, and empty turns the keyword off.
            for free in ["zz", ""] {
                let mut config = Config::default();
                set(&mut config, free);
                assert_eq!(check(&config), Ok(()), "{name} = {free:?}");
            }
        }
        // Two configurable ones clash with each other too.
        let mut config = Config::default();
        config.tasks.keyword = "x".into();
        config.media.keyword = "X".into();
        let err = check(&config).unwrap_err();
        assert!(err.contains("automation tasks"), "{err}");
        // A web engine cannot take a fixed keyword.
        let mut config = Config::default();
        config.web_search[0].keyword = "cb".into();
        assert!(check(&config).is_err());
    }

    #[test]
    fn os_index_keywords_must_be_single_words_and_unique() {
        let mut config = Config::default();
        assert!(check(&config).is_ok());
        for (index, content) in [
            ("g", "in"),
            ("ff", "yt"),
            ("f", "in"),
            ("ff", "b"),
            ("x", "X"),
        ] {
            config.files.index_keyword = index.to_owned();
            config.files.content_keyword = content.to_owned();
            assert!(check(&config).is_err(), "{index} / {content}");
        }
        config.files.index_keyword = "find all".to_owned();
        config.files.content_keyword = "in".to_owned();
        assert!(check(&config).is_err());
        // Empty turns a search off; two empty keywords do not clash.
        config.files.index_keyword = String::new();
        config.files.content_keyword = String::new();
        assert!(check(&config).is_ok());
    }

    fn entry(key: &str, query: Option<&str>, run: Option<&str>) -> HotkeyBinding {
        HotkeyBinding {
            key: key.to_owned(),
            query: query.map(str::to_owned),
            run: run.map(str::to_owned),
        }
    }

    fn with_entries(entries: Vec<HotkeyBinding>) -> Config {
        Config {
            hotkeys: entries,
            ..Config::default()
        }
    }

    #[test]
    fn hotkey_entries_are_validated() {
        let good = with_entries(vec![
            entry("Ctrl+Alt+T", Some("> "), None),
            entry("Ctrl+Alt+F", None, Some("apps:firefox.desktop")),
            entry("F9", Some(""), None),
        ]);
        assert_eq!(check(&good), Ok(()));

        let bad = [
            entry("", Some("x"), None),
            entry("Banana+K", Some("x"), None),
            entry("Ctrl+Alt+T", None, None),
            entry("Ctrl+Alt+T", Some("x"), Some("y")),
            entry("Ctrl+Alt+T", None, Some("  ")),
        ];
        for entry in bad {
            assert!(
                check(&with_entries(vec![entry.clone()])).is_err(),
                "{entry:?}"
            );
        }
    }

    #[test]
    fn the_actions_key_is_optional_but_must_be_a_usable_key() {
        let mut config = Config::default();
        assert_eq!(check(&config), Ok(()));
        config.general.actions_hotkey = String::new();
        assert_eq!(check(&config), Ok(()));
        config.general.actions_hotkey = "  ".to_owned();
        assert_eq!(check(&config), Ok(()));
        config.general.actions_hotkey = "Ctrl+Shift+A".to_owned();
        assert_eq!(check(&config), Ok(()));
        config.general.actions_hotkey = "Banana+K".to_owned();
        assert!(check(&config).is_err());
        // Wayland leaves parsing to the desktop.
        assert_eq!(validate(&config, HotkeyStrategy::External), Ok(()));
    }

    #[test]
    fn the_actions_key_cannot_clash_with_other_keys() {
        let mut config = Config::default();
        config.general.actions_hotkey = "win + space".to_owned();
        assert!(check(&config).unwrap_err().contains("same as the main"));
        config.general.hotkey = "Alt+Space".to_owned();
        config.general.actions_hotkey = "alt+space".to_owned();
        assert!(check(&config).unwrap_err().contains("same as the main"));
        config.general.hotkey = Config::default().general.hotkey;

        let mut config = with_entries(vec![entry("ctrl + alt + space", Some("x"), None)]);
        assert!(check(&config).unwrap_err().contains("more than once"));
        config.general.actions_hotkey = String::new();
        assert_eq!(check(&config), Ok(()));
    }

    #[test]
    fn hotkey_entries_cannot_share_a_key() {
        let twice = with_entries(vec![
            entry("Ctrl+Alt+T", Some("a"), None),
            entry("ctrl + alt + t", Some("b"), None),
        ]);
        assert!(check(&twice).unwrap_err().contains("more than once"));

        // The main shortcut counts, however it is spelled.
        let clash = with_entries(vec![entry("meta + SPACE", Some("a"), None)]);
        assert!(check(&clash).is_err());
    }

    #[test]
    fn wayland_does_not_parse_entry_keys_but_still_needs_an_action() {
        let config = with_entries(vec![entry("Banana+K", Some("x"), None)]);
        assert_eq!(validate(&config, HotkeyStrategy::External), Ok(()));
        let config = with_entries(vec![entry("Banana+K", None, None)]);
        assert!(validate(&config, HotkeyStrategy::External).is_err());
    }

    #[test]
    fn appearance_values_are_validated() {
        let mut config = Config::default();
        config.appearance.accent = "#7c3aed".to_owned();
        config.appearance.font_size = 18;
        assert_eq!(check(&config), Ok(()));

        config.appearance.accent = "violet".to_owned();
        assert!(check(&config).unwrap_err().starts_with("Appearance:"));
        config.appearance.accent = String::new();
        config.appearance.opacity = 10;
        assert!(check(&config).is_err());
    }

    #[test]
    fn bookmarks_keyword_must_not_clash_with_a_web_keyword() {
        let mut config = Config::default();
        config.bookmarks.keyword = "gh".to_owned();
        assert!(check(&config).is_err());
        config.bookmarks.keyword = "my marks".to_owned();
        assert!(check(&config).is_err());
        config.bookmarks.keyword = String::new();
        assert_eq!(check(&config), Ok(()));
    }

    #[test]
    fn window_theme_mapping() {
        assert_eq!(window_theme(Theme::System), None);
        assert_eq!(window_theme(Theme::Dark), Some(tauri::Theme::Dark));
    }
}
