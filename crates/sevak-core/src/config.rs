//! User configuration stored as TOML.
//!
//! Every field has a default and every section is `#[serde(default)]`, so a
//! config written by an older Sevak (or a hand-trimmed one) keeps loading as new
//! options are added. Unknown keys are ignored for the same reason.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The file written on first run. It mirrors [`Config::default`] (enforced by a
/// unit test) but carries comments, which `toml::to_string` cannot produce.
pub const DEFAULT_CONFIG_TOML: &str = r##"# Sevak configuration
#
# Created with default values on first run. Edit it, then choose "Reload index"
# from the tray menu (or restart Sevak) to apply changes.

[general]
# Shortcut that shows and hides Sevak. Super is the Windows key (Cmd on macOS).
# Examples: "Super+Space", "Alt+Space", "Ctrl+Space", "Ctrl+Shift+K".
# Super+Space is normally taken by the system (Windows' input-language switcher,
# macOS Spotlight, GNOME's input sources). Sevak takes the key over on Windows
# with a keyboard hook, and on macOS and GNOME after asking you once. To go back
# to a key the system leaves alone, use "Alt+Space" (Option+Space on macOS).
# On Linux Wayland sessions applications cannot grab global keys. Run
# `sevak --setup-hotkey` to bind this key to `sevak --toggle` in GNOME instead.
hotkey = "Super+Space"

# Shortcut for Universal Actions: it copies what you have selected in the app
# you are using (text, a URL, files) and offers actions for it. "" turns it off.
# On Wayland run `sevak --setup-hotkey` to bind it to `sevak --actions`. See [actions].
actions_hotkey = "Ctrl+Alt+Space"

# Hide the window when it loses focus.
hide_on_blur = true

# Start Sevak in the background when you log in.
launch_at_login = false

# Check GitHub for a new version at startup, every six hours and when you open Sevak. Updates are only
# installed after you agree. Apart from the optional currency rates (see
# [calculator]), this is the only request Sevak makes on its own.
check_for_updates = true

[window]
# Width of the search window in logical pixels (400-1600).
width = 720

[linux]
# On Wayland sessions, draw Sevak's window through XWayland. Native Wayland
# windows cannot position themselves and may be refused focus, so this keeps the
# launcher centered and typeable. Set to false to use the native Wayland backend.
wayland_use_xwayland = true

[search]
# Number of results shown (1-20).
max_results = 8
# Keyword of the web search engine offered when nothing else matches ("" to
# disable). A list offers several, in order: ["g", "yt", "gh"].
fallback_web_search = "g"
# Up/Down on an empty search box recalls the last 50 searches you ran. They are
# kept in usage.json in the data folder; false stops recording and forgets them.
query_history = true

[appearance]
# "system", "light" or "dark".
theme = "system"
# Accent color as "#rrggbb", "#rgb" or "rgb(r, g, b)". "" keeps the theme's own.
accent = ""
# Size of the result titles in pixels (12-22); the rest of the bar scales with it.
font_size = 15
# Font for the search bar, e.g. "Fira Sans, sans-serif". "" uses the system font.
font_family = ""
# How opaque the search bar's background is, in percent (30-100).
opacity = 100
# Frosted-glass blur of the desktop behind the search bar (Windows and macOS).
# You only see it when opacity is below 100.
blur = false
# Corner radius of the search bar in pixels (0-32).
radius = 14
# A theme file inside this config folder, for example "themes/Nord.toml". Settings,
# Appearance, Theme editor creates and applies them. "" uses no theme file.
theme_file = ""
# A stylesheet inside this config folder that overrides the theme's CSS variables
# (see docs/themes.md), for example "theme.css". "" loads none.
custom_css = ""

[plugins]
# Ids of built-in plugins to turn off: "apps", "calculator", "files",
# "bookmarks", "system", "tasks", "media", "shell", "clipboard", "snippets",
# "emoji", "selection" (Universal Actions), "contacts", "1password", "dict",
# "web:<keyword>".
disabled = []

[calculator]
# Convert currencies ("100 usd in eur"). Off by default because it needs the
# network: when on, Sevak downloads the European Central Bank's daily reference
# rates (a small XML file, no account or key) in the background at most once a
# day and keeps them on disk. Unit conversion ("10 km in mi") never needs it.
currency = false

[files]
# Folders whose files and subfolders are searchable. "~" is your home folder.
directories = ["~/Desktop", "~/Documents", "~/Downloads"]
# How many folder levels below each directory are indexed.
max_depth = 4
# Index dot-files and dot-folders.
include_hidden = false
# Type "<keyword> <name>" to search only files.
keyword = "f"
# Also show (lower-ranked) file results for plain queries.
global = true
# Whole-disk search through the operating system's own index (Windows Search,
# Spotlight, locate / Tracker / Baloo). Nothing leaves your computer. Type
# "<index_keyword> <name>" for file names and "<content_keyword> <words>" for
# what is inside files. false turns both off.
use_os_index = true
index_keyword = "ff"
content_keyword = "in"

[bookmarks]
# Browsers whose bookmarks are searchable; [] means every browser found.
# Names: "chrome", "edge", "brave", "vivaldi", "chromium", "opera", "opera-gx",
# "firefox", "librewolf", "zen". All profiles of each browser are read.
browsers = []
# Type "<keyword> <text>" to search only bookmarks.
keyword = "b"
# Also show (lower-ranked) bookmark results for plain queries.
global = true

[system]
# System commands: lock, sleep, hibernate, restart, shut down, log out, empty
# the trash, and shortcuts to OS settings pages. Ask before restart, shut down,
# log out and emptying the trash.
confirm = true
# Commands or pages to hide: "lock", "sleep", "hibernate", "restart",
# "shutdown", "logout", "empty_trash", "settings" (every settings page) or
# "settings:<page>" such as "settings:bluetooth". To turn the whole plugin off,
# add "system" to [plugins] disabled instead.
disabled = []

[tasks]
# Automation tasks: toggle dark mode, show the desktop, mute and set the volume
# ("vol 30"), take a screenshot, quit an app ("quit"), kill a process by name
# ("kill chrome"), eject a drive ("eject"), keep the computer awake ("awake 30")
# and more. What is offered depends on your system.
# Ask before force quitting an app, ending a process and restarting Explorer or
# Finder.
confirm = true
# Tasks to hide: "dark_mode", "show_desktop", "hide_others", "minimize_all",
# "screenshot", "downloads", "recent_files", "flush_dns", "restart_shell",
# "empty_clipboard", "mute", "unmute", "volume_up", "volume_down", "volume",
# "wifi", "bluetooth", "keep_awake", "stop_keep_awake", "quit_app",
# "force_quit_app", "kill", "eject". To turn the whole plugin off, add "tasks"
# to [plugins] disabled instead.
disabled = []
# Type "<keyword> <task>" to search only tasks.
keyword = "t"
# Also show tasks for plain queries ("dark mode", "kill chrome").
global = true

[media]
# Media controls: play/pause, next, previous, stop, and what is playing now.
# Type "<keyword> <button>" to search only the controls.
keyword = "play"
# Also show the controls for plain queries ("pause", "next track").
global = true
# Show the track that is playing as a row (Enter plays or pauses it). It is read
# from the system's media player on request; nothing is stored or sent anywhere.
now_playing = true

[shell]
# Type "> <command>" (or ">command") to run a command in a terminal window. It
# only runs when you press Enter; recent commands are offered again.
# Terminal to use. "" detects one: Windows Terminal (else a console window) on
# Windows, Terminal.app on macOS, $TERMINAL then common terminals on Linux.
# Examples: "wt", "iterm", "kitty", "gnome-terminal", "alacritty --class sevak".
terminal = ""
# Shell that runs the command. "" picks pwsh, powershell, then cmd on Windows
# and $SHELL (or sh) on Linux. Not used on macOS: your login shell runs it.
shell = ""
# Leave the terminal open, at a shell prompt, after the command exits.
keep_open = true

[paste]
# Clipboard history and snippets paste into the app you were using before Sevak
# opened. With this on, the clipboard's previous text is put back afterwards.
restore_clipboard = false

[actions]
# Universal Actions (see actions_hotkey under [general]). Sevak presses Ctrl+C
# (Cmd+C) in the app you were using, reads the result and puts your clipboard
# back. The selection is never stored or logged. Terminal windows are skipped
# on Windows and Linux because Ctrl+C would interrupt the running program.
# Linux (X11): read the PRIMARY selection (text you just highlighted) first,
# without pressing a key.
use_primary_selection = true
# When the selection cannot be captured (Wayland, a terminal, missing macOS
# Accessibility permission), act on the current clipboard contents instead.
use_clipboard_fallback = false

[file_buffer]
# The file buffer (Alt+Up / Alt+Down on a file result collects it). By default it
# is emptied whenever the launcher hides; true keeps what you collected.
keep_between_shows = false

[clipboard]
# Clipboard history ("cb <text>"). Off by default: turning it on makes Sevak
# watch the clipboard and keep what you copy in clipboard-history.json in its
# local data folder (on Windows %LOCALAPPDATA%\sevak, which does not roam with your
# profile): text, images (as PNG files in a "clipboard" folder next to it) and
# the paths of copied files. Content that apps mark as secret (password
# managers) is never recorded.
enabled = false
# Items kept, of all kinds together (the oldest are dropped).
max_items = 200
# Longer text is not recorded.
max_item_bytes = 65536
# Record copied images, and copied files and folders (paths only).
images = true
files = true
# An image whose PNG is larger than this is not recorded.
max_image_bytes = 10485760
# Encrypt the history file and the image files for your Windows account
# (DPAPI). macOS and Linux have no such encryption here: the files are plain,
# readable by you only. Files already stored plain are encrypted on the next
# start.
encrypt = true
# Never record text copied from these apps, e.g. ["Signal", "Messages"].
# Matched case-insensitively against the program or app name.
ignore_apps = []
# Also skip password managers (KeePass, KeePassXC, 1Password, Bitwarden,
# LastPass, Dashlane, Enpass, NordPass, RoboForm, Keeper, Proton Pass), the
# system's credential prompts and ssh/gpg passphrase prompts, in addition to
# ignore_apps. The full list is in the clipboard documentation. false turns it off.
default_ignore_apps = true

[contacts]
# Search your contacts ("c <name>" or "@name"): copy an email or phone number,
# write an email, call (tel: link), or open the card. Off by default. Contacts
# are read into memory only; nothing is written to disk or sent anywhere.
enabled = false
# Keyword (the "@" keyword always works too). "" keeps the default.
keyword = "c"
# Also read the system address book: macOS Contacts (asks for permission the
# first time you use it), the Windows People store and Evolution's local
# address books on Linux.
use_system = true
# vCard files (.vcf) or folders of them, e.g. ["~/contacts.vcf", "~/Contacts"].
# This works everywhere and needs no permission.
vcard_files = []

[onepassword]
# Search your 1Password logins ("1p github"): Enter opens the item's website,
# the action panel opens it in the 1Password app or copies the username. Needs
# the official `op` command-line tool, signed in (1Password > Settings >
# Developer > "Integrate with 1Password CLI"). Only titles, websites and
# usernames are read, never passwords or one-time codes. Off by default.
enabled = false
keyword = "1p"
# Path to the `op` program. "" looks on PATH and in the usual install folders.
op_path = ""
# Which account to use when several are signed in: its address (my.1password.com),
# short name or ID. "" uses op's default.
account = ""
# How long the list of logins is kept in memory before `1p` refreshes it.
cache_minutes = 10

[dictionary]
# "define <word>" shows definitions and "spell <word>" suggests corrections, all
# offline. macOS uses its Dictionary and Windows its spell checker; elsewhere a
# bundled English dictionary (WordNet) is used. Turn it off with "dict" in
# [plugins] disabled.
define_keyword = "define"
spell_keyword = "spell"
# false always uses the bundled dictionary and word list.
use_system = true

# Snippets ("s <name>"): text you paste often. Placeholders: {date}, {time},
# {datetime}, {date:%d %B %Y}, {clipboard}, {uuid}; write {{ and }} for literal
# braces. "keyword" is optional and also matches the search.
# [[snippet]]
# name = "Email signature"
# keyword = "sig"
# text = "Best regards,\nNinad"

[snippets]
# Expand snippets as you type in any app (a snippet needs a "keyword"). OFF by
# default: while on, Sevak watches your keystrokes (in memory only, last 64
# characters, never stored or logged) to notice a keyword. See "Privacy" in the
# README. Not available on Wayland.
auto_expand = false
# Typed before every keyword, e.g. ";" so that ";sig" expands and "sig" does not.
prefix = ""
# "immediate" expands the moment the keyword is typed; "delimiter" waits for a
# space or punctuation mark, which is kept after the text.
expand_on = "immediate"
# false: "SIG" and "sig" both expand.
case_sensitive = true
# Never expand in these apps (program or app name, case-insensitive).
ignore_apps = []
# Terminal windows are skipped unless this is on.
expand_in_terminals = false

# Web search engines: type "<keyword> <terms>". "{query}" is replaced by the
# URL-encoded terms. Defining any [[web_search]] entry replaces this list.
[[web_search]]
keyword = "g"
name = "Google"
url = "https://www.google.com/search?q={query}"

[[web_search]]
keyword = "yt"
name = "YouTube"
url = "https://www.youtube.com/results?search_query={query}"

[[web_search]]
keyword = "gh"
name = "GitHub"
url = "https://github.com/search?q={query}"

# Extra global hotkeys. Each [[hotkey]] has a "key" and exactly one of:
#   query = "..."  open Sevak with this text already typed
#   run = "..."    run a result directly, without showing Sevak; the value is a
#                  result id such as "apps:firefox.desktop" or "files:<full path>"
# On Linux Wayland, `sevak --setup-hotkey` binds these in GNOME as well.
#
# [[hotkey]]
# key = "Ctrl+Alt+T"
# query = "> "
#
# [[hotkey]]
# key = "Ctrl+Alt+F"
# run = "apps:firefox.desktop"
"##;

pub const MAX_CLIPBOARD_ITEMS_LIMIT: usize = 5_000;
pub const MAX_CLIPBOARD_ITEM_BYTES_LIMIT: usize = 4 * 1024 * 1024;
pub const MAX_CLIPBOARD_IMAGE_BYTES_LIMIT: usize = 64 * 1024 * 1024;
pub const MIN_WINDOW_WIDTH: u32 = 400;
pub const MAX_WINDOW_WIDTH: u32 = 1600;
pub const MAX_RESULTS_LIMIT: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub window: WindowConfig,
    pub linux: LinuxConfig,
    pub search: SearchConfig,
    pub appearance: AppearanceConfig,
    pub plugins: PluginsConfig,
    pub calculator: CalculatorConfig,
    pub files: FilesConfig,
    pub bookmarks: BookmarksConfig,
    pub system: SystemConfig,
    pub tasks: TasksConfig,
    pub media: MediaConfig,
    pub shell: ShellConfig,
    pub paste: PasteConfig,
    pub actions: ActionsConfig,
    pub clipboard: ClipboardConfig,
    pub file_buffer: FileBufferConfig,
    pub contacts: ContactsConfig,
    pub onepassword: OnePasswordConfig,
    pub dictionary: DictionaryConfig,
    /// `[snippets]`: expanding snippet keywords as you type.
    pub snippets: SnippetsConfig,
    /// `[[snippet]]` entries. Edited by hand only: saves from the settings
    /// window leave them untouched (see `merge_document`).
    pub snippet: Vec<Snippet>,
    pub web_search: Vec<WebSearchEngine>,
    /// Extra global hotkeys (`[[hotkey]]` tables).
    #[serde(rename = "hotkey")]
    pub hotkeys: Vec<HotkeyBinding>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            window: WindowConfig::default(),
            linux: LinuxConfig::default(),
            search: SearchConfig::default(),
            appearance: AppearanceConfig::default(),
            plugins: PluginsConfig::default(),
            calculator: CalculatorConfig::default(),
            files: FilesConfig::default(),
            bookmarks: BookmarksConfig::default(),
            system: SystemConfig::default(),
            tasks: TasksConfig::default(),
            media: MediaConfig::default(),
            shell: ShellConfig::default(),
            paste: PasteConfig::default(),
            actions: ActionsConfig::default(),
            clipboard: ClipboardConfig::default(),
            file_buffer: FileBufferConfig::default(),
            contacts: ContactsConfig::default(),
            onepassword: OnePasswordConfig::default(),
            dictionary: DictionaryConfig::default(),
            snippets: SnippetsConfig::default(),
            snippet: Vec::new(),
            web_search: WebSearchEngine::defaults(),
            hotkeys: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Accelerator string, e.g. `"Super+Space"`. Parsed by the shell, because
    /// the accepted key names depend on the hotkey backend.
    pub hotkey: String,
    /// Accelerator for Universal Actions; empty turns the feature off.
    pub actions_hotkey: String,
    pub hide_on_blur: bool,
    pub launch_at_login: bool,
    /// Look for a new release at startup and daily (asks before installing).
    pub check_for_updates: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            hotkey: "Super+Space".to_owned(),
            actions_hotkey: "Ctrl+Alt+Space".to_owned(),
            hide_on_blur: true,
            launch_at_login: false,
            check_for_updates: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    /// Logical width of the launcher window.
    pub width: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self { width: 720 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinuxConfig {
    pub wayland_use_xwayland: bool,
}

impl Default for LinuxConfig {
    fn default() -> Self {
        Self {
            wayland_use_xwayland: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    pub max_results: usize,
    /// Keyword(s) of the `[[web_search]]` engines offered when nothing matched;
    /// empty disables the fallback.
    pub fallback_web_search: FallbackSearch,
    /// Remember executed queries so Up/Down can recall them.
    pub query_history: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_results: 8,
            fallback_web_search: FallbackSearch::single("g"),
            query_history: true,
        }
    }
}

/// `fallback_web_search`: one keyword written as a string (`"g"`, the original
/// form) or several written as a list (`["g", "yt"]`). Both forms read and
/// write back as they were written, so saving never rewrites the user's choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackSearch {
    keywords: Vec<String>,
    /// Written as a list rather than a string.
    list: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum FallbackRepr {
    One(String),
    Many(Vec<String>),
}

impl FallbackSearch {
    /// One keyword in string form; `""` disables the fallback.
    pub fn single(keyword: impl Into<String>) -> Self {
        Self::from_repr(FallbackRepr::One(keyword.into()))
    }

    /// Several keywords in list form, tried in this order.
    pub fn list(keywords: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::from_repr(FallbackRepr::Many(
            keywords.into_iter().map(Into::into).collect(),
        ))
    }

    /// The keywords in order: trimmed, without blanks or case-insensitive repeats.
    pub fn keywords(&self) -> &[String] {
        &self.keywords
    }

    pub fn is_empty(&self) -> bool {
        self.keywords.is_empty()
    }

    fn from_repr(repr: FallbackRepr) -> Self {
        let (raw, list) = match repr {
            FallbackRepr::One(keyword) => (vec![keyword], false),
            FallbackRepr::Many(keywords) => (keywords, true),
        };
        let mut keywords: Vec<String> = Vec::new();
        for keyword in raw {
            let keyword = keyword.trim();
            let repeated = keywords.iter().any(|k| k.eq_ignore_ascii_case(keyword));
            if !keyword.is_empty() && !repeated {
                keywords.push(keyword.to_owned());
            }
        }
        Self { keywords, list }
    }
}

impl Serialize for FallbackSearch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.list {
            self.keywords.serialize(serializer)
        } else {
            self.keywords
                .first()
                .map_or("", String::as_str)
                .serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for FallbackSearch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        FallbackRepr::deserialize(deserializer).map(Self::from_repr)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Appearance settings. Values are stored as written; [`crate::theme::resolve`]
/// validates them (falling back to the defaults) when they are applied, so a typo
/// never costs the user the rest of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    pub theme: Theme,
    /// `#rgb`, `#rrggbb` or `rgb(r, g, b)`; empty keeps the theme's accent.
    pub accent: String,
    /// Pixel size of result titles.
    pub font_size: u32,
    /// Comma-separated font families; empty uses the system font.
    pub font_family: String,
    /// Background opacity of the search bar, in percent.
    pub opacity: u32,
    /// Frosted-glass blur behind the search bar (Windows and macOS). Only
    /// visible when `opacity` is below 100.
    pub blur: bool,
    /// Corner radius of the search bar, in pixels.
    pub radius: u32,
    /// Theme file inside the config directory (`themes/Nord.toml`); empty uses none.
    pub theme_file: String,
    /// Stylesheet inside the config directory; empty loads none.
    pub custom_css: String,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            accent: String::new(),
            font_size: crate::theme::DEFAULT_FONT_SIZE,
            font_family: String::new(),
            opacity: crate::theme::MAX_OPACITY,
            blur: false,
            radius: crate::theme::DEFAULT_RADIUS,
            theme_file: String::new(),
            custom_css: String::new(),
        }
    }
}

/// One `[[hotkey]]` entry: a global key bound to a query or a result.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyBinding {
    /// Accelerator string, parsed like `general.hotkey`.
    pub key: String,
    /// Open Sevak with this text typed in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Run the result with this id without showing Sevak.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

/// What a [`HotkeyBinding`] does when its key is pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyTarget {
    Query(String),
    Run(String),
}

impl HotkeyBinding {
    /// The binding's action, or why it has none (both or neither of `query`
    /// and `run` given, or an empty `run`).
    pub fn target(&self) -> Result<HotkeyTarget, &'static str> {
        match (&self.query, &self.run) {
            (Some(_), Some(_)) => Err("set either \"query\" or \"run\", not both"),
            (None, None) => Err("set \"query\" or \"run\""),
            (Some(query), None) => Ok(HotkeyTarget::Query(query.clone())),
            (None, Some(run)) if run.trim().is_empty() => Err("\"run\" is empty"),
            (None, Some(run)) => Ok(HotkeyTarget::Run(run.trim().to_owned())),
        }
    }

    /// Short description for logs and the settings window.
    pub fn describe(&self) -> String {
        match self.target() {
            Ok(HotkeyTarget::Query(query)) if query.is_empty() => "Open Sevak".to_owned(),
            Ok(HotkeyTarget::Query(query)) => format!("Open Sevak with \"{query}\""),
            Ok(HotkeyTarget::Run(id)) => format!("Run {id}"),
            Err(reason) => format!("Invalid ({reason})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginsConfig {
    /// Plugin ids that are not loaded.
    pub disabled: Vec<String>,
}

impl PluginsConfig {
    pub fn is_enabled(&self, plugin_id: &str) -> bool {
        !self.disabled.iter().any(|id| id == plugin_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CalculatorConfig {
    /// Convert fiat currencies with the ECB's daily reference rates. Off by
    /// default: it is the one calculator feature that uses the network.
    pub currency: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesConfig {
    /// Roots to index; a leading `~` means the home directory (expanded by the
    /// files plugin, since core has no notion of the user's home).
    pub directories: Vec<String>,
    pub max_depth: usize,
    pub include_hidden: bool,
    pub keyword: String,
    pub global: bool,
    /// Search the whole disk through the OS index (`index_keyword`,
    /// `content_keyword`). The folder index above still answers `keyword` and
    /// plain queries, and stands in when the OS index cannot be reached.
    pub use_os_index: bool,
    /// Keyword for whole-disk file-name search; empty turns it off.
    pub index_keyword: String,
    /// Keyword for searching inside files; empty turns it off.
    pub content_keyword: String,
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self {
            directories: vec![
                "~/Desktop".to_owned(),
                "~/Documents".to_owned(),
                "~/Downloads".to_owned(),
            ],
            max_depth: 4,
            include_hidden: false,
            keyword: "f".to_owned(),
            global: true,
            use_os_index: true,
            index_keyword: "ff".to_owned(),
            content_keyword: "in".to_owned(),
        }
    }
}

/// The `>` shell command plugin: which terminal and shell run the command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    /// Terminal program (a name on `PATH` or a full path, optionally followed
    /// by extra arguments) or a well-known name such as `iterm`. Empty means
    /// auto-detect.
    pub terminal: String,
    /// Shell program that runs the command; empty means auto-detect. Unused on
    /// macOS, where the terminal starts the user's login shell itself.
    pub shell: String,
    /// Keep the terminal open, at a shell prompt, after the command exits.
    pub keep_open: bool,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            terminal: String::new(),
            shell: String::new(),
            keep_open: true,
        }
    }
}

/// How text is pasted into the previously focused app.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PasteConfig {
    /// Put the clipboard's previous text back after pasting.
    pub restore_clipboard: bool,
}

/// Universal Actions: how the selection in another app is captured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionsConfig {
    /// Linux (X11): read the PRIMARY selection (highlighted text) first,
    /// without pressing a key.
    pub use_primary_selection: bool,
    /// Act on the clipboard's contents when the selection cannot be captured.
    pub use_clipboard_fallback: bool,
}

impl Default for ActionsConfig {
    fn default() -> Self {
        Self {
            use_primary_selection: true,
            use_clipboard_fallback: false,
        }
    }
}

/// The file buffer: files collected from the results to act on together.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileBufferConfig {
    /// Keep the collected files when the launcher hides (else it is emptied).
    pub keep_between_shows: bool,
}

/// Apps whose copies the clipboard history never records, unless
/// `[clipboard] default_ignore_apps = false`: password managers and the tools
/// that ask for a password or passphrase (credential prompts, `ssh` askpass and
/// `pinentry` programs, key agents).
///
/// Each entry is compared, ignoring case and a `.exe` / `.app` ending, with the
/// name the system reports for the app in front: the program name on Windows
/// (`KeePassXC.exe`), the app name or bundle id on macOS, the window class or
/// program name on Linux. It is a best-effort list: an app that is not on it,
/// or that reports another name, is not covered (add it to `ignore_apps`).
pub const DEFAULT_CLIPBOARD_IGNORE_APPS: &[&str] = &[
    // Password managers.
    "KeePass",
    "KeePassXC",
    "org.keepassxc.KeePassXC",
    "keepassx",
    "1Password",
    "1Password 7",
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.agilebits.onepassword-osx",
    "Bitwarden",
    "com.bitwarden.desktop",
    "LastPass",
    "com.lastpass.lastpass",
    "Dashlane",
    "com.dashlane.dashlanephonefinal",
    "Enpass",
    "in.sinew.Enpass-Desktop",
    "NordPass",
    "RoboForm",
    "Keeper",
    "KeeperPasswordManager",
    "Proton Pass",
    "ProtonPass",
    "Authy Desktop",
    "WinAuth",
    "org.gnome.World.Secrets",
    "seahorse",
    "kwalletmanager",
    "kwalletmanager5",
    // The system's own credential prompts.
    "CredentialUIBroker",
    "consent",
    "LogonUI",
    "Keychain Access",
    "com.apple.keychainaccess",
    "com.apple.Passwords",
    "SecurityAgent",
    "com.apple.SecurityAgent",
    "gcr-prompter",
    // Passphrase prompts of ssh, gpg and their agents.
    "ssh-askpass",
    "x11-ssh-askpass",
    "gnome-ssh-askpass",
    "ssh-askpass-gnome",
    "ksshaskpass",
    "lxqt-openssh-askpass",
    "pinentry",
    "pinentry-gtk",
    "pinentry-gtk-2",
    "pinentry-gnome3",
    "pinentry-qt",
    "pinentry-x11",
    "pinentry-mac",
    "pinentry-curses",
    "pinentry-tty",
    "pageant",
    "puttygen",
];

/// The clipboard history plugin (`cb`). Opt-in: nothing is watched or stored
/// unless `enabled` is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardConfig {
    pub enabled: bool,
    pub max_items: usize,
    /// Text longer than this many bytes is not recorded.
    pub max_item_bytes: usize,
    /// Apps whose copies are never recorded (program or app names).
    pub ignore_apps: Vec<String>,
    /// Also never record copies from the password managers and secret-handling
    /// tools in [`DEFAULT_CLIPBOARD_IGNORE_APPS`], in addition to `ignore_apps`.
    pub default_ignore_apps: bool,
    /// Record copied images (as PNG files next to the history).
    pub images: bool,
    /// Record copied files and folders (their paths; the files stay where they are).
    pub files: bool,
    /// An image whose PNG is larger than this many bytes is not recorded.
    pub max_image_bytes: usize,
    /// Encrypt the history file and the image files at rest for the current
    /// user, where the system can (Windows: DPAPI). Elsewhere the files are
    /// plain but readable by the owner only.
    pub encrypt: bool,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_items: 200,
            max_item_bytes: 64 * 1024,
            ignore_apps: Vec::new(),
            default_ignore_apps: true,
            images: true,
            files: true,
            max_image_bytes: 10 * 1024 * 1024,
            encrypt: true,
        }
    }
}

/// The contacts plugin (`c` / `@`). Opt-in: nothing is read unless `enabled`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContactsConfig {
    pub enabled: bool,
    pub keyword: String,
    /// Also read the system address book (macOS Contacts, Windows People,
    /// Evolution on Linux).
    pub use_system: bool,
    /// `.vcf` files and folders of them; `~` is the home folder.
    pub vcard_files: Vec<String>,
}

impl Default for ContactsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            keyword: "c".to_owned(),
            use_system: true,
            vcard_files: Vec::new(),
        }
    }
}

/// The 1Password plugin (`1p`). Opt-in: the `op` tool is never started unless
/// `enabled`, and then only when the user types the keyword.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OnePasswordConfig {
    pub enabled: bool,
    pub keyword: String,
    /// Path to `op`; empty searches `PATH` and the usual install folders.
    pub op_path: String,
    /// Account address, shorthand or ID passed to `op --account`; empty uses
    /// op's default.
    pub account: String,
    /// Minutes the list of logins is kept before it is refreshed.
    pub cache_minutes: u32,
}

impl Default for OnePasswordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            keyword: "1p".to_owned(),
            op_path: String::new(),
            account: String::new(),
            cache_minutes: 10,
        }
    }
}

/// The dictionary plugin (`define`, `spell`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DictionaryConfig {
    pub define_keyword: String,
    pub spell_keyword: String,
    /// Prefer the OS dictionary and spell checker where there is one.
    pub use_system: bool,
}

impl Default for DictionaryConfig {
    fn default() -> Self {
        Self {
            define_keyword: "define".to_owned(),
            spell_keyword: "spell".to_owned(),
            use_system: true,
        }
    }
}

/// When a typed keyword is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExpandOn {
    /// The moment the last character of the keyword is typed.
    #[default]
    Immediate,
    /// When a space or punctuation mark follows the keyword. Also what any
    /// unrecognised value in the file means: the cautious choice.
    #[serde(other)]
    Delimiter,
}

/// Expanding `[[snippet]]` keywords as you type, in any app. Opt-in: no key is
/// observed unless `auto_expand` is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnippetsConfig {
    pub auto_expand: bool,
    /// Put in front of every keyword (`;` makes `;sig` expand and `sig` not).
    pub prefix: String,
    pub expand_on: ExpandOn,
    pub case_sensitive: bool,
    /// Apps in which nothing is observed or expanded (program or app names).
    pub ignore_apps: Vec<String>,
    /// Expand in terminal windows too.
    pub expand_in_terminals: bool,
}

impl Default for SnippetsConfig {
    fn default() -> Self {
        Self {
            auto_expand: false,
            prefix: String::new(),
            expand_on: ExpandOn::Immediate,
            case_sensitive: true,
            ignore_apps: Vec::new(),
            expand_in_terminals: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemConfig {
    /// Ask before restart, shut down, log out and emptying the trash.
    pub confirm: bool,
    /// Commands and settings pages to hide, by key (`restart`, `settings`,
    /// `settings:bluetooth`, ...). Matched by the system plugin.
    pub disabled: Vec<String>,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            confirm: true,
            disabled: Vec::new(),
        }
    }
}

impl SystemConfig {
    /// Whether `key` is switched off in `disabled` (case-insensitive).
    pub fn is_disabled(&self, key: &str) -> bool {
        self.disabled
            .iter()
            .any(|entry| entry.trim().eq_ignore_ascii_case(key))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TasksConfig {
    /// Ask before force quitting an app, ending a process and restarting
    /// Explorer or Finder.
    pub confirm: bool,
    /// Tasks to hide, by key (`dark_mode`, `kill`, `wifi`, ...). Matched by the
    /// tasks plugin.
    pub disabled: Vec<String>,
    /// Type "<keyword> <task>" to search only tasks.
    pub keyword: String,
    /// Also show tasks for plain queries (`dark mode`, `kill chrome`).
    pub global: bool,
}

impl Default for TasksConfig {
    fn default() -> Self {
        Self {
            confirm: true,
            disabled: Vec::new(),
            keyword: "t".to_owned(),
            global: true,
        }
    }
}

impl TasksConfig {
    /// Whether `key` is switched off in `disabled` (case-insensitive).
    pub fn is_disabled(&self, key: &str) -> bool {
        self.disabled
            .iter()
            .any(|entry| entry.trim().eq_ignore_ascii_case(key))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaConfig {
    /// Type "<keyword> <button>" to search only the media controls.
    pub keyword: String,
    /// Also show the controls for plain queries (`pause`, `next`).
    pub global: bool,
    /// Show the playing track as a row. Reads it from the player the system
    /// reports; nothing is stored or sent anywhere.
    pub now_playing: bool,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            keyword: "play".to_owned(),
            global: true,
            now_playing: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BookmarksConfig {
    /// Browser ids to read (`chrome`, `firefox`, ...); empty means every
    /// browser found. The ids are defined by the platform layer.
    pub browsers: Vec<String>,
    pub keyword: String,
    pub global: bool,
}

impl Default for BookmarksConfig {
    fn default() -> Self {
        Self {
            browsers: Vec::new(),
            keyword: "b".to_owned(),
            global: true,
        }
    }
}

/// One `[[snippet]]`: text pasted on demand, with placeholders expanded.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snippet {
    pub name: String,
    /// Extra word the snippet is found by (`s sig`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchEngine {
    pub keyword: String,
    pub name: String,
    /// URL template; `{query}` is replaced by the URL-encoded search terms.
    pub url: String,
}

impl WebSearchEngine {
    pub fn defaults() -> Vec<Self> {
        [
            ("g", "Google", "https://www.google.com/search?q={query}"),
            (
                "yt",
                "YouTube",
                "https://www.youtube.com/results?search_query={query}",
            ),
            ("gh", "GitHub", "https://github.com/search?q={query}"),
        ]
        .into_iter()
        .map(|(keyword, name, url)| Self {
            keyword: keyword.to_owned(),
            name: name.to_owned(),
            url: url.to_owned(),
        })
        .collect()
    }
}

/// How the configuration returned by [`Config::load_or_create`] was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigOrigin {
    /// No file existed; defaults were written to disk.
    Created,
    /// An existing file was parsed.
    Loaded,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot access config file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid config file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("cannot update config file {path}: it is not valid TOML: {source}")]
    Edit {
        path: PathBuf,
        #[source]
        source: Box<toml_edit::TomlError>,
    },
    #[error("cannot serialize the configuration: {0}")]
    Serialize(#[from] toml_edit::ser::Error),
}

impl Config {
    /// Parses TOML text and normalizes out-of-range values.
    pub fn from_toml_str(text: &str) -> Result<Self, toml::de::Error> {
        let config: Config = toml::from_str(text)?;
        Ok(config.normalized())
    }

    /// Loads the config at `path`, writing [`DEFAULT_CONFIG_TOML`] there first
    /// if the file does not exist yet.
    ///
    /// A file that exists but fails to parse is reported as an error and left
    /// untouched, so a typo never costs the user their settings.
    pub fn load_or_create(path: &Path) -> Result<(Self, ConfigOrigin), ConfigError> {
        let io_err = |source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        };

        match fs::read_to_string(path) {
            Ok(text) => {
                let config = Self::from_toml_str(&text).map_err(|source| ConfigError::Parse {
                    path: path.to_path_buf(),
                    source: Box::new(source),
                })?;
                Ok((config, ConfigOrigin::Loaded))
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(io_err)?;
                }
                fs::write(path, DEFAULT_CONFIG_TOML).map_err(io_err)?;
                Ok((Self::default(), ConfigOrigin::Created))
            }
            Err(err) => Err(io_err(err)),
        }
    }

    /// Writes this configuration to `path`, editing the user's existing document
    /// in place so their comments, key order and untouched values survive.
    ///
    /// - A missing file is created from [`DEFAULT_CONFIG_TOML`] first.
    /// - Scalar values and arrays are replaced only when they differ, keeping
    ///   the key's comments and any trailing inline comment.
    /// - Keys Sevak does not know are left alone.
    /// - `[[web_search]]` is rewritten as a whole (it is a list, so there is no
    ///   meaningful per-entry merge); the comment above its first entry stays.
    /// - The file is replaced atomically (temporary file + rename).
    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        let io_err = |source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        };

        let existing = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => DEFAULT_CONFIG_TOML.to_owned(),
            Err(err) => return Err(io_err(err)),
        };
        let mut document = existing
            .parse::<toml_edit::DocumentMut>()
            .map_err(|source| ConfigError::Edit {
                path: path.to_path_buf(),
                source: Box::new(source),
            })?;

        let updated = toml_edit::ser::to_document(self)?;
        merge_document(&mut document, &updated);
        let mut text = document.to_string();
        // The parser normalizes line endings to LF; give a CRLF file its own back.
        if existing.contains("\r\n") {
            text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_err)?;
        }
        let mut temp_name = path.file_name().unwrap_or_default().to_owned();
        temp_name.push(".tmp");
        let temp = path.with_file_name(temp_name);
        let written = fs::write(&temp, text).and_then(|()| fs::rename(&temp, path));
        if let Err(err) = written {
            let _ = fs::remove_file(&temp);
            return Err(io_err(err));
        }
        Ok(())
    }

    /// Clamps values into their supported ranges.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.window.width = self.window.width.clamp(MIN_WINDOW_WIDTH, MAX_WINDOW_WIDTH);
        self.search.max_results = self.search.max_results.clamp(1, MAX_RESULTS_LIMIT);
        // Engines without a keyword or a `{query}` placeholder cannot work.
        self.web_search
            .retain(|engine| !engine.keyword.trim().is_empty() && engine.url.contains("{query}"));
        self.shell.terminal = self.shell.terminal.trim().to_owned();
        self.shell.shell = self.shell.shell.trim().to_owned();
        self.clipboard.max_items = self.clipboard.max_items.clamp(1, MAX_CLIPBOARD_ITEMS_LIMIT);
        self.clipboard.max_item_bytes = self
            .clipboard
            .max_item_bytes
            .clamp(1, MAX_CLIPBOARD_ITEM_BYTES_LIMIT);
        self.clipboard.max_image_bytes = self
            .clipboard
            .max_image_bytes
            .clamp(1, MAX_CLIPBOARD_IMAGE_BYTES_LIMIT);
        self.clipboard
            .ignore_apps
            .retain(|app| !app.trim().is_empty());
        for (keyword, default) in [
            (&mut self.contacts.keyword, "c"),
            (&mut self.onepassword.keyword, "1p"),
            (&mut self.dictionary.define_keyword, "define"),
            (&mut self.dictionary.spell_keyword, "spell"),
        ] {
            *keyword = keyword.trim().to_owned();
            if keyword.is_empty() || keyword.contains(char::is_whitespace) {
                *keyword = default.to_owned();
            }
        }
        self.contacts
            .vcard_files
            .retain(|path| !path.trim().is_empty());
        self.onepassword.op_path = self.onepassword.op_path.trim().to_owned();
        self.onepassword.account = self.onepassword.account.trim().to_owned();
        self.onepassword.cache_minutes = self.onepassword.cache_minutes.clamp(1, 24 * 60);
        self.snippets.prefix = self.snippets.prefix.trim().to_owned();
        self.snippets
            .ignore_apps
            .retain(|app| !app.trim().is_empty());
        // A snippet needs a name to be found by and text to paste.
        self.snippet
            .retain(|snippet| !snippet.name.trim().is_empty() && !snippet.text.is_empty());
        for snippet in &mut self.snippet {
            snippet.keyword = snippet
                .keyword
                .take()
                .map(|keyword| keyword.trim().to_owned())
                .filter(|keyword| !keyword.is_empty());
        }
        let hotkey = self.general.hotkey.trim();
        self.general.hotkey = if hotkey.is_empty() {
            GeneralConfig::default().hotkey
        } else {
            hotkey.to_owned()
        };
        self.general.actions_hotkey = self.general.actions_hotkey.trim().to_owned();
        // An entry without a key cannot be reported against anything.
        for binding in &mut self.hotkeys {
            binding.key = binding.key.trim().to_owned();
        }
        self.hotkeys.retain(|binding| !binding.key.is_empty());
        self
    }
}

/// Keys of the arrays of tables in the schema.
const WEB_SEARCH_KEY: &str = "web_search";
/// `[[snippet]]` is edited by hand only; the settings window never changes it,
/// so saving leaves the user's entries exactly as written.
const SNIPPET_KEY: &str = "snippet";
const HOTKEY_KEY: &str = "hotkey";

/// Applies `updated` (a freshly serialized config) onto `document`.
fn merge_document(document: &mut toml_edit::DocumentMut, updated: &toml_edit::DocumentMut) {
    use toml_edit::Item;

    for (key, new_item) in updated.as_table() {
        if key == SNIPPET_KEY {
            continue;
        }
        if key == WEB_SEARCH_KEY {
            // The defaults come back when the key is absent, so "none" is written out.
            merge_table_list(document.as_table_mut(), key, new_item, true);
            continue;
        }
        if key == HOTKEY_KEY {
            merge_table_list(document.as_table_mut(), key, new_item, false);
            continue;
        }
        // The serializer emits sections as inline tables; edit them as tables.
        let new_table = match new_item {
            Item::Table(table) => Some(table.clone()),
            other => other.clone().into_table().ok(),
        };
        let Some(new_table) = new_table else {
            merge_item(document.as_table_mut(), key, new_item);
            continue;
        };
        match document.get_mut(key) {
            Some(Item::Table(table)) => merge_table(table, &new_table),
            // Missing, or not a table: write the section fresh.
            _ => {
                let mut table = new_table;
                table.set_implicit(false);
                document.insert(key, Item::Table(table));
            }
        }
    }
}

fn merge_table(table: &mut toml_edit::Table, new_table: &toml_edit::Table) {
    for (key, new_item) in new_table {
        merge_item(table, key, new_item);
    }
}

/// Sets `table[key]` to `new_item` unless it already holds an equal value.
fn merge_item(table: &mut toml_edit::Table, key: &str, new_item: &toml_edit::Item) {
    let Some(new_value) = new_item.as_value() else {
        return;
    };
    match table.get_mut(key) {
        Some(existing) => {
            if existing
                .as_value()
                .is_some_and(|old| values_equal(old, new_value))
            {
                return;
            }
            let mut replacement = new_value.clone();
            // Keep the spacing and trailing `# comment` of the old value.
            if let Some(old) = existing.as_value() {
                *replacement.decor_mut() = old.decor().clone();
            }
            *existing = toml_edit::Item::Value(replacement);
        }
        None => {
            table.insert(key, toml_edit::Item::Value(new_value.clone()));
        }
    }
}

/// Replaces the array of tables `key` as a whole. An empty list is written as
/// `key = []` when `keep_empty` is set, and removed otherwise.
fn merge_table_list(
    root: &mut toml_edit::Table,
    key: &str,
    new_item: &toml_edit::Item,
    keep_empty: bool,
) {
    use toml_edit::Item;

    let mut new_engines = match new_item {
        Item::ArrayOfTables(tables) => tables.clone(),
        other => other
            .clone()
            .into_array_of_tables()
            .unwrap_or_else(|_| toml_edit::ArrayOfTables::new()),
    };

    let unchanged = match root.get(key) {
        Some(Item::ArrayOfTables(old)) => {
            old.len() == new_engines.len()
                && old
                    .iter()
                    .zip(new_engines.iter())
                    .all(|(a, b)| tables_equal(a, b))
        }
        Some(Item::Value(toml_edit::Value::Array(old))) => old.is_empty() && new_engines.is_empty(),
        None => new_engines.is_empty() && !keep_empty,
        _ => false,
    };
    if unchanged {
        return;
    }

    // An empty list cannot be written as `[[...]]` tables.
    if new_engines.is_empty() {
        if keep_empty {
            // Omitting the key would bring the defaults back on the next load.
            root.insert(
                key,
                Item::Value(toml_edit::Value::Array(toml_edit::Array::new())),
            );
        } else {
            root.remove(key);
        }
        return;
    }

    // The comment block above the first `[[...]]` belongs to the list.
    let leading_decor = match root.get(key) {
        Some(Item::ArrayOfTables(old)) => old.iter().next().map(|t| t.decor().clone()),
        _ => None,
    };
    match (leading_decor, new_engines.iter_mut().next()) {
        (Some(decor), Some(first)) => *first.decor_mut() = decor,
        // A list new to the file: set it apart from what precedes it.
        (None, Some(first)) => first.decor_mut().set_prefix("\n"),
        _ => {}
    }
    root.insert(key, Item::ArrayOfTables(new_engines));
}

fn tables_equal(a: &toml_edit::Table, b: &toml_edit::Table) -> bool {
    a.len() == b.len()
        && a.iter().all(|(key, item)| {
            match (
                item.as_value(),
                b.get(key).and_then(toml_edit::Item::as_value),
            ) {
                (Some(x), Some(y)) => values_equal(x, y),
                _ => false,
            }
        })
}

/// Semantic equality: formatting and comments are ignored.
fn values_equal(a: &toml_edit::Value, b: &toml_edit::Value) -> bool {
    use toml_edit::Value;
    match (a, b) {
        (Value::String(x), Value::String(y)) => x.value() == y.value(),
        (Value::Integer(x), Value::Integer(y)) => x.value() == y.value(),
        (Value::Float(x), Value::Float(y)) => x.value() == y.value(),
        (Value::Boolean(x), Value::Boolean(y)) => x.value() == y.value(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| values_equal(p, q))
        }
        (Value::InlineTable(x), Value::InlineTable(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(key, p)| y.get(key).is_some_and(|q| values_equal(p, q)))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_template_matches_default_struct() {
        let parsed = Config::from_toml_str(DEFAULT_CONFIG_TOML).expect("template parses");
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn super_space_is_the_default_and_existing_files_keep_their_key() {
        assert_eq!(Config::default().general.hotkey, "Super+Space");
        assert!(DEFAULT_CONFIG_TOML.contains("hotkey = \"Super+Space\""));

        // A config written by an earlier version still says Alt+Space.
        let existing = "[general]\nhotkey = \"Alt+Space\"\n";
        let old = Config::from_toml_str(existing).unwrap();
        assert_eq!(old.general.hotkey, "Alt+Space");
        // Saving it unchanged does not rewrite the key.
        let text = saved(Some(existing), &old);
        assert!(text.contains("hotkey = \"Alt+Space\""), "{text}");
        assert!(!text.contains("Super+Space"), "{text}");
        assert_eq!(
            Config::from_toml_str(&text).unwrap().general.hotkey,
            "Alt+Space"
        );
    }

    #[test]
    fn integrations_are_opt_in_and_normalized() {
        let defaults = Config::default();
        assert!(!defaults.contacts.enabled);
        assert!(!defaults.onepassword.enabled);
        assert_eq!(defaults.dictionary.define_keyword, "define");

        let config = Config::from_toml_str(
            "[contacts]\nenabled = true\nkeyword = \"  \"\nvcard_files = [\"a.vcf\", \" \"]\n\
             [onepassword]\nkeyword = \"two words\"\ncache_minutes = 0\nop_path = \" /bin/op \"\n",
        )
        .unwrap()
        .normalized();
        assert!(config.contacts.enabled);
        assert_eq!(config.contacts.keyword, "c");
        assert_eq!(config.contacts.vcard_files, ["a.vcf"]);
        assert_eq!(config.onepassword.keyword, "1p");
        assert_eq!(config.onepassword.cache_minutes, 1);
        assert_eq!(config.onepassword.op_path, "/bin/op");
    }

    #[test]
    fn empty_file_yields_defaults() {
        assert_eq!(Config::from_toml_str("").unwrap(), Config::default());
    }

    #[test]
    fn partial_file_fills_missing_fields() {
        let config = Config::from_toml_str("[general]\nhotkey = \"Ctrl+Space\"\n").unwrap();
        assert_eq!(config.general.hotkey, "Ctrl+Space");
        assert!(config.general.hide_on_blur);
        assert_eq!(config.window, WindowConfig::default());
        assert_eq!(config.linux, LinuxConfig::default());
        assert_eq!(config.web_search, WebSearchEngine::defaults());
    }

    #[test]
    fn web_search_entries_replace_defaults_and_invalid_ones_are_dropped() {
        let config = Config::from_toml_str(
            r#"
[[web_search]]
keyword = "ddg"
name = "DuckDuckGo"
url = "https://duckduckgo.com/?q={query}"

[[web_search]]
keyword = "x"
name = "Broken"
url = "https://example.com"
"#,
        )
        .unwrap();
        assert_eq!(config.web_search.len(), 1);
        assert_eq!(config.web_search[0].keyword, "ddg");
    }

    #[test]
    fn theme_and_plugin_toggles_parse() {
        let config = Config::from_toml_str(
            "[appearance]\ntheme = \"dark\"\n[plugins]\ndisabled = [\"files\"]\n",
        )
        .unwrap();
        assert_eq!(config.appearance.theme, Theme::Dark);
        assert!(!config.plugins.is_enabled("files"));
        assert!(config.plugins.is_enabled("apps"));
    }

    #[test]
    fn bookmarks_section_parses_with_defaults() {
        let config = Config::from_toml_str(
            "[bookmarks]\nbrowsers = [\"firefox\", \"chrome\"]\nkeyword = \"bm\"\n",
        )
        .unwrap();
        assert_eq!(config.bookmarks.browsers, ["firefox", "chrome"]);
        assert_eq!(config.bookmarks.keyword, "bm");
        assert!(config.bookmarks.global);

        let config = Config::from_toml_str("").unwrap();
        assert!(config.bookmarks.browsers.is_empty());
        assert_eq!(config.bookmarks.keyword, "b");
    }

    #[test]
    fn os_index_search_is_on_with_its_own_keywords() {
        let config = Config::from_toml_str("").unwrap();
        assert!(config.files.use_os_index);
        assert_eq!(config.files.index_keyword, "ff");
        assert_eq!(config.files.content_keyword, "in");

        // A files section from before the OS index existed keeps working.
        let config = Config::from_toml_str("[files]\nkeyword = \"x\"\n").unwrap();
        assert!(config.files.use_os_index);
        assert_eq!(config.files.index_keyword, "ff");

        let config = Config::from_toml_str(
            "[files]\nuse_os_index = false\nindex_keyword = \"all\"\ncontent_keyword = \"\"\n",
        )
        .unwrap();
        assert!(!config.files.use_os_index);
        assert_eq!(config.files.index_keyword, "all");
        assert_eq!(config.files.content_keyword, "");
    }

    #[test]
    fn tasks_and_media_sections_have_defaults_and_parse() {
        let config = Config::from_toml_str("").unwrap();
        assert!(config.tasks.confirm && config.tasks.global);
        assert_eq!(config.tasks.keyword, "t");
        assert!(config.tasks.disabled.is_empty());
        assert_eq!(config.media.keyword, "play");
        assert!(config.media.global && config.media.now_playing);

        let config = Config::from_toml_str(
            "[tasks]
confirm = false
keyword = \"tk\"
disabled = [\"Kill\"]
[media]
now_playing = false
",
        )
        .unwrap();
        assert!(!config.tasks.confirm);
        assert_eq!(config.tasks.keyword, "tk");
        assert!(config.tasks.is_disabled("kill"));
        assert!(!config.tasks.is_disabled("wifi"));
        assert!(!config.media.now_playing);
        assert_eq!(config.media.keyword, "play");
    }

    #[test]
    fn system_section_defaults_to_confirming_everything_enabled() {
        let config = Config::from_toml_str("").unwrap();
        assert!(config.system.confirm);
        assert!(config.system.disabled.is_empty());
    }

    #[test]
    fn system_section_parses_and_matches_keys_ignoring_case() {
        let config = Config::from_toml_str(
            "[system]\nconfirm = false\ndisabled = [\"Restart\", \"settings:wifi\"]\n",
        )
        .unwrap();
        assert!(!config.system.confirm);
        assert!(config.system.is_disabled("restart"));
        assert!(config.system.is_disabled("settings:wifi"));
        assert!(!config.system.is_disabled("shutdown"));
    }

    #[test]
    fn shell_section_defaults_and_parses() {
        let defaults = Config::default().shell;
        assert_eq!(defaults.terminal, "");
        assert_eq!(defaults.shell, "");
        assert!(defaults.keep_open);

        let config = Config::from_toml_str(
            "[shell]\nterminal = \"  kitty --single-instance \"\nkeep_open = false\n",
        )
        .unwrap();
        assert_eq!(config.shell.terminal, "kitty --single-instance");
        assert_eq!(config.shell.shell, "");
        assert!(!config.shell.keep_open);
    }

    #[test]
    fn currency_conversion_is_off_unless_enabled() {
        assert!(!Config::default().calculator.currency);
        let config = Config::from_toml_str("[calculator]\ncurrency = true\n").unwrap();
        assert!(config.calculator.currency);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let config = Config::from_toml_str("[future]\nthing = 1\n[general]\nnew_key = true\n");
        assert_eq!(config.unwrap(), Config::default());
    }

    #[test]
    fn clipboard_is_opt_in_and_paste_keeps_the_clipboard_by_default() {
        let config = Config::default();
        assert!(!config.clipboard.enabled);
        assert_eq!(config.clipboard.max_items, 200);
        assert!(!config.paste.restore_clipboard);
        assert!(config.snippet.is_empty());
    }

    #[test]
    fn the_file_buffer_is_emptied_when_the_window_hides_unless_asked() {
        assert!(!Config::default().file_buffer.keep_between_shows);
        let config = Config::from_toml_str(
            "[file_buffer]
keep_between_shows = true
",
        )
        .unwrap();
        assert!(config.file_buffer.keep_between_shows);
    }

    #[test]
    fn clipboard_and_paste_sections_parse_and_are_clamped() {
        let config = Config::from_toml_str(
            "[paste]\nrestore_clipboard = true\n[clipboard]\nenabled = true\nmax_items = 0\n\
             max_item_bytes = 999999999\nignore_apps = [\"KeePassXC\", \"  \"]\n",
        )
        .unwrap();
        assert!(config.paste.restore_clipboard);
        assert!(config.clipboard.enabled);
        assert_eq!(config.clipboard.max_items, 1);
        assert_eq!(
            config.clipboard.max_item_bytes,
            MAX_CLIPBOARD_ITEM_BYTES_LIMIT
        );
        assert_eq!(config.clipboard.ignore_apps, ["KeePassXC"]);
    }

    #[test]
    fn clipboard_records_images_and_files_unless_turned_off() {
        let config = Config::default();
        assert!(config.clipboard.images && config.clipboard.files);
        assert_eq!(config.clipboard.max_image_bytes, 10 * 1024 * 1024);

        // A config written before these keys existed keeps working.
        let old = Config::from_toml_str("[clipboard]\nenabled = true\n").unwrap();
        assert!(old.clipboard.images && old.clipboard.files);

        let config = Config::from_toml_str(
            "[clipboard]\nimages = false\nfiles = false\nmax_image_bytes = 0\n",
        )
        .unwrap();
        assert!(!config.clipboard.images && !config.clipboard.files);
        assert_eq!(config.clipboard.max_image_bytes, 1);
        let config = Config::from_toml_str("[clipboard]\nmax_image_bytes = 99999999999\n").unwrap();
        assert_eq!(
            config.clipboard.max_image_bytes,
            MAX_CLIPBOARD_IMAGE_BYTES_LIMIT
        );
    }

    #[test]
    fn universal_actions_default_to_a_hotkey_and_no_clipboard_fallback() {
        let config = Config::default();
        assert_eq!(config.general.actions_hotkey, "Ctrl+Alt+Space");
        assert_ne!(config.general.actions_hotkey, config.general.hotkey);
        assert!(config.actions.use_primary_selection);
        assert!(!config.actions.use_clipboard_fallback);
    }

    #[test]
    fn actions_settings_parse_and_an_empty_hotkey_stays_off() {
        let config = Config::from_toml_str(
            "[general]
actions_hotkey = \"  \"
[actions]
use_clipboard_fallback = true
             use_primary_selection = false
",
        )
        .unwrap();
        // Unlike the main hotkey, empty is a valid choice and is not replaced.
        assert_eq!(config.general.actions_hotkey, "");
        assert!(config.actions.use_clipboard_fallback);
        assert!(!config.actions.use_primary_selection);

        let trimmed = Config::from_toml_str(
            "[general]
actions_hotkey = \" F9 \"
",
        )
        .unwrap();
        assert_eq!(trimmed.general.actions_hotkey, "F9");
    }

    #[test]
    fn snippets_parse_and_incomplete_ones_are_dropped() {
        let config = Config::from_toml_str(
            r#"
[[snippet]]
name = "Signature"
keyword = " sig "
text = "Regards\nNinad"

[[snippet]]
name = "No keyword"
text = "x"

[[snippet]]
name = "  "
text = "nameless"

[[snippet]]
name = "Empty"
text = ""
"#,
        )
        .unwrap();
        assert_eq!(config.snippet.len(), 2);
        assert_eq!(config.snippet[0].keyword.as_deref(), Some("sig"));
        assert_eq!(config.snippet[0].text, "Regards\nNinad");
        assert_eq!(config.snippet[1].keyword, None);
    }

    #[test]
    fn snippet_expansion_is_off_by_default() {
        let snippets = Config::default().snippets;
        assert!(!snippets.auto_expand);
        assert_eq!(snippets.prefix, "");
        assert_eq!(snippets.expand_on, ExpandOn::Immediate);
        assert!(snippets.case_sensitive);
        assert!(snippets.ignore_apps.is_empty());
        assert!(!snippets.expand_in_terminals);
    }

    #[test]
    fn snippet_expansion_settings_parse_and_normalize() {
        let config = Config::from_toml_str(
            r#"
[snippets]
auto_expand = true
prefix = " ; "
expand_on = "delimiter"
case_sensitive = false
ignore_apps = ["KeePassXC", "  "]
expand_in_terminals = true
"#,
        )
        .unwrap();
        let snippets = config.snippets;
        assert!(snippets.auto_expand);
        assert_eq!(snippets.prefix, ";");
        assert_eq!(snippets.expand_on, ExpandOn::Delimiter);
        assert!(!snippets.case_sensitive);
        assert_eq!(snippets.ignore_apps, ["KeePassXC"]);
        assert!(snippets.expand_in_terminals);
    }

    #[test]
    fn the_default_ignore_list_has_no_blanks_or_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for app in DEFAULT_CLIPBOARD_IGNORE_APPS {
            assert!(!app.trim().is_empty());
            assert_eq!(*app, app.trim());
            assert!(seen.insert(app.to_lowercase()), "{app} is listed twice");
        }
        assert!(Config::default().clipboard.default_ignore_apps);
    }

    #[test]
    fn an_unknown_expand_on_means_delimiter_not_a_broken_file() {
        let config = Config::from_toml_str("[snippets]\nexpand_on = \"whenever\"\n").unwrap();
        assert_eq!(config.snippets.expand_on, ExpandOn::Delimiter);
    }

    #[test]
    fn out_of_range_values_are_normalized() {
        let config =
            Config::from_toml_str("[general]\nhotkey = \"  \"\n[window]\nwidth = 10\n").unwrap();
        assert_eq!(config.general.hotkey, "Super+Space");
        assert_eq!(config.window.width, MIN_WINDOW_WIDTH);

        let config = Config::from_toml_str("[window]\nwidth = 99999\n").unwrap();
        assert_eq!(config.window.width, MAX_WINDOW_WIDTH);
    }

    #[test]
    fn wrong_types_are_rejected() {
        assert!(Config::from_toml_str("[general]\nhide_on_blur = \"yes\"\n").is_err());
    }

    #[test]
    fn load_or_create_writes_defaults_then_reads_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");

        let (config, origin) = Config::load_or_create(&path).unwrap();
        assert_eq!(origin, ConfigOrigin::Created);
        assert_eq!(config, Config::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG_TOML);

        let (config, origin) = Config::load_or_create(&path).unwrap();
        assert_eq!(origin, ConfigOrigin::Loaded);
        assert_eq!(config, Config::default());
    }

    #[test]
    fn invalid_file_is_reported_and_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[general\nhotkey = ").unwrap();

        let err = Config::load_or_create(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), "[general\nhotkey = ");
    }

    fn saved(existing: Option<&str>, config: &Config) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        if let Some(text) = existing {
            fs::write(&path, text).unwrap();
        }
        config.save_to(&path).unwrap();
        assert!(
            !dir.path().join("config.toml.tmp").exists(),
            "temporary file left behind"
        );
        fs::read_to_string(&path).unwrap()
    }

    #[test]
    fn saving_an_unchanged_config_keeps_the_file_byte_identical() {
        assert_eq!(
            saved(Some(DEFAULT_CONFIG_TOML), &Config::default()),
            DEFAULT_CONFIG_TOML
        );
    }

    #[test]
    fn saving_without_a_file_creates_it_from_the_template() {
        assert_eq!(saved(None, &Config::default()), DEFAULT_CONFIG_TOML);
    }

    #[test]
    fn saving_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("config.toml");
        Config::default().save_to(&path).unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn changed_values_persist_and_comments_survive() {
        let mut config = Config::default();
        config.general.hotkey = "Ctrl+Shift+K".to_owned();
        config.general.launch_at_login = true;
        config.search.max_results = 12;
        config.appearance.theme = Theme::Dark;
        config.window.width = 900;
        config.files.directories = vec!["~/Projects".to_owned()];
        config.plugins.disabled = vec!["files".to_owned()];

        let text = saved(Some(DEFAULT_CONFIG_TOML), &config);
        for comment in [
            "# Sevak configuration",
            "# Shortcut that shows and hides Sevak.",
            "# Width of the search window in logical pixels (400-1600).",
            "# \"system\", \"light\" or \"dark\".",
            "# Index dot-files and dot-folders.",
            "# Web search engines: type",
        ] {
            assert!(text.contains(comment), "lost comment {comment:?}:\n{text}");
        }
        assert_eq!(Config::from_toml_str(&text).unwrap(), config);

        // Order is the document's own: the hotkey stays the first key of [general].
        let general = text.find("[general]").unwrap();
        let hotkey = text.find("hotkey = ").unwrap();
        let hide = text.find("hide_on_blur").unwrap();
        assert!(general < hotkey && hotkey < hide);
    }

    fn fallbacks(text: &str) -> Vec<String> {
        Config::from_toml_str(text)
            .unwrap()
            .search
            .fallback_web_search
            .keywords()
            .to_vec()
    }

    #[test]
    fn fallback_accepts_a_string_or_a_list() {
        assert_eq!(fallbacks(""), ["g"]);
        assert_eq!(
            fallbacks("[search]\nfallback_web_search = \" yt \"\n"),
            ["yt"]
        );
        assert!(fallbacks("[search]\nfallback_web_search = \"\"\n").is_empty());
        assert_eq!(
            fallbacks("[search]\nfallback_web_search = [\"g\", \"yt\", \"G\", \" \", \"gh\"]\n"),
            ["g", "yt", "gh"]
        );
        assert!(fallbacks("[search]\nfallback_web_search = []\n").is_empty());
        assert!(Config::from_toml_str("[search]\nfallback_web_search = 3\n").is_err());
    }

    #[test]
    fn fallback_round_trips_through_json_in_either_form() {
        let one = FallbackSearch::single("g");
        assert_eq!(serde_json::to_string(&one).unwrap(), "\"g\"");
        let many = FallbackSearch::list(["g", "yt"]);
        assert_eq!(serde_json::to_string(&many).unwrap(), "[\"g\",\"yt\"]");
        for value in [
            one,
            many,
            FallbackSearch::single(""),
            FallbackSearch::list(["g"]),
        ] {
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(
                serde_json::from_str::<FallbackSearch>(&json).unwrap(),
                value
            );
        }
    }

    #[test]
    fn saving_keeps_the_fallback_form_and_its_comments() {
        let mut config = Config::default();
        // Unchanged: a list of one stays a list, the file stays byte-identical.
        let existing = DEFAULT_CONFIG_TOML.replace(
            "fallback_web_search = \"g\"",
            "fallback_web_search = [\"g\"]",
        );
        config.search.fallback_web_search = FallbackSearch::list(["g"]);
        assert_eq!(saved(Some(&existing), &config), existing);

        config.search.fallback_web_search = FallbackSearch::list(["g", "yt", "gh"]);
        config.search.query_history = false;
        let text = saved(Some(&existing), &config);
        assert!(
            text.contains("fallback_web_search = [\"g\", \"yt\", \"gh\"]"),
            "{text}"
        );
        assert!(text.contains("# disable). A list offers several"), "{text}");
        assert!(text.contains("query_history = false"), "{text}");
        assert_eq!(Config::from_toml_str(&text).unwrap(), config);

        // Back to the string form.
        config.search.fallback_web_search = FallbackSearch::single("yt");
        let text = saved(Some(&text), &config);
        assert!(text.contains("fallback_web_search = \"yt\""), "{text}");
    }

    #[test]
    fn query_history_defaults_on_and_is_added_to_older_files() {
        assert!(Config::default().search.query_history);
        let old = "[search]\nmax_results = 8\n";
        assert!(Config::from_toml_str(old).unwrap().search.query_history);
        let text = saved(Some(old), &Config::default());
        assert!(text.contains("query_history = true"), "{text}");
    }

    #[test]
    fn currency_setting_is_added_to_a_config_written_by_an_older_version() {
        let mut config = Config::default();
        config.calculator.currency = true;
        let text = saved(Some("[general]\nhotkey = \"Alt+Space\"\n"), &config);
        assert!(text.contains("[calculator]") && text.contains("currency = true"));
        assert_eq!(Config::from_toml_str(&text).unwrap(), config);

        let text = saved(Some(DEFAULT_CONFIG_TOML), &config);
        assert!(text.contains("# Convert currencies"));
        assert_eq!(Config::from_toml_str(&text).unwrap(), config);
    }

    #[test]
    fn user_comments_and_unknown_keys_survive() {
        let existing = "\
# my own notes
[general]
hotkey = \"Alt+Space\" # my favourite
future_key = 42

[search]
max_results = 8 # keep it short

[custom]
thing = true
";
        let mut config = Config::default();
        config.general.hotkey = "Ctrl+Space".to_owned();
        config.search.max_results = 5;
        let text = saved(Some(existing), &config);

        assert!(text.starts_with("# my own notes\n[general]\n"));
        assert!(text.contains("hotkey = \"Ctrl+Space\" # my favourite"));
        assert!(text.contains("max_results = 5 # keep it short"));
        assert!(text.contains("future_key = 42"));
        assert!(text.contains("[custom]\nthing = true"));
        assert_eq!(Config::from_toml_str(&text).unwrap().search.max_results, 5);
    }

    #[test]
    fn missing_sections_and_keys_are_added() {
        let text = saved(
            Some("[general]\nhotkey = \"Alt+Space\"\n"),
            &Config::default(),
        );
        assert!(text.contains("hide_on_blur = true"));
        assert!(text.contains("[window]\nwidth = 720"));
        assert_eq!(Config::from_toml_str(&text).unwrap(), Config::default());
    }

    #[test]
    fn hand_written_snippets_survive_a_save() {
        let existing = format!(
            "{DEFAULT_CONFIG_TOML}\n[[snippet]]\nname = \"Sig\"   # mine\ntext = \"Hi\\nthere\"\n"
        );
        let mut config = Config::from_toml_str(&existing).unwrap();
        config.clipboard.enabled = true;
        let text = saved(Some(&existing), &config);

        assert!(text.contains("name = \"Sig\"   # mine"));
        assert!(text.contains("text = \"Hi\\nthere\""));
        assert_eq!(text.matches("[[snippet]]").count(), 2); // the template's comment + ours
        let reloaded = Config::from_toml_str(&text).unwrap();
        assert!(reloaded.clipboard.enabled);
        assert_eq!(reloaded.snippet, config.snippet);
    }

    #[test]
    fn web_search_list_is_replaced_and_keeps_its_comment() {
        let config = Config {
            web_search: vec![
                WebSearchEngine {
                    keyword: "ddg".to_owned(),
                    name: "DuckDuckGo".to_owned(),
                    url: "https://duckduckgo.com/?q={query}".to_owned(),
                },
                WebSearchEngine {
                    keyword: "g".to_owned(),
                    name: "Google".to_owned(),
                    url: "https://www.google.com/search?q={query}".to_owned(),
                },
            ],
            ..Config::default()
        };
        let text = saved(Some(DEFAULT_CONFIG_TOML), &config);

        assert!(text.contains("# Web search engines: type"));
        assert!(text.contains("keyword = \"ddg\""));
        assert!(!text.contains("YouTube"));
        assert_eq!(
            text.matches(
                "
[[web_search]]
"
            )
            .count(),
            2
        );
        assert_eq!(
            Config::from_toml_str(&text).unwrap().web_search,
            config.web_search
        );
    }

    #[test]
    fn an_empty_web_search_list_is_remembered() {
        let mut config = Config::default();
        config.web_search.clear();
        let text = saved(Some(DEFAULT_CONFIG_TOML), &config);
        assert!(!text.contains("[[web_search]]"));
        assert!(Config::from_toml_str(&text).unwrap().web_search.is_empty());

        // And back again.
        let text = saved(Some(&text), &Config::default());
        assert_eq!(
            Config::from_toml_str(&text).unwrap().web_search,
            WebSearchEngine::defaults()
        );
    }

    #[test]
    fn saving_over_an_invalid_file_fails_and_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[general\nhotkey = ").unwrap();
        let err = Config::default().save_to(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Edit { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), "[general\nhotkey = ");
    }

    #[test]
    fn crlf_files_stay_parseable_after_saving() {
        let existing = DEFAULT_CONFIG_TOML.replace('\n', "\r\n");
        let mut config = Config::default();
        config.search.max_results = 3;
        let text = saved(Some(&existing), &config);
        assert!(text.contains("# Sevak configuration\r\n"), "{text:?}");
        assert_eq!(Config::from_toml_str(&text).unwrap(), config);
    }

    fn binding(key: &str, query: Option<&str>, run: Option<&str>) -> HotkeyBinding {
        HotkeyBinding {
            key: key.to_owned(),
            query: query.map(str::to_owned),
            run: run.map(str::to_owned),
        }
    }

    #[test]
    fn hotkey_entries_parse() {
        let config = Config::from_toml_str(
            r#"
[[hotkey]]
key = " Ctrl+Alt+T "
query = "> "

[[hotkey]]
key = "Ctrl+Alt+F"
run = "apps:firefox.desktop"

[[hotkey]]
query = "no key, dropped"
"#,
        )
        .unwrap();
        assert_eq!(
            config.hotkeys,
            vec![
                binding("Ctrl+Alt+T", Some("> "), None),
                binding("Ctrl+Alt+F", None, Some("apps:firefox.desktop")),
            ]
        );
    }

    #[test]
    fn hotkey_targets() {
        assert_eq!(
            binding("K", Some("> "), None).target(),
            Ok(HotkeyTarget::Query("> ".to_owned()))
        );
        assert_eq!(
            binding("K", None, Some(" apps:x ")).target(),
            Ok(HotkeyTarget::Run("apps:x".to_owned()))
        );
        assert!(binding("K", Some("a"), Some("b")).target().is_err());
        assert!(binding("K", None, None).target().is_err());
        assert!(binding("K", None, Some("  ")).target().is_err());
        assert_eq!(binding("K", Some(""), None).describe(), "Open Sevak");
        assert_eq!(
            binding("K", Some("> "), None).describe(),
            "Open Sevak with \"> \""
        );
    }

    #[test]
    fn hotkey_entries_are_written_and_removed_again() {
        let mut config = Config {
            hotkeys: vec![
                binding("Ctrl+Alt+T", Some("> "), None),
                binding("Ctrl+Alt+F", None, Some("apps:firefox.desktop")),
            ],
            ..Config::default()
        };
        let text = saved(Some(DEFAULT_CONFIG_TOML), &config);
        assert_eq!(text.matches("\n[[hotkey]]\n").count(), 2, "{text}");
        assert!(text.contains("# Extra global hotkeys."));
        assert!(text.contains("# [[hotkey]]"));
        assert_eq!(Config::from_toml_str(&text).unwrap(), config);

        // An unchanged list leaves the file alone.
        assert_eq!(saved(Some(&text), &config), text);

        config.hotkeys.remove(0);
        let text = saved(Some(&text), &config);
        assert_eq!(text.matches("\n[[hotkey]]\n").count(), 1, "{text}");
        assert_eq!(Config::from_toml_str(&text).unwrap(), config);

        let text = saved(Some(&text), &Config::default());
        assert!(!text.contains("\n[[hotkey]]\n"), "{text}");
        assert_eq!(text, DEFAULT_CONFIG_TOML);
    }

    #[test]
    fn appearance_options_roundtrip_and_keep_comments() {
        let mut config = Config::default();
        config.appearance.accent = "#7c3aed".to_owned();
        config.appearance.font_size = 18;
        config.appearance.font_family = "Fira Sans, sans-serif".to_owned();
        config.appearance.opacity = 85;
        config.appearance.blur = true;
        config.appearance.radius = 4;
        config.appearance.theme_file = "themes/Nord.toml".to_owned();
        config.appearance.custom_css = "theme.css".to_owned();
        let text = saved(Some(DEFAULT_CONFIG_TOML), &config);
        assert!(text.contains("# Corner radius of the search bar in pixels (0-32)."));
        assert_eq!(Config::from_toml_str(&text).unwrap(), config);
    }

    #[test]
    fn older_files_without_the_new_options_load_with_defaults() {
        let config = Config::from_toml_str("[appearance]\ntheme = \"dark\"\n").unwrap();
        assert_eq!(config.appearance.theme, Theme::Dark);
        assert_eq!(
            AppearanceConfig {
                theme: Theme::Dark,
                ..AppearanceConfig::default()
            },
            config.appearance
        );
        assert!(config.hotkeys.is_empty());
    }
}
