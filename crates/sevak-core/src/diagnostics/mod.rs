//! The diagnostics report: what Sevak can say about itself so that a bug
//! report is useful, without telemetry and without private data.
//!
//! Nothing here uses the network, and nothing but the folder inventory and
//! the log reader (which only read what they are pointed at) touches the disk.
//! The shell (or a test) gathers the facts into a [`DiagnosticsInput`];
//! [`render`] turns it into Markdown text, passing everything free-form
//! through the [`Redactor`]. The pieces:
//!
//! - [`redact`]: what is removed, and how.
//! - [`config_summary`]: which settings appear (an allow-list).
//! - [`inventory`]: names and sizes of the files in Sevak's folders, and the
//!   tail and error counts of the logs.
//! - [`install`] and [`osinfo`]: how Sevak is installed, and the OS.
//!
//! The report is only ever shown to the user, copied by them or saved where
//! they say. It is never sent anywhere.

pub mod config_summary;
pub mod install;
pub mod inventory;
pub mod osinfo;
pub mod redact;

use crate::config::Config;
pub use install::{classify_install, InstallEnv, InstallKind};
pub use inventory::{inventory, scan_logs, FileEntry, Inventory, LogScan};
pub use osinfo::OsInfo;
pub use redact::{Identity, Redactor};

/// How many log lines the report carries.
pub const LOG_TAIL_LINES: usize = 100;

/// Who asked for the report. The command line cannot see the running app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportSource {
    /// `sevak --diagnostics`: reads files only; the running app (hotkey, tray,
    /// loaded plugins) is not consulted.
    CommandLine,
    /// Settings → Help: also reports what the running app knows.
    SettingsWindow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    pub version: String,
    /// Short git commit the binary was built from, when known.
    pub commit: Option<String>,
    /// `release` or `debug`.
    pub profile: String,
    /// The target triple.
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInfo {
    pub os: OsInfo,
    /// `windows`, `macos`, `x11`, `wayland` or `unknown`.
    pub display_server: String,
    /// `XDG_CURRENT_DESKTOP`, where there is one.
    pub desktop: Option<String>,
    /// The web view runtime's version, or why there is none.
    pub webview: Result<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallInfo {
    /// The executable's path, as the OS reports it (redacted when rendered).
    pub exe: Option<String>,
    pub kind: InstallKind,
    /// The package manager that updates Sevak, if one does.
    pub managed_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathsInfo {
    pub config_dir: String,
    pub config_file: String,
    pub data_dir: String,
    pub log_dir: String,
    /// `SEVAK_CONFIG_DIR` or `--config` is in effect.
    pub config_overridden: bool,
    /// `SEVAK_DATA_DIR` is in effect.
    pub data_overridden: bool,
}

/// The config file as it was found.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigState {
    Loaded(Box<Config>),
    /// There is no config file (Sevak writes the defaults on first run).
    Missing,
    /// The file exists but is not valid; the message says where.
    Invalid(String),
    Unreadable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Pass,
    Warn,
    Fail,
    /// Cannot be told from here (needs the running app).
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthCheck {
    pub name: String,
    pub status: Health,
    pub detail: String,
}

impl HealthCheck {
    pub fn new(name: &str, status: Health, detail: impl Into<String>) -> Self {
        Self {
            name: name.to_owned(),
            status,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    Builtin,
    Script,
    Workflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginState {
    Loaded,
    /// Switched off in the config.
    Disabled,
    /// Could not load or index; the text is the error.
    Failed(String),
    /// A script plugin or workflow the user has not allowed (yet).
    WaitingForApproval,
    /// Switched on; whether it loaded is only known to the running app.
    Enabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginStatus {
    /// `apps`, `web:g`, `script:my-plugin`, `workflow:my-flow`.
    pub id: String,
    pub kind: PluginKind,
    pub state: PluginState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSize {
    pub plugin: String,
    pub count: usize,
}

/// A keyword and what answers it (`builtin`, `web search`, `script plugin`,
/// `workflow`); never the engine's or workflow's own name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordEntry {
    pub keyword: String,
    pub owner: String,
}

/// Everything the report is made from.
#[derive(Debug, Clone)]
pub struct DiagnosticsInput {
    pub generated_at_unix: u64,
    pub source: ReportSource,
    pub build: BuildInfo,
    pub system: SystemInfo,
    pub install: InstallInfo,
    pub paths: PathsInfo,
    pub config: ConfigState,
    pub keywords: Vec<KeywordEntry>,
    pub health: Vec<HealthCheck>,
    /// Facts only the running app knows (the shortcut's mechanism, whether
    /// snippet expansion runs); empty for the command line.
    pub live: Vec<(String, String)>,
    pub plugins: Vec<PluginStatus>,
    /// Apps and files indexed, where known.
    pub indexes: Vec<IndexSize>,
    /// Why there are no index sizes, if there are none.
    pub indexes_note: Option<String>,
    pub config_dir: Inventory,
    /// `None` when it is the same folder as the config folder.
    pub data_dir: Option<Inventory>,
    pub logs: LogScan,
}

/// What every report says about itself, at the top.
const HEADER: &str = "\
> Made on this computer. **Nothing was sent anywhere**: this text exists only here until you copy it or save it. Read it before you share it.

**Included:** the Sevak version and build; your operating system, display server and install location; which features are on, your shortcut and whether it works, the theme, the update setting and the keywords in use; the names and sizes (never the contents) of the files in Sevak's folders; which plugins loaded, and the ids of script plugins and workflows with whether you allowed them; how many apps and files are indexed; the last 100 log lines and a count of recent errors; a few health checks.

**Not included:** clipboard history or snippet text; what is in any script or workflow; what you searched for, or your search history; usage statistics; contacts or 1Password data; file lists or bookmark titles; the folders you chose to search; web search addresses; any setting not listed above. Your user name, computer name and home folder are shown as `<user>`, `<host>` and `~`; e-mail addresses, web addresses with a query, tokens, keys and IP addresses are removed from the log lines.

Removing private data automatically cannot be perfect. Skim the log lines at the end and delete anything you would rather not share.";

/// Renders the report as Markdown, ready to paste into a GitHub issue.
pub fn render(input: &DiagnosticsInput, redactor: &Redactor) -> String {
    let mut out = Writer {
        text: String::new(),
        redactor,
    };
    out.line("# Sevak diagnostics");
    out.blank();
    out.line(&format!(
        "Made {} by {}.",
        format_utc(input.generated_at_unix),
        match input.source {
            ReportSource::CommandLine => "`sevak --diagnostics`",
            ReportSource::SettingsWindow => "Settings → Help → Copy diagnostics",
        }
    ));
    out.blank();
    out.line(HEADER);

    overview(&mut out, input);
    health(&mut out, input);
    configuration(&mut out, input);
    keywords(&mut out, input);
    plugins(&mut out, input);
    indexes(&mut out, input);
    files(&mut out, input);
    logs(&mut out, input);
    out.text
}

struct Writer<'a> {
    text: String,
    redactor: &'a Redactor,
}

impl Writer<'_> {
    fn line(&mut self, line: &str) {
        self.text.push_str(line);
        self.text.push('\n');
    }

    fn blank(&mut self) {
        self.text.push('\n');
    }

    fn heading(&mut self, title: &str) {
        self.blank();
        self.line(&format!("## {title}"));
        self.blank();
    }

    /// `- **name:** value`, the value redacted.
    fn item(&mut self, name: &str, value: &str) {
        let value = self.redactor.text(value);
        self.line(&format!("- **{name}:** {value}"));
    }

    /// `- **name:** path`, the path redacted (see [`Redactor::path`]).
    fn path(&mut self, name: &str, value: &str) {
        let value = self.redactor.path(value);
        self.line(&format!("- **{name}:** {value}"));
    }

    /// Like [`Writer::item`] for a value that is only ever our own words.
    fn fact(&mut self, name: &str, value: &str) {
        self.line(&format!("- **{name}:** {value}"));
    }
}

fn overview(out: &mut Writer<'_>, input: &DiagnosticsInput) {
    out.heading("Overview");
    let build = &input.build;
    let commit = build
        .commit
        .as_deref()
        .map_or(String::new(), |commit| format!(", commit {commit}"));
    out.fact(
        "Sevak",
        &format!(
            "{}{commit} ({} build for {})",
            build.version, build.profile, build.target
        ),
    );
    let system = &input.system;
    out.fact("Operating system", &system.os.describe());
    if let Some(kernel) = &system.os.kernel {
        out.fact("Kernel", kernel);
    }
    out.fact("Display server", &system.display_server);
    if let Some(desktop) = &system.desktop {
        out.item("Desktop", desktop);
    }
    match &system.webview {
        Ok(version) => out.fact("Web view", version),
        Err(why) => out.item("Web view", &format!("not found ({why})")),
    }
    let install = &input.install;
    out.fact("Install type", install.kind.label());
    if let Some(exe) = &install.exe {
        out.path("Program", exe);
    }
    if let Some(manager) = &install.managed_by {
        out.item("Updated by", manager);
    }
    let paths = &input.paths;
    let note = |overridden: bool| if overridden { " (set by you)" } else { "" };
    let config_dir = format!(
        "{}{}",
        out.redactor.path(&paths.config_dir),
        note(paths.config_overridden)
    );
    out.fact("Config folder", &config_dir);
    let data_dir = format!(
        "{}{}",
        out.redactor.path(&paths.data_dir),
        note(paths.data_overridden)
    );
    out.fact("Data folder", &data_dir);
    match input.source {
        ReportSource::CommandLine => out.fact(
            "Report from",
            "the command line: files only. What the running app knows (shortcut, tray, loaded plugins, index sizes) is in the report from Settings → Help",
        ),
        ReportSource::SettingsWindow => out.fact("Report from", "the running app (Settings → Help)"),
    }
    for (name, value) in &input.live {
        out.item(name, value);
    }
}

fn health(out: &mut Writer<'_>, input: &DiagnosticsInput) {
    out.heading("Health checks");
    for check in &input.health {
        let mark = match check.status {
            Health::Pass => "OK",
            Health::Warn => "WARN",
            Health::Fail => "FAIL",
            Health::Unknown => "?",
        };
        let detail = out.redactor.log_line(&check.detail);
        let line = if detail.is_empty() {
            format!("- **{mark}** {}.", check.name)
        } else {
            format!("- **{mark}** {}: {detail}", check.name)
        };
        out.line(&line);
    }
}

fn configuration(out: &mut Writer<'_>, input: &DiagnosticsInput) {
    out.heading("Configuration");
    match &input.config {
        ConfigState::Loaded(config) => {
            out.line("Only the settings below are reported; lists of folders, snippets and the like are counted, never shown.");
            for group in config_summary::summarize(config) {
                out.blank();
                out.line(&format!("**{}**", group.title));
                out.blank();
                for (name, value) in group.lines {
                    let value = out.redactor.text(&value);
                    out.line(&format!("- `{name}` = {value}"));
                }
            }
        }
        ConfigState::Missing => out.line("There is no config file yet: Sevak uses its defaults."),
        ConfigState::Invalid(message) => {
            out.line("The config file is not valid, so Sevak starts with its defaults:");
            out.blank();
            // Quoted values in a parse error can be the user's own text.
            let message = out.redactor.log_line(message);
            out.line(&format!("> {message}"));
        }
        ConfigState::Unreadable(message) => {
            let message = out.redactor.log_line(message);
            out.line(&format!("The config file cannot be read: {message}"));
        }
    }
}

fn keywords(out: &mut Writer<'_>, input: &DiagnosticsInput) {
    if input.keywords.is_empty() {
        return;
    }
    out.heading("Keywords in use");
    let mut sorted: Vec<&KeywordEntry> = input.keywords.iter().collect();
    sorted.sort_by(|a, b| {
        a.keyword
            .to_lowercase()
            .cmp(&b.keyword.to_lowercase())
            .then_with(|| a.owner.cmp(&b.owner))
    });
    for entry in sorted {
        let keyword = out.redactor.text(&entry.keyword);
        out.line(&format!("- `{keyword}`: {}", entry.owner));
    }
}

fn plugins(out: &mut Writer<'_>, input: &DiagnosticsInput) {
    out.heading("Plugins");
    for (kind, title) in [
        (PluginKind::Builtin, "Built in"),
        (PluginKind::Script, "Script plugins"),
        (PluginKind::Workflow, "Workflows"),
    ] {
        let members: Vec<&PluginStatus> = input.plugins.iter().filter(|p| p.kind == kind).collect();
        out.line(&format!("**{title}**"));
        out.blank();
        if members.is_empty() {
            out.line("- (none)");
        }
        for plugin in members {
            let id = out.redactor.text(&plugin.id);
            let state = match &plugin.state {
                PluginState::Loaded => "loaded".to_owned(),
                PluginState::Disabled => "switched off".to_owned(),
                PluginState::WaitingForApproval => "waiting for your approval".to_owned(),
                PluginState::Enabled => "on (whether it loaded needs the running app)".to_owned(),
                PluginState::Failed(error) => {
                    format!("**failed**: {}", out.redactor.log_line(error))
                }
            };
            out.line(&format!("- `{id}`: {state}"));
        }
        out.blank();
    }
}

fn indexes(out: &mut Writer<'_>, input: &DiagnosticsInput) {
    out.heading("Index sizes");
    if input.indexes.is_empty() {
        let why = input
            .indexes_note
            .as_deref()
            .unwrap_or("not available in this report");
        out.line(&format!("Not available: {why}."));
        return;
    }
    for index in &input.indexes {
        let plugin = out.redactor.text(&index.plugin);
        out.line(&format!("- `{plugin}`: {} entries", index.count));
    }
}

fn files(out: &mut Writer<'_>, input: &DiagnosticsInput) {
    out.heading("Files in Sevak's folders");
    out.line("Names and sizes only.");
    inventory_block(out, "Config folder", &input.config_dir);
    match &input.data_dir {
        Some(data) => inventory_block(out, "Data folder", data),
        None => {
            out.blank();
            out.line("The data folder is the config folder.");
        }
    }
}

fn inventory_block(out: &mut Writer<'_>, title: &str, inventory: &Inventory) {
    out.blank();
    out.line(&format!("**{title}**"));
    out.blank();
    if !inventory.exists {
        out.line("- (does not exist yet)");
        return;
    }
    if let Some(error) = &inventory.error {
        let error = out.redactor.log_line(error);
        out.line(&format!("- cannot be read: {error}"));
        return;
    }
    if inventory.entries.is_empty() {
        out.line("- (empty)");
    }
    for entry in &inventory.entries {
        let name = out.redactor.text(&entry.name);
        match entry.files {
            Some(files) => out.line(&format!(
                "- `{name}/`: {files} file{}, {}",
                if files == 1 { "" } else { "s" },
                format_bytes(entry.bytes)
            )),
            None => out.line(&format!("- `{name}`: {}", format_bytes(entry.bytes))),
        }
    }
    if inventory.truncated {
        out.line("- (more entries not listed)");
    }
}

fn logs(out: &mut Writer<'_>, input: &DiagnosticsInput) {
    out.heading("Logs");
    let logs = &input.logs;
    if let Some(error) = &logs.error {
        let error = out.redactor.log_line(error);
        out.line(&format!("The log folder cannot be read: {error}"));
        return;
    }
    if logs.files.is_empty() {
        out.line("There are no log files yet.");
        return;
    }
    out.line(&format!(
        "{} log file{} ({}). In the recent logs: **{} error{}, {} warning{}, {} panic{}**.",
        logs.files.len(),
        if logs.files.len() == 1 { "" } else { "s" },
        logs.files
            .iter()
            .map(|file| format!(
                "{} {}",
                out.redactor.text(&file.name),
                format_bytes(file.bytes)
            ))
            .collect::<Vec<_>>()
            .join(", "),
        logs.errors,
        if logs.errors == 1 { "" } else { "s" },
        logs.warnings,
        if logs.warnings == 1 { "" } else { "s" },
        logs.panics,
        if logs.panics == 1 { "" } else { "s" },
    ));
    out.blank();
    out.line(&format!(
        "The last {} line{}, with private text removed:",
        logs.tail.len(),
        if logs.tail.len() == 1 { "" } else { "s" }
    ));
    out.blank();
    out.line("```text");
    for line in &logs.tail {
        // A fence inside a line would end the block early.
        let line = out.redactor.log_line(line).replace("```", "'''");
        out.line(&line);
    }
    out.line("```");
}

/// `1.5 KB`, `12 bytes`.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} byte{}", if bytes == 1 { "" } else { "s" });
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// `2026-10-04 12:00:00 UTC` for seconds since the Unix epoch.
pub fn format_utc(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests;
