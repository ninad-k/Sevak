//! `ext` and `store` in the launcher: search the extension catalog and install
//! from the result list.
//!
//! The plugin only reads what [`ExtensionStore`] already loaded (in memory or
//! from the cache file) and never touches the network or the disk while
//! typing: [`Plugin::query`] answers from a snapshot built in
//! [`Plugin::refresh`] and after each change. Pressing Enter on a row is the
//! user's decision to use the network:
//!
//! | Row | Enter |
//! |---|---|
//! | "Load the extension list" (shown until a list is loaded) | fetches the list once |
//! | Install `<name>` | downloads, verifies and installs it (after a confirmation) |
//! | Update `<name>` | replaces the installed version (after a confirmation) |
//! | an installed or unavailable item | opens Settings > Extensions |
//!
//! The work runs on a background thread and the shell is told when it
//! finishes ([`Hooks::finished`]): the launcher never waits for a download. An
//! installed extension is, as everywhere, new and unapproved: Sevak's Allow
//! dialog comes next. Everything the plugin does goes through the same store as
//! the settings page, so the same checks apply.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use sevak_core::model::score;
use sevak_core::{Action, Config, IconSource, Plugin, PluginError, PluginResult, ResultItem};

use super::store::{CatalogItem, CatalogView, ExtensionStore, ItemKind, State};
use crate::PluginInfo;

/// The family id: `[plugins] disabled = ["extensions"]` turns both keywords off.
pub const FAMILY: &str = "extensions";
/// `ext`.
pub const KEYWORD: &str = "ext";
/// `store`, the same plugin under a second name.
pub const STORE_KEYWORD: &str = "store";

/// What a finished background change tells the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// It worked.
    pub ok: bool,
    /// A sentence for the user (a notification).
    pub message: String,
    /// Something on disk changed, so Sevak should reload to see it (and ask
    /// about a new extension).
    pub changed: bool,
}

/// What the plugin needs from the shell.
pub struct Hooks {
    /// Called on the worker thread when a load, install or update ends.
    pub finished: Box<dyn Fn(Report) + Send + Sync>,
    /// Opens Settings > Extensions.
    pub open_settings: Box<dyn Fn() + Send + Sync>,
}

/// The shared state of the two keyword plugins.
struct Inner {
    store: Arc<ExtensionStore>,
    hooks: Hooks,
    snapshot: RwLock<Option<Arc<CatalogView>>>,
    /// A change is running; a second Enter does nothing until it ends.
    working: AtomicBool,
}

impl Inner {
    fn snapshot(&self) -> Option<Arc<CatalogView>> {
        self.snapshot
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn set_snapshot(&self, view: Option<CatalogView>) {
        *self.snapshot.write().unwrap_or_else(|p| p.into_inner()) = view.map(Arc::new);
    }

    /// Rebuilds the snapshot from what the store has (memory, then the cache
    /// file). Local only.
    fn reload_snapshot(&self) {
        let view = self
            .store
            .current_view()
            .or_else(|| self.store.load_cached());
        self.set_snapshot(view);
    }
}

/// Builds the plugins and lists them for Settings.
pub struct ExtensionsHost {
    inner: Arc<Inner>,
}

impl ExtensionsHost {
    pub fn new(store: Arc<ExtensionStore>, hooks: Hooks) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                hooks,
                snapshot: RwLock::new(None),
                working: AtomicBool::new(false),
            }),
        }
    }

    pub fn store(&self) -> &Arc<ExtensionStore> {
        &self.inner.store
    }

    /// The enabled plugins (`ext` and `store`).
    pub fn plugins(&self, config: &Config) -> Vec<Arc<dyn Plugin>> {
        if !config.plugins.is_enabled(FAMILY) {
            return Vec::new();
        }
        [("extensions", KEYWORD), ("extensions:store", STORE_KEYWORD)]
            .into_iter()
            .filter(|(id, _)| config.plugins.is_enabled(id))
            .map(|(id, keyword)| {
                Arc::new(ExtensionsPlugin {
                    id,
                    keyword,
                    inner: self.inner.clone(),
                }) as Arc<dyn Plugin>
            })
            .collect()
    }

    /// The Settings > Plugins rows.
    pub fn catalog(&self, config: &Config) -> Vec<PluginInfo> {
        [
            ("extensions", "Extensions", KEYWORD),
            ("extensions:store", "Extensions (store)", STORE_KEYWORD),
        ]
        .into_iter()
        .map(|(id, name, keyword)| PluginInfo {
            id: id.to_owned(),
            name: name.to_owned(),
            description: DESCRIPTION.to_owned(),
            keyword: Some(keyword.to_owned()),
            enabled: config.plugins.is_enabled(FAMILY) && config.plugins.is_enabled(id),
        })
        .collect()
    }

    /// Takes the snapshot the plugins answer from; the page calls this after it
    /// changed something.
    pub fn refresh_snapshot(&self) {
        self.inner.reload_snapshot();
    }
}

const DESCRIPTION: &str =
    "Type ext (or store) and a name to browse the extension gallery and install from the list.";

pub struct ExtensionsPlugin {
    id: &'static str,
    keyword: &'static str,
    inner: Arc<Inner>,
}

/// Text for a row: no control characters, cut to `max` characters.
fn clean(text: &str, max: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &flat[..end]),
        None => flat,
    }
}

/// How well `item` matches `terms` (lower case): `None` when a term matches
/// nothing. The name counts most.
fn match_score(item: &CatalogItem, terms: &[String]) -> Option<f64> {
    if terms.is_empty() {
        return Some(0.0);
    }
    let name = item.name.to_lowercase();
    let id = item.id.to_lowercase();
    let author = item.author.to_lowercase();
    let description = item.description.to_lowercase();
    let kind = item.kind.label().to_lowercase();
    let tags = item.tags.join(" ");
    let mut total = 0.0;
    for term in terms {
        let term = term.as_str();
        total += if name == term || id == term {
            100.0
        } else if name.starts_with(term) || id.starts_with(term) {
            80.0
        } else if name.contains(term) || id.contains(term) {
            60.0
        } else if tags.contains(term) || kind.contains(term) {
            40.0
        } else if author.contains(term) {
            30.0
        } else if description.contains(term) {
            20.0
        } else {
            return None;
        };
    }
    Some(total)
}

impl ExtensionsPlugin {
    fn key(verb: &str, id: &str) -> String {
        format!("{verb}:{id}")
    }

    fn load_row(&self, subtitle: &str) -> ResultItem {
        ResultItem::new(
            self.id,
            "load",
            "Load the extension list",
            Action::Custom {
                payload: "load".to_owned(),
            },
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin("plugin"))
        .with_score(score::KEYWORD - 1.0)
    }

    fn item_row(&self, item: &CatalogItem, rank: f64, order: usize) -> ResultItem {
        let kind = item.kind.label();
        let by = clean(&item.author, 40);
        let version = if item.version.is_empty() {
            String::new()
        } else {
            format!(" {}", clean(&item.version, 20))
        };
        let (verb, title, subtitle, payload) = match item.state {
            State::UpdateAvailable => (
                "update",
                format!("Update {}", clean(&item.name, 80)),
                format!(
                    "{kind} by {by}: {} to {} (Enter updates)",
                    clean(item.installed_version.as_deref().unwrap_or("?"), 20),
                    clean(&item.version, 20)
                ),
                format!("update:{}", item.id),
            ),
            State::Available => (
                "install",
                clean(&item.name, 80),
                format!(
                    "{kind} by {by}{version}: {} (Enter installs)",
                    clean(&item.description, 120)
                ),
                format!("install:{}", item.id),
            ),
            State::Installed => (
                "installed",
                clean(&item.name, 80),
                format!("Installed{version}. Enter opens Settings > Extensions."),
                "open".to_owned(),
            ),
            State::Unavailable => (
                "unavailable",
                clean(&item.name, 80),
                clean(
                    item.unavailable.as_deref().unwrap_or("Not available here."),
                    160,
                ),
                "open".to_owned(),
            ),
        };
        // Updates first, then installs by match, then what is already there;
        // among equals the catalog's own (alphabetical) order.
        let base = match item.state {
            State::UpdateAvailable => 4_000.0,
            State::Available => 3_000.0,
            State::Installed => 2_000.0,
            State::Unavailable => 1_000.0,
        };
        ResultItem::new(
            self.id,
            Self::key(verb, &item.id),
            title,
            Action::Custom { payload },
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin(match item.kind {
            ItemKind::Theme => "settings",
            _ => "plugin",
        }))
        .with_score((base + rank - order as f64 * 0.01).min(score::KEYWORD - 2.0))
    }
}

impl Plugin for ExtensionsPlugin {
    fn id(&self) -> &str {
        self.id
    }

    fn name(&self) -> &str {
        if self.id == "extensions" {
            "Extensions"
        } else {
            "Extensions (store)"
        }
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn keyword(&self) -> Option<&str> {
        Some(self.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn tracks_usage(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let Some(view) = self.inner.snapshot() else {
            return vec![self.load_row(
                "Fetches the list from the Sevak repository on GitHub. Nothing is installed.",
            )];
        };
        let terms: Vec<String> = input.split_whitespace().map(str::to_lowercase).collect();
        let mut rows: Vec<ResultItem> = view
            .items
            .iter()
            .enumerate()
            .filter_map(|(order, item)| {
                match_score(item, &terms).map(|rank| self.item_row(item, rank, order))
            })
            .collect();
        rows.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.title.cmp(&b.title)));
        let refresh = if view.from_cache {
            "Showing the list saved earlier. Enter loads the current one."
        } else {
            "Enter loads the list again."
        };
        rows.push(self.load_row(refresh).with_score(1.0));
        rows
    }

    fn confirmation(&self, item: &ResultItem) -> Option<String> {
        let Action::Custom { payload } = &item.action else {
            return None;
        };
        if payload == "load" {
            return Some(
                "Load the list of extensions from Sevak's repository on GitHub \
                 (raw.githubusercontent.com)? Nothing is installed and nothing else is sent."
                    .to_owned(),
            );
        }
        let (verb, id) = payload.split_once(':')?;
        let view = self.inner.snapshot()?;
        let entry = view.items.iter().find(|entry| entry.id == id)?;
        let what = format!(
            "{} \"{}\"{} by {}",
            entry.kind.label(),
            clean(&entry.name, 80),
            if entry.version.is_empty() {
                String::new()
            } else {
                format!(" {}", clean(&entry.version, 20))
            },
            clean(&entry.author, 60)
        );
        let action = if verb == "update" {
            "Update"
        } else {
            "Install"
        };
        let mut text = format!(
            "{action} {what}?\n\nSevak downloads one package from its own repository, checks \
             its SHA-256 against the list and puts it in your folder. "
        );
        match entry.kind {
            ItemKind::Native => text.push_str(&format!(
                "This is a native extension: a compiled program, not a script. Declared \
                 permissions: {}. Sevak does not sandbox it. It never runs until you allow it \
                 in the dialog that follows, which shows its publisher and checksum.",
                if entry.permissions.is_empty() {
                    "none".to_owned()
                } else {
                    entry
                        .permissions
                        .iter()
                        .map(|p| clean(p, 24))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            )),
            ItemKind::Theme => text.push_str("A theme only changes colors and sizes."),
            _ => text.push_str("It does not run until you allow it, where it can run code."),
        }
        Some(text)
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return Err(PluginError::Unsupported(item.id.clone()));
        };
        let inner = self.inner.clone();
        if payload == "open" {
            (inner.hooks.open_settings)();
            return Ok(());
        }
        let work = match payload.split_once(':') {
            None if payload == "load" => Work::Load,
            Some(("install", id)) => Work::Install(id.to_owned()),
            Some(("update", id)) => Work::Update(id.to_owned()),
            _ => return Err(PluginError::Unsupported(item.id.clone())),
        };
        if inner.working.swap(true, Ordering::SeqCst) {
            return Err(PluginError::Message(
                "Another extension change is still running.".to_owned(),
            ));
        }
        std::thread::Builder::new()
            .name("sevak-extensions".to_owned())
            .spawn(move || {
                let report = run(&inner, work);
                inner.working.store(false, Ordering::SeqCst);
                (inner.hooks.finished)(report);
            })
            .map_err(|err| {
                self.inner.working.store(false, Ordering::SeqCst);
                PluginError::Message(format!("could not start the work: {err}"))
            })?;
        Ok(())
    }

    fn refresh(&self) -> PluginResult<()> {
        self.inner.reload_snapshot();
        Ok(())
    }

    fn index_size(&self) -> Option<usize> {
        self.inner.snapshot().map(|view| view.items.len())
    }
}

enum Work {
    Load,
    Install(String),
    Update(String),
}

/// Does `work` and says how it went.
fn run(inner: &Inner, work: Work) -> Report {
    let outcome = match work {
        Work::Load => inner.store.refresh().map(|view| {
            let count = view.items.len();
            inner.set_snapshot(Some(view));
            Report {
                ok: true,
                message: format!(
                    "The extension list is loaded: {count} items. Type ext to browse."
                ),
                changed: false,
            }
        }),
        Work::Install(id) | Work::Update(id) if inner.store.current().is_none() => {
            Err(format!("Load the extension list first ({id})."))
        }
        Work::Install(id) => inner
            .store
            .install(&id)
            .map(|done| done_report(&done, false)),
        Work::Update(id) => inner.store.update(&id).map(|done| done_report(&done, true)),
    };
    // What the page and the launcher show follows what happened.
    if let Some(view) = inner.store.current_view() {
        inner.set_snapshot(Some(view));
    }
    outcome.unwrap_or_else(|message| Report {
        ok: false,
        message,
        changed: false,
    })
}

fn done_report(done: &super::store::Outcome, updated: bool) -> Report {
    let name = clean(&done.name, 60);
    let message = match (done.kind, updated) {
        (ItemKind::Theme, _) => {
            format!("Installed the theme {name}. Choose it in Settings > Appearance.")
        }
        (ItemKind::Native, true) => format!(
            "Updated {name}. It is a new version of a program, so Sevak asks for your permission again."
        ),
        (_, true) => format!("Updated {name}. Sevak asks again if it can run code."),
        (ItemKind::Native, false) => format!(
            "Installed {name}. It does not run until you allow it in the dialog Sevak shows next."
        ),
        (_, false) => format!("Installed {name}. It does not run until you allow it, if it can run code."),
    };
    Report {
        ok: true,
        message,
        changed: true,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver};
    use std::sync::Mutex;
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::extensions::testing::*;

    struct Shell {
        world: World,
        host: ExtensionsHost,
        reports: Receiver<Report>,
        opened: Arc<AtomicBool>,
    }

    fn shell() -> Shell {
        let world = World::new();
        let (tx, reports) = mpsc::channel();
        let tx = Mutex::new(tx);
        let opened = Arc::new(AtomicBool::new(false));
        let flag = opened.clone();
        // The host shares the world's store.
        let store = Arc::new(ExtensionStore::with(
            world.store.dirs().clone(),
            world.net.clone(),
            sevak_core::gallery_source::Pin::new("v1.2.3"),
            platform(),
            "0.1.0".to_owned(),
        ));
        let host = ExtensionsHost::new(
            store,
            Hooks {
                finished: Box::new(move |report| {
                    let _ = tx.lock().unwrap().send(report);
                }),
                open_settings: Box::new(move || flag.store(true, Ordering::SeqCst)),
            },
        );
        Shell {
            world,
            host,
            reports,
            opened,
        }
    }

    fn plugin(shell: &Shell, id: &str) -> Arc<dyn Plugin> {
        shell
            .host
            .plugins(&Config::default())
            .into_iter()
            .find(|p| p.id() == id)
            .unwrap()
    }

    fn wait(shell: &Shell) -> Report {
        shell.reports.recv_timeout(Duration::from_secs(20)).unwrap()
    }

    fn titles(items: &[ResultItem]) -> Vec<String> {
        items.iter().map(|i| i.title.clone()).collect()
    }

    fn publish_two(shell: &Shell) {
        let w = &shell.world;
        let zip = workflow_zip("docs", &[]);
        let entry = workflow_entry(w, "docs", "1.0", &zip);
        w.publish(vec![entry, native_entry(w, "0.2.0", b"elf")], vec![]);
    }

    #[test]
    fn two_keywords_one_plugin_and_both_can_be_switched_off() {
        let shell = shell();
        let ids: Vec<String> = shell
            .host
            .plugins(&Config::default())
            .iter()
            .map(|p| format!("{}={}", p.id(), p.keyword().unwrap()))
            .collect();
        assert_eq!(ids, ["extensions=ext", "extensions:store=store"]);
        let mut config = Config::default();
        config.plugins.disabled = vec!["extensions:store".to_owned()];
        assert_eq!(shell.host.plugins(&config).len(), 1);
        config.plugins.disabled = vec!["extensions".to_owned()];
        assert!(shell.host.plugins(&config).is_empty());
        let rows = shell.host.catalog(&config);
        assert!(rows.iter().all(|row| !row.enabled));
        assert_eq!(shell.host.catalog(&Config::default()).len(), 2);
        assert!(!plugin(&shell, "extensions").global());
        assert!(!plugin(&shell, "extensions").tracks_usage());
    }

    #[test]
    fn before_a_list_is_loaded_the_only_row_offers_to_load_it() {
        let shell = shell();
        let rows = plugin(&shell, "extensions").query("notes");
        assert_eq!(titles(&rows), ["Load the extension list"]);
        assert!(
            shell.world.net.asked().is_empty(),
            "typing requests nothing"
        );
        let question = plugin(&shell, "extensions").confirmation(&rows[0]).unwrap();
        assert!(question.contains("Nothing is installed"), "{question}");
    }

    #[test]
    fn pressing_enter_on_load_fetches_the_list_once_and_then_answers_from_it() {
        let shell = shell();
        publish_two(&shell);
        let ext = plugin(&shell, "extensions");
        let load = ext.query("")[0].clone();
        ext.execute(&load).unwrap();
        let report = wait(&shell);
        assert!(report.ok, "{report:?}");
        assert!(!report.changed);
        let asked = shell.world.net.asked().len();

        let rows = ext.query("docs");
        assert_eq!(rows[0].title, "DOCS");
        assert!(
            rows[0].subtitle.contains("Enter installs"),
            "{}",
            rows[0].subtitle
        );
        // Typing never goes back to the network.
        for query in ["", "d", "doc", "tool"] {
            ext.query(query);
        }
        assert_eq!(shell.world.net.asked().len(), asked);
    }

    #[test]
    fn rows_are_filtered_and_ranked_by_name_first() {
        let shell = shell();
        publish_two(&shell);
        shell.host.store().refresh().unwrap();
        shell.host.refresh_snapshot();
        let ext = plugin(&shell, "extensions");
        let names = |q: &str| {
            titles(&ext.query(q))
                .into_iter()
                .filter(|t| t != "Load the extension list")
                .collect::<Vec<_>>()
        };
        assert_eq!(names(""), ["DOCS", "Tool"]);
        assert_eq!(names("tool"), ["Tool"]);
        assert_eq!(names("TOOL"), ["Tool"]);
        assert_eq!(names("native"), ["Tool"], "kind labels match");
        assert_eq!(names("ada"), ["Tool"], "the publisher matches");
        assert!(names("zzz-no-match").is_empty());
        // Every term must match.
        assert!(names("tool zzz").is_empty());
        // The store keyword gives the same answers.
        let store = plugin(&shell, "extensions:store");
        assert_eq!(titles(&store.query("tool"))[0], "Tool");
        assert_eq!(store.query("tool")[0].id, "extensions:store:install:tool");
    }

    #[test]
    fn install_asks_first_runs_in_the_background_and_reports() {
        let shell = shell();
        publish_two(&shell);
        shell.host.store().refresh().unwrap();
        shell.host.refresh_snapshot();
        let ext = plugin(&shell, "extensions");
        let row = ext.query("docs").remove(0);

        let question = ext.confirmation(&row).unwrap();
        assert!(
            question.contains("Install Workflow \"DOCS\" 1.0 by Sevak"),
            "{question}"
        );
        assert!(question.contains("SHA-256"), "{question}");

        ext.execute(&row).unwrap();
        let report = wait(&shell);
        assert!(report.ok && report.changed, "{report:?}");
        assert!(
            report.message.contains("Installed DOCS"),
            "{}",
            report.message
        );
        assert!(shell.world.workflows().join("docs/workflow.toml").is_file());
        // The list now shows it as installed.
        let after = ext.query("docs");
        assert!(
            after[0].subtitle.starts_with("Installed"),
            "{}",
            after[0].subtitle
        );
        assert_eq!(after[0].id, "extensions:installed:docs");
    }

    #[test]
    fn the_native_question_says_what_it_is_and_what_it_can_do() {
        let shell = shell();
        publish_two(&shell);
        shell.host.store().refresh().unwrap();
        shell.host.refresh_snapshot();
        let ext = plugin(&shell, "extensions");
        let row = ext.query("tool").remove(0);
        let question = ext.confirmation(&row).unwrap();
        assert!(question.contains("native extension"), "{question}");
        assert!(question.contains("compiled program"), "{question}");
        assert!(question.contains("network"), "{question}");
        assert!(question.contains("does not sandbox"), "{question}");
        assert!(
            question.contains("never runs until you allow"),
            "{question}"
        );
    }

    #[test]
    fn a_failed_install_is_reported_and_changes_nothing() {
        let shell = shell();
        publish_two(&shell);
        shell.host.store().refresh().unwrap();
        shell.host.refresh_snapshot();
        shell.world.net.down();
        let ext = plugin(&shell, "extensions");
        let row = ext.query("docs").remove(0);
        ext.execute(&row).unwrap();
        let report = wait(&shell);
        assert!(!report.ok && !report.changed, "{report:?}");
        assert!(
            report.message.contains("Could not download"),
            "{}",
            report.message
        );
        assert!(!shell.world.workflows().join("docs").exists());
        // The working flag is released: another try is accepted.
        ext.execute(&row).unwrap();
        wait(&shell);
    }

    #[test]
    fn installed_and_unavailable_rows_open_the_settings_page() {
        let shell = shell();
        let w = &shell.world;
        let mut tool = native_entry(w, "0.2.0", b"elf");
        tool["min_sevak"] = json!("9.0.0");
        w.publish(vec![tool], vec![]);
        shell.host.store().refresh().unwrap();
        shell.host.refresh_snapshot();
        let ext = plugin(&shell, "extensions");
        let row = ext.query("tool").remove(0);
        assert_eq!(row.id, "extensions:unavailable:tool");
        assert!(
            row.subtitle.contains("Needs Sevak 9.0.0"),
            "{}",
            row.subtitle
        );
        assert!(ext.confirmation(&row).is_none(), "no download, no question");
        ext.execute(&row).unwrap();
        assert!(shell.opened.load(Ordering::SeqCst));
    }

    #[test]
    fn unknown_payloads_are_refused() {
        let shell = shell();
        let ext = plugin(&shell, "extensions");
        for payload in ["", "nope", "install", "delete:docs", "../../x"] {
            let item = ResultItem::new(
                "extensions",
                "x",
                "x",
                Action::Custom {
                    payload: payload.to_owned(),
                },
            );
            assert!(ext.execute(&item).is_err(), "{payload:?}");
        }
        let copy = ResultItem::new(
            "extensions",
            "x",
            "x",
            Action::CopyText { text: "t".into() },
        );
        assert!(ext.execute(&copy).is_err());
    }

    #[test]
    fn row_text_is_made_safe_and_short() {
        assert_eq!(clean("a\nb\u{7}c  d", 40), "a b c d");
        assert_eq!(clean(&"x".repeat(100), 10), format!("{}…", "x".repeat(10)));
        assert_eq!(clean("", 10), "");
    }
}
