//! Whole-disk file search through the OS index: `ff <name>` finds files and
//! folders by name anywhere, `in <words>` finds files by what is inside them.
//!
//! The OS index (see [`sevak_platform::os_search`]) takes tens of milliseconds
//! to seconds, so it never runs on the typing path. [`Plugin::query`] numbers
//! the request with a [`Delivery`] (the mechanism script plugins use), hands it
//! to one worker thread, and waits only a short while. Whatever the index has
//! not answered by then arrives late: the shell is told, runs the same query
//! again, and finds the answer cached. A request the user has typed past is
//! dropped, before it runs if the worker has not started it yet and when it
//! answers otherwise.
//!
//! `ff` also lists matches from the in-memory folder index at once, so there
//! is something to see (and something to fall back on when the OS index is
//! missing) while the index is still working. Rows are the files plugin's own
//! rows, so they open, show in folder and copy their path the same way.

use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use sevak_core::{
    Action, Config, FuzzyQuery, IconSource, Plugin, PluginResult, ResultItem, ResultsNotifier,
};
use sevak_platform::os_search::search_words;
use sevak_platform::{OsHit, OsSearchError, OsSearchKind, OsSearchRequest, PlatformProvider};

use crate::files::{name_bonus, FilesPlugin, PRUNED_DIRS};
use crate::path_browse;
use crate::script::delivery::{Begin, Delivery};

/// How long `query` waits for the index before returning what it has.
const SOFT_BUDGET: Duration = Duration::from_millis(80);
/// How long the index may take before the search is given up.
const INDEX_TIMEOUT: Duration = Duration::from_secs(2);
/// Rows shown for one search.
const MAX_ROWS: usize = 50;
const MIN_NAME_CHARS: usize = 2;
/// Inside-file searches on one or two letters match almost everything.
const MIN_CONTENT_CHARS: usize = 3;
/// Content hits are in the index's order, best first; the scores keep it.
const CONTENT_TOP_SCORE: f64 = 1000.0;

type Backend = dyn Fn(&OsSearchRequest) -> Result<Vec<OsHit>, OsSearchError> + Send + Sync;

/// Builds the plugins of the `files` family: the folder index (`files`), and,
/// unless `[files] use_os_index` is off, `files:names` (`index_keyword`) and
/// `files:content` (`content_keyword`). An empty or clashing keyword leaves
/// that plugin out.
pub fn files_family(config: &Config, platform: &Arc<dyn PlatformProvider>) -> Vec<Arc<dyn Plugin>> {
    let files = Arc::new(FilesPlugin::new(config.files.clone(), platform.clone()));
    let mut plugins: Vec<Arc<dyn Plugin>> = vec![files.clone()];
    if !config.files.use_os_index {
        return plugins;
    }

    let backend: Arc<Backend> = {
        let platform = platform.clone();
        Arc::new(move |request| platform.os_search(request))
    };
    let mut taken: Vec<String> = vec![config.files.keyword.trim().to_lowercase()];
    let variants = [
        (OsSearchKind::Names, &config.files.index_keyword),
        (OsSearchKind::Content, &config.files.content_keyword),
    ];
    for (kind, keyword) in variants {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            continue;
        }
        if taken.contains(&keyword.to_lowercase()) {
            tracing::warn!(
                keyword,
                "files keyword is used twice; one of them is ignored"
            );
            continue;
        }
        taken.push(keyword.to_lowercase());
        plugins.push(Arc::new(OsFilesPlugin::new(
            kind,
            keyword,
            files.clone(),
            backend.clone(),
            config.files.include_hidden,
        )));
    }
    plugins
}

/// What the worker thread should search for next.
struct Job {
    id: u64,
    input: String,
}

#[derive(Default)]
struct Queue {
    /// Only the newest request matters, so a new one replaces an unstarted one.
    job: Option<Job>,
    stopped: bool,
    worker_started: bool,
}

struct Inner {
    kind: OsSearchKind,
    files: Arc<FilesPlugin>,
    backend: Arc<Backend>,
    include_hidden: bool,
    delivery: Delivery,
    queue: Mutex<Queue>,
    wake: Condvar,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Inner {
    /// Queues request `id` for the worker, starting the worker on first use.
    /// False if there is nobody to run it.
    fn submit(self: &Arc<Self>, id: u64, input: &str) -> bool {
        let mut queue = lock(&self.queue);
        if queue.stopped {
            return false;
        }
        queue.job = Some(Job {
            id,
            input: input.to_owned(),
        });
        if !queue.worker_started {
            let inner = Arc::clone(self);
            let name = match self.kind {
                OsSearchKind::Names => "sevak-os-names",
                OsSearchKind::Content => "sevak-os-content",
            };
            if let Err(err) = thread::Builder::new()
                .name(name.to_owned())
                .spawn(move || inner.work())
            {
                tracing::error!("could not start the {name} thread: {err}");
                queue.job = None;
                return false;
            }
            queue.worker_started = true;
        }
        self.wake.notify_one();
        true
    }

    fn stop(&self) {
        lock(&self.queue).stopped = true;
        self.wake.notify_all();
    }

    fn work(&self) {
        loop {
            let job = {
                let mut queue = lock(&self.queue);
                loop {
                    if queue.stopped {
                        return;
                    }
                    if let Some(job) = queue.job.take() {
                        break job;
                    }
                    queue = self
                        .wake
                        .wait(queue)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            };
            // Typed past before the worker got to it: skip the search entirely.
            if !self.delivery.is_current(job.id) {
                continue;
            }
            let items = self.search(&job.input);
            self.delivery.deliver(job.id, items);
        }
    }

    /// One search of the index, as result rows. Never panics and never fails:
    /// a missing or broken index is a row that says so.
    fn search(&self, input: &str) -> Vec<ResultItem> {
        let request = OsSearchRequest {
            text: input.to_owned(),
            kind: self.kind,
            limit: MAX_ROWS,
            timeout: INDEX_TIMEOUT,
        };
        let answer = catch_unwind(AssertUnwindSafe(|| (self.backend)(&request)))
            .unwrap_or_else(|_| Err(OsSearchError::Failed("the search crashed".to_owned())));
        match answer {
            Ok(hits) => self.rows(input, hits),
            Err(OsSearchError::Unavailable(reason)) => {
                tracing::debug!(%reason, "OS file index unavailable");
                vec![status_row(&reason)]
            }
            Err(OsSearchError::TimedOut) => {
                tracing::debug!(input, "OS file index timed out");
                Vec::new()
            }
            Err(OsSearchError::Failed(reason)) => {
                tracing::warn!(%reason, "OS file index search failed");
                vec![status_row(&format!(
                    "The file index could not be searched: {reason}"
                ))]
            }
        }
    }

    fn rows(&self, input: &str, hits: Vec<OsHit>) -> Vec<ResultItem> {
        let words = search_words(input);
        let mut queries: Vec<FuzzyQuery> = words.iter().map(|w| FuzzyQuery::for_paths(w)).collect();
        let whole = input.trim();
        hits.into_iter()
            .filter(|hit| !is_noise(&hit.path, self.include_hidden, cfg!(target_os = "macos")))
            .take(MAX_ROWS)
            .enumerate()
            .map(|(rank, hit)| {
                let name = hit.path.file_name().map_or_else(
                    || hit.path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                let score = match self.kind {
                    OsSearchKind::Content => CONTENT_TOP_SCORE - rank as f64,
                    OsSearchKind::Names => {
                        let fuzzy: f64 = queries
                            .iter_mut()
                            .map(|q| q.score(&name).map_or(0.0, f64::from))
                            .sum::<f64>()
                            / queries.len().max(1) as f64;
                        // The index's own order breaks ties.
                        fuzzy + name_bonus(&name, whole) - rank as f64 * 0.001
                    }
                };
                let completion = if hit.is_dir {
                    format!("{}{}", hit.path.display(), std::path::MAIN_SEPARATOR)
                } else {
                    hit.path.display().to_string()
                };
                self.files
                    .row(&name, hit.path, hit.is_dir, score)
                    .with_autocomplete(completion)
            })
            .collect()
    }
}

/// A row that explains why there are no (more) results. Copying it is the only
/// thing Enter can do with it.
fn status_row(message: &str) -> ResultItem {
    ResultItem::new(
        "files",
        "os-index-status",
        "File index unavailable",
        Action::CopyText {
            text: message.to_owned(),
        },
    )
    .with_subtitle(message)
    .with_icon(IconSource::builtin("file"))
    .with_score(0.0)
}

/// Whether a path from the OS index is clutter: inside a hidden folder (unless
/// `include_hidden`), inside a generated or cache folder the folder index never
/// descends into either, and, on macOS, inside an app bundle or the system
/// and per-user library folders.
pub(crate) fn is_noise(path: &Path, include_hidden: bool, mac: bool) -> bool {
    let text = path.to_string_lossy();
    if mac {
        let in_user_library = text
            .strip_prefix("/Users/")
            .and_then(|rest| rest.split_once('/'))
            .is_some_and(|(_, rest)| rest.starts_with("Library/"));
        if text.contains(".app/")
            || in_user_library
            || ["/System/", "/Library/", "/private/"]
                .iter()
                .any(|prefix| text.starts_with(prefix))
        {
            return true;
        }
    }
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        let hidden = name.starts_with('.') && name != "." && name != "..";
        (hidden && !include_hidden) || PRUNED_DIRS.iter().any(|p| name.eq_ignore_ascii_case(p))
    })
}

/// One of the two OS-index keyword plugins; see the module documentation.
pub struct OsFilesPlugin {
    id: &'static str,
    name: &'static str,
    description: String,
    keyword: String,
    /// How long `query` waits for the index.
    budget: Duration,
    inner: Arc<Inner>,
}

impl OsFilesPlugin {
    pub fn new(
        kind: OsSearchKind,
        keyword: &str,
        files: Arc<FilesPlugin>,
        backend: Arc<Backend>,
        include_hidden: bool,
    ) -> Self {
        let (id, name, what) = match kind {
            OsSearchKind::Names => (
                "files:names",
                "Files (whole disk)",
                "Finds files and folders anywhere on the disk by name, through the operating system's own index.",
            ),
            OsSearchKind::Content => (
                "files:content",
                "Files (contents)",
                "Finds files by the words inside them, through the operating system's own index.",
            ),
        };
        Self {
            id,
            name,
            description: format!("{what} Type `{keyword} <words>`."),
            keyword: keyword.to_owned(),
            budget: SOFT_BUDGET,
            inner: Arc::new(Inner {
                kind,
                files,
                backend,
                include_hidden,
                delivery: Delivery::new(id),
                queue: Mutex::new(Queue::default()),
                wake: Condvar::new(),
            }),
        }
    }

    #[cfg(test)]
    fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }

    fn min_chars(&self) -> usize {
        match self.inner.kind {
            OsSearchKind::Names => MIN_NAME_CHARS,
            OsSearchKind::Content => MIN_CONTENT_CHARS,
        }
    }
}

impl Drop for OsFilesPlugin {
    fn drop(&mut self) {
        self.inner.stop();
    }
}

/// `primary` first, then the rows of `extra` it does not already have.
fn merge(mut primary: Vec<ResultItem>, extra: Vec<ResultItem>) -> Vec<ResultItem> {
    // Windows paths compare case-insensitively.
    let key = |id: &str| {
        if cfg!(windows) {
            id.to_lowercase()
        } else {
            id.to_owned()
        }
    };
    let mut seen: HashSet<String> = primary.iter().map(|item| key(&item.id)).collect();
    primary.extend(extra.into_iter().filter(|item| seen.insert(key(&item.id))));
    primary
}

impl Plugin for OsFilesPlugin {
    fn id(&self) -> &str {
        self.id
    }

    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.keyword)
    }

    /// Only the keyword reaches the OS index: every global query would otherwise
    /// ask it on every keystroke.
    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let input = input.trim();
        let inner = &self.inner;
        let names = inner.kind == OsSearchKind::Names;
        // A typed path is browsed live, like in the folder search.
        if names && path_browse::parse(input, inner.files.home(), cfg!(windows)).is_some() {
            return inner.files.search(input);
        }
        if input.chars().count() < self.min_chars() || search_words(input).is_empty() {
            return Vec::new();
        }

        let folders = if names {
            inner.files.search(input)
        } else {
            Vec::new()
        };
        let indexed = match inner.delivery.begin(input) {
            Begin::Cached(items) => return merge(items, folders),
            Begin::InFlight(id) => id,
            Begin::Send(id) => {
                if !inner.submit(id, input) {
                    inner.delivery.abandon(id);
                    return merge(inner.delivery.wait(id, Duration::ZERO), folders);
                }
                id
            }
        };
        merge(inner.delivery.wait(indexed, self.budget), folders)
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        self.inner.files.execute(item)
    }

    fn attach_notifier(&self, notifier: ResultsNotifier) {
        self.inner.delivery.attach_notifier(notifier);
    }

    fn shutdown(&self) {
        self.inner.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Instant;

    use sevak_core::config::FilesConfig;
    use sevak_core::Modifier;

    use super::*;
    use crate::test_util::MockPlatform;

    fn hit(path: &str) -> OsHit {
        OsHit {
            path: PathBuf::from(path),
            is_dir: false,
        }
    }

    type Answer = dyn Fn(&str) -> Result<Vec<OsHit>, OsSearchError> + Send + Sync;

    /// A fake index: records the searches, optionally holds each one up, and
    /// answers with `answer(text)`.
    struct Fake {
        calls: Mutex<Vec<String>>,
        delay: Duration,
        answer: Box<Answer>,
    }

    impl Fake {
        fn new(
            delay: Duration,
            answer: impl Fn(&str) -> Result<Vec<OsHit>, OsSearchError> + Send + Sync + 'static,
        ) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                delay,
                answer: Box::new(answer),
            })
        }

        fn hits(delay: Duration, paths: &'static [&'static str]) -> Arc<Self> {
            Self::new(delay, move |_| Ok(paths.iter().map(|p| hit(p)).collect()))
        }

        fn backend(self: &Arc<Self>) -> Arc<Backend> {
            let fake = Arc::clone(self);
            Arc::new(move |request| {
                lock(&fake.calls).push(request.text.clone());
                thread::sleep(fake.delay);
                (fake.answer)(&request.text)
            })
        }

        fn calls(&self) -> Vec<String> {
            lock(&self.calls).clone()
        }
    }

    fn folder_index(dir: &Path, names: &[&str]) -> Arc<FilesPlugin> {
        for name in names {
            fs::write(dir.join(name), b"x").unwrap();
        }
        let config = FilesConfig {
            directories: vec![dir.display().to_string()],
            max_depth: 2,
            ..FilesConfig::default()
        };
        let files = Arc::new(FilesPlugin::with_home(config, MockPlatform::empty(), None));
        files.refresh().unwrap();
        files
    }

    fn plugin(kind: OsSearchKind, fake: &Arc<Fake>) -> OsFilesPlugin {
        let files = Arc::new(FilesPlugin::with_home(
            FilesConfig::default(),
            MockPlatform::empty(),
            None,
        ));
        // A generous budget: a fake that answers at once is never "late", even
        // on a loaded machine.
        OsFilesPlugin::new(kind, "ff", files, fake.backend(), false)
            .with_budget(Duration::from_secs(5))
    }

    /// Like `plugin`, but `query` gives the index only 30 ms.
    fn impatient(kind: OsSearchKind, fake: &Arc<Fake>) -> OsFilesPlugin {
        plugin(kind, fake).with_budget(Duration::from_millis(30))
    }

    fn counting_notifier(plugin: &OsFilesPlugin) -> Arc<AtomicUsize> {
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        plugin.attach_notifier(Arc::new(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
        }));
        count
    }

    /// Runs the query again, like the shell does after a notification, until
    /// it has rows.
    fn settle(plugin: &OsFilesPlugin, input: &str) -> Vec<ResultItem> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let items = plugin.query(input);
            if !items.is_empty() || Instant::now() > deadline {
                return items;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_until(what: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn titles(items: &[ResultItem]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[cfg(unix)]
    const HOME: &str = "/home/me";
    #[cfg(windows)]
    const HOME: &str = "C:\\Users\\me";

    fn doc(name: &str) -> String {
        Path::new(HOME)
            .join("docs")
            .join(name)
            .display()
            .to_string()
    }

    #[test]
    fn rows_are_file_rows_with_actions_and_a_completion() {
        let fake = Fake::new(Duration::ZERO, |_| {
            Ok(vec![
                hit(&doc("Report.txt")),
                OsHit {
                    path: PathBuf::from(doc("Reports")),
                    is_dir: true,
                },
            ])
        });
        let plugin = plugin(OsSearchKind::Names, &fake);
        let items = settle(&plugin, "report");
        assert_eq!(titles(&items), ["Report.txt", "Reports"]);

        let file = &items[0];
        let path = PathBuf::from(doc("Report.txt"));
        // Same plugin id as the folder search, so activation, usage statistics
        // and hotkeys treat both alike.
        assert_eq!(file.plugin_id, "files");
        assert_eq!(file.id, format!("files:{}", path.display()));
        assert_eq!(file.action, Action::OpenPath { path: path.clone() });
        assert_eq!(file.secondary[0].label, "Show in folder");
        assert_eq!(file.secondary[0].modifier, Some(Modifier::Ctrl));
        assert_eq!(file.secondary[1].label, "Copy path");
        assert_eq!(file.autocomplete, Some(path.display().to_string()));
        assert_eq!(
            items[1].autocomplete,
            Some(format!("{}{}", doc("Reports"), std::path::MAIN_SEPARATOR))
        );
    }

    #[test]
    fn exact_names_rank_first_and_every_word_counts() {
        let fake = Fake::hits(
            Duration::ZERO,
            &[
                "/data/annual_report_final.docx",
                "/data/report.txt",
                "/data/reporting-tool.md",
            ],
        );
        let plugin = plugin(OsSearchKind::Names, &fake);
        let best = |input: &str| {
            let mut items = settle(&plugin, input);
            items.sort_by(|a, b| b.score.total_cmp(&a.score));
            items.remove(0).title
        };
        assert_eq!(best("report.txt"), "report.txt");
        assert_eq!(best("annual report"), "annual_report_final.docx");
    }

    #[test]
    fn content_rows_keep_the_index_order() {
        let fake = Fake::hits(Duration::ZERO, &["/b/zzz.txt", "/a/aaa.txt", "/c/mmm.txt"]);
        let plugin = plugin(OsSearchKind::Content, &fake);
        let items = settle(&plugin, "budget plan");
        assert_eq!(titles(&items), ["zzz.txt", "aaa.txt", "mmm.txt"]);
        // The search text, not a file name, is what the index received.
        assert_eq!(fake.calls(), ["budget plan"]);
    }

    #[test]
    fn a_slow_index_answers_late_and_the_shell_is_told() {
        let fake = Fake::hits(Duration::from_millis(1500), &["/data/report.txt"]);
        let plugin = impatient(OsSearchKind::Names, &fake);
        let notified = counting_notifier(&plugin);

        let started = Instant::now();
        assert!(plugin.query("report").is_empty());
        assert!(
            started.elapsed() < Duration::from_millis(1000),
            "typing waited {:?}",
            started.elapsed()
        );

        wait_until("the late answer", || notified.load(Ordering::SeqCst) == 1);
        // The re-run is answered from the cache, without asking the index again.
        assert_eq!(titles(&plugin.query("report")), ["report.txt"]);
        assert_eq!(fake.calls().len(), 1);
    }

    #[test]
    fn a_fast_index_needs_no_notification() {
        let fake = Fake::hits(Duration::ZERO, &["/data/report.txt"]);
        let plugin = plugin(OsSearchKind::Names, &fake);
        let notified = counting_notifier(&plugin);
        assert_eq!(titles(&plugin.query("report")), ["report.txt"]);
        assert_eq!(notified.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn requests_typed_past_are_skipped_or_dropped() {
        // The first search is held until released; "ab" and "abc" queue behind it.
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let fake = Fake::new(Duration::ZERO, move |text| {
            if text == "ab" {
                gate.lock().unwrap().recv().unwrap();
            }
            Ok(vec![hit(&format!("/data/{text}.txt"))])
        });
        let plugin = impatient(OsSearchKind::Names, &fake);
        let notified = counting_notifier(&plugin);

        assert!(plugin.query("ab").is_empty());
        wait_until("the first search to start", || fake.calls() == ["ab"]);
        assert!(plugin.query("abc").is_empty());
        assert!(plugin.query("abcd").is_empty());
        release.send(()).unwrap();

        // Only the newest request is answered: "abc" was replaced before it ran,
        // and the answer for "ab" is stale.
        wait_until("the newest answer", || notified.load(Ordering::SeqCst) == 1);
        assert_eq!(titles(&plugin.query("abcd")), ["abcd.txt"]);
        assert_eq!(fake.calls(), ["ab", "abcd"]);
        assert_eq!(notified.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stale_answer_is_not_shown_for_a_different_query() {
        let fake = Fake::hits(Duration::from_millis(300), &["/data/old.txt"]);
        let plugin = impatient(OsSearchKind::Names, &fake);
        assert!(plugin.query("old").is_empty());
        wait_until("the first search", || fake.calls() == ["old"]);
        // Unrelated text typed before the answer: when it arrives it is dropped.
        assert!(plugin.query("new").is_empty());
        wait_until("both searches", || fake.calls().len() == 2);
        let rows = settle(&plugin, "new");
        assert_eq!(rows.len(), 1);
        assert_eq!(fake.calls(), ["old", "new"]);
    }

    #[test]
    fn a_missing_index_becomes_a_row_that_says_so() {
        let fake = Fake::new(Duration::ZERO, |_| {
            Err(OsSearchError::Unavailable("install plocate".to_owned()))
        });
        let plugin = plugin(OsSearchKind::Names, &fake);
        let items = settle(&plugin, "report");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "File index unavailable");
        assert_eq!(items[0].subtitle, "install plocate");
        assert_eq!(items[0].score, 0.0);
        assert!(plugin.execute(&items[0]).is_ok());
    }

    #[test]
    fn a_failing_index_is_reported_and_a_timeout_is_silent() {
        let failing = Fake::new(Duration::ZERO, |_| {
            Err(OsSearchError::Failed("boom".into()))
        });
        let items = settle(&plugin(OsSearchKind::Content, &failing), "report");
        assert!(items[0].subtitle.contains("boom"));

        let slow = Fake::new(Duration::ZERO, |_| Err(OsSearchError::TimedOut));
        let plugin = plugin(OsSearchKind::Content, &slow);
        let _ = plugin.query("report");
        wait_until("the search", || slow.calls().len() == 1);
        thread::sleep(Duration::from_millis(50));
        assert!(plugin.query("report").is_empty());
    }

    #[test]
    fn a_panicking_index_does_not_take_the_worker_down() {
        let fake = Fake::new(Duration::ZERO, |text| {
            assert_ne!(text, "boom", "backend bug");
            Ok(vec![hit("/data/fine.txt")])
        });
        let plugin = plugin(OsSearchKind::Names, &fake);
        let items = settle(&plugin, "boom");
        assert!(items[0].subtitle.contains("crashed"));
        assert_eq!(titles(&settle(&plugin, "fine")), ["fine.txt"]);
    }

    #[test]
    fn short_or_wordless_input_never_reaches_the_index() {
        let fake = Fake::hits(Duration::ZERO, &["/data/a.txt"]);
        let names = plugin(OsSearchKind::Names, &fake);
        let content = plugin(OsSearchKind::Content, &fake);
        for input in ["", " ", "a", " a ", "- -", "&&"] {
            assert!(names.query(input).is_empty(), "{input:?}");
        }
        assert!(content.query("ab").is_empty());
        assert!(fake.calls().is_empty());
        assert!(!settle(&content, "abc").is_empty());
    }

    #[test]
    fn names_also_list_the_folder_index_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let files = folder_index(dir.path(), &["report.txt", "reports.txt"]);
        let fake = Fake::new(Duration::from_millis(300), |_| Ok(Vec::new()));
        let plugin = OsFilesPlugin::new(OsSearchKind::Names, "ff", files, fake.backend(), false);

        // The index has not answered, but the folder index already has rows.
        let early = plugin.query("report");
        assert_eq!(early.len(), 2, "{:?}", titles(&early));
        assert!(early.iter().all(|item| item.plugin_id == "files"));
    }

    #[test]
    fn merging_lists_a_file_found_twice_once() {
        let row = |path: &str, score: f64| {
            ResultItem::new("files", path, path, Action::CopyText { text: path.into() })
                .with_score(score)
        };
        let merged = merge(
            vec![row("/a", 3.0), row("/b", 2.0)],
            vec![row("/b", 9.0), row("/c", 1.0)],
        );
        assert_eq!(titles(&merged), ["/a", "/b", "/c"]);
        // The index's own row wins.
        assert_eq!(merged[1].score, 2.0);
    }

    #[test]
    fn contents_do_not_use_the_folder_index() {
        let dir = tempfile::tempdir().unwrap();
        let files = folder_index(dir.path(), &["report.txt"]);
        let fake = Fake::new(Duration::ZERO, |_| Ok(Vec::new()));
        let plugin = OsFilesPlugin::new(OsSearchKind::Content, "in", files, fake.backend(), false);
        assert!(plugin.query("report").is_empty());
    }

    #[test]
    fn a_typed_path_is_browsed_and_not_searched() {
        let dir = tempfile::tempdir().unwrap();
        let files = folder_index(dir.path(), &["alpha.txt", "beta.txt"]);
        let fake = Fake::hits(Duration::ZERO, &["/data/x.txt"]);
        let plugin = OsFilesPlugin::new(OsSearchKind::Names, "ff", files, fake.backend(), false);
        let typed = format!("{}{}al", dir.path().display(), std::path::MAIN_SEPARATOR);
        assert_eq!(titles(&plugin.query(&typed)), ["alpha.txt"]);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn noise_is_hidden_pruned_or_macos_clutter() {
        let noise = |p: &str, hidden: bool, mac: bool| is_noise(Path::new(p), hidden, mac);
        assert!(!noise("/home/me/docs/a.txt", false, false));
        assert!(noise("/home/me/.config/a.txt", false, false));
        assert!(!noise("/home/me/.config/a.txt", true, false));
        assert!(noise("/home/me/.hidden", false, false));
        assert!(noise("/home/me/proj/node_modules/x/index.js", true, false));
        assert!(noise("/home/me/proj/Target/debug/x", true, false));
        assert!(!noise("/home/me/archive/x.txt", false, false));
        // macOS only.
        assert!(!noise("/Library/Fonts/a.ttf", false, false));
        assert!(noise("/Library/Fonts/a.ttf", false, true));
        assert!(noise("/System/Applications/x", false, true));
        assert!(noise(
            "/Applications/Safari.app/Contents/Info.plist",
            false,
            true
        ));
        assert!(noise("/Users/me/Library/Caches/x", false, true));
        assert!(!noise("/Users/me/Documents/Library/x", false, true));
        assert!(!noise("/Users/me/Documents/a.txt", false, true));
    }

    #[test]
    fn noise_is_dropped_from_the_rows() {
        let fake = Fake::hits(
            Duration::ZERO,
            &[
                "/home/me/.cache/report.txt",
                "/home/me/proj/node_modules/report.js",
                "/home/me/docs/report.md",
            ],
        );
        let plugin = plugin(OsSearchKind::Names, &fake);
        assert_eq!(titles(&settle(&plugin, "report")), ["report.md"]);
    }

    fn family_ids(config: &Config) -> Vec<String> {
        files_family(
            config,
            &(MockPlatform::empty() as Arc<dyn PlatformProvider>),
        )
        .iter()
        .map(|p| p.id().to_owned())
        .collect()
    }

    #[test]
    fn the_family_follows_the_configuration() {
        let mut config = Config::default();
        assert_eq!(
            family_ids(&config),
            ["files", "files:names", "files:content"]
        );
        let plugins = files_family(
            &config,
            &(MockPlatform::empty() as Arc<dyn PlatformProvider>),
        );
        assert_eq!(plugins[0].keyword(), Some("f"));
        assert_eq!(plugins[1].keyword(), Some("ff"));
        assert_eq!(plugins[2].keyword(), Some("in"));
        assert!(plugins.iter().all(|p| !p.description().is_empty()));
        assert!(!plugins[1].global() && !plugins[2].global());

        config.files.content_keyword = " ".into();
        assert_eq!(family_ids(&config), ["files", "files:names"]);

        config.files.index_keyword = "IN".into();
        config.files.content_keyword = "in".into();
        // The second `in` clashes with the first.
        assert_eq!(family_ids(&config), ["files", "files:names"]);

        config.files.index_keyword = "F".into();
        assert_eq!(family_ids(&config), ["files", "files:content"]);

        config.files.use_os_index = false;
        assert_eq!(family_ids(&config), ["files"]);
    }

    /// The whole path on this computer's real OS index: the family built from
    /// the default configuration, a real search, and its rows. Ignored by
    /// default; run with
    /// `cargo test -p sevak-plugins real_os_index -- --ignored --nocapture`.
    #[test]
    #[ignore = "asks this computer's real file index"]
    fn real_os_index_answers_ff_and_in() {
        let platform: Arc<dyn PlatformProvider> = Arc::from(sevak_platform::native_provider());
        let plugins = files_family(&Config::default(), &platform);
        for (id, input) in [("files:names", "windows"), ("files:content", "the")] {
            let plugin = plugins.iter().find(|p| p.id() == id).unwrap();
            let started = Instant::now();
            let mut items = plugin.query(input);
            let deadline = Instant::now() + Duration::from_secs(10);
            while items.is_empty() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
                items = plugin.query(input);
            }
            println!(
                "{id}: {} rows in {} ms",
                items.len(),
                started.elapsed().as_millis()
            );
            assert!(!items.is_empty(), "{id}");
            assert!(items.iter().all(|item| item.plugin_id == "files"));
        }
    }

    #[test]
    fn dropping_the_plugin_stops_the_worker() {
        let fake = Fake::hits(Duration::ZERO, &["/data/report.txt"]);
        let plugin = plugin(OsSearchKind::Names, &fake);
        let inner = Arc::clone(&plugin.inner);
        let _ = settle(&plugin, "report");
        assert!(lock(&inner.queue).worker_started);
        drop(plugin);
        assert!(lock(&inner.queue).stopped);
        // The worker held one reference; once it exits only ours remains.
        wait_until("the worker to exit", || Arc::strong_count(&inner) == 1);
    }
}
