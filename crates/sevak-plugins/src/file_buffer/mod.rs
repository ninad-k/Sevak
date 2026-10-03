//! The file buffer: collect several files and folders from the file results,
//! then act on all of them at once (open, copy, move, trash, zip, ...).
//!
//! [`FileBuffer`] is the collection; [`BufferAction`] the things that can be
//! done with it; [`run`] does one of them and reports a [`Outcome`] that says
//! how many items it worked for. The disk work is in [`ops`]. This module has
//! no UI and no threads: the shell calls [`run`] off the UI thread and turns the
//! progress callback into events.

pub mod ops;

use std::fs;
use std::path::{Path, PathBuf};

use sevak_core::ShellConfig;
use sevak_platform::PlatformProvider;

use crate::files::{expand_home, home_dir};
pub use ops::Progress;
use ops::{Failure, Report};

/// The most items the buffer holds; a click-happy hold on Alt+Down stops here.
pub const MAX_ITEMS: usize = 500;
/// More items than this opened at once ask first.
const OPEN_ALL_CONFIRM_ABOVE: usize = 10;
/// More folders or terminals than this opened at once ask first.
const FOLDERS_CONFIRM_ABOVE: usize = 3;
/// Names listed in a confirmation question.
const LISTED_IN_QUESTION: usize = 5;

/// The collected paths, in the order they were added, each once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileBuffer {
    items: Vec<PathBuf>,
}

/// What [`FileBuffer::add`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    Yes,
    /// It was in the buffer already.
    Already,
    /// The buffer holds [`MAX_ITEMS`] already.
    Full,
}

impl FileBuffer {
    pub fn items(&self) -> &[PathBuf] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn add(&mut self, path: PathBuf) -> Added {
        if self.items.contains(&path) {
            Added::Already
        } else if self.items.len() >= MAX_ITEMS {
            Added::Full
        } else {
            self.items.push(path);
            Added::Yes
        }
    }

    pub fn remove_last(&mut self) -> Option<PathBuf> {
        self.items.pop()
    }

    /// Removes the item at `index`, if there is one.
    pub fn remove(&mut self, index: usize) -> Option<PathBuf> {
        (index < self.items.len()).then(|| self.items.remove(index))
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Drops `paths` (the ones an action has used up) from the buffer.
    pub fn forget(&mut self, paths: &[PathBuf]) {
        self.items.retain(|item| !paths.contains(item));
    }
}

/// Something to do with every item in the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferAction {
    OpenAll,
    ShowInFolder,
    CopyPaths,
    CopyFiles,
    MoveTo,
    CopyTo,
    Trash,
    Zip,
    OpenInTerminal,
}

impl BufferAction {
    /// In the order the action list shows them.
    pub const ALL: [Self; 9] = [
        Self::OpenAll,
        Self::ShowInFolder,
        Self::CopyPaths,
        Self::CopyFiles,
        Self::MoveTo,
        Self::CopyTo,
        Self::Trash,
        Self::Zip,
        Self::OpenInTerminal,
    ];

    /// The stable name the UI sends back.
    pub fn key(self) -> &'static str {
        match self {
            Self::OpenAll => "open_all",
            Self::ShowInFolder => "show_in_folder",
            Self::CopyPaths => "copy_paths",
            Self::CopyFiles => "copy_files",
            Self::MoveTo => "move_to",
            Self::CopyTo => "copy_to",
            Self::Trash => "trash",
            Self::Zip => "zip",
            Self::OpenInTerminal => "open_terminal",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.key() == key)
    }

    /// As the action list words it.
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAll => "Open all",
            Self::ShowInFolder => "Show in folder",
            Self::CopyPaths => "Copy paths",
            Self::CopyFiles => "Copy files to clipboard",
            Self::MoveTo => "Move to…",
            Self::CopyTo => "Copy to…",
            Self::Trash => "Move to Trash",
            Self::Zip => "Compress to .zip",
            Self::OpenInTerminal => "Open in terminal",
        }
    }

    /// Asks for a destination folder first.
    pub fn needs_destination(self) -> bool {
        matches!(self, Self::MoveTo | Self::CopyTo)
    }

    /// Takes the items away from where they were (so a failure to finish is
    /// worth a second look, and the ones that went are dropped from the buffer).
    pub fn consumes_items(self) -> bool {
        matches!(self, Self::MoveTo | Self::Trash)
    }

    /// Leaves Sevak for another program: the window should get out of the way.
    pub fn hands_over(self) -> bool {
        matches!(
            self,
            Self::OpenAll | Self::ShowInFolder | Self::OpenInTerminal
        )
    }

    /// May take a while (disk work), so it runs with a progress row.
    pub fn is_long(self) -> bool {
        matches!(self, Self::MoveTo | Self::CopyTo | Self::Trash | Self::Zip)
    }
}

/// What an action did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// One or two sentences for the user: what happened, and the first failure.
    pub message: String,
    pub total: usize,
    pub failed: usize,
    /// The items the action fully handled, as they were given.
    pub done: Vec<PathBuf>,
}

impl Outcome {
    pub fn succeeded(&self) -> usize {
        self.total - self.failed
    }

    /// Nothing went wrong.
    pub fn is_ok(&self) -> bool {
        self.failed == 0
    }

    /// Nothing worked at all.
    pub fn is_failure(&self) -> bool {
        self.total > 0 && self.failed == self.total
    }

    fn failure(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            total: 1,
            failed: 1,
            done: Vec::new(),
        }
    }
}

/// What [`run`] works with besides the items.
pub struct RunContext<'a> {
    pub platform: &'a dyn PlatformProvider,
    pub shell: &'a ShellConfig,
}

/// Turns the folder a user typed or picked into a path that exists: `~` is
/// expanded, and it must be a folder.
pub fn resolve_destination(typed: &str) -> Result<PathBuf, String> {
    let text = typed.trim();
    if text.is_empty() {
        return Err("Type or pick a destination folder.".to_owned());
    }
    let path = expand_home(text, home_dir().as_deref());
    if !path.is_absolute() {
        return Err(
            "Type the destination as a full path, starting with ~, / or a drive letter.".to_owned(),
        );
    }
    match fs::metadata(&path) {
        Ok(meta) if meta.is_dir() => Ok(path),
        Ok(_) => Err(format!("{} is not a folder.", path.display())),
        Err(_) => Err(format!("{} does not exist.", path.display())),
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// The folder as a short name for messages.
fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// "Moved 2 of 3 items to Documents. a.txt: access denied" and the like.
/// `past` is the verb in the past tense ("Moved"), `base` what could not be
/// done in the present ("move"), `tail` what follows the count ("to Documents").
fn summary(report: &Report, past: &str, base: &str, tail: &str) -> String {
    let tail = if tail.is_empty() {
        String::new()
    } else {
        format!(" {tail}")
    };
    let done = report.total - report.failed.len();
    let head = if report.failed.is_empty() {
        format!("{past} {}{tail}.", plural(done, "item", "items"))
    } else if done == 0 {
        format!(
            "Could not {base} {}{tail}.",
            if report.total == 1 {
                "the item".to_owned()
            } else {
                format!("any of the {} items", report.total)
            }
        )
    } else {
        format!(
            "{past} {done} of {}{tail}.",
            plural(report.total, "item", "items")
        )
    };
    match report.failed.first() {
        Some(Failure { path, reason }) => {
            let more = report.failed.len() - 1;
            let extra = if more > 0 {
                format!(" (and {more} more)")
            } else {
                String::new()
            };
            format!("{head} {}: {reason}{extra}", display_name(path))
        }
        None => head,
    }
}

fn outcome_of(report: Report, message: String) -> Outcome {
    Outcome {
        message,
        total: report.total,
        failed: report.failed.len(),
        done: report.done,
    }
}

/// The first item of each distinct parent folder, in order.
fn one_per_folder(items: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen: Vec<&Path> = Vec::new();
    let mut picked = Vec::new();
    for item in items {
        let parent = item.parent().unwrap_or(item);
        if !seen.contains(&parent) {
            seen.push(parent);
            picked.push(item.clone());
        }
    }
    picked
}

/// The folders to open terminals in: a folder itself, a file's folder; each once.
fn terminal_folders(items: &[PathBuf]) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for item in items {
        let folder = if item.is_dir() {
            item.clone()
        } else {
            item.parent()
                .map_or_else(|| item.clone(), Path::to_path_buf)
        };
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    folders
}

/// The question to put to the user before `action` runs, if it needs one:
/// anything that moves or trashes, and opening a lot of things at once.
pub fn confirmation(
    action: BufferAction,
    items: &[PathBuf],
    destination: Option<&Path>,
) -> Option<String> {
    let list = || {
        let mut lines: Vec<String> = items
            .iter()
            .take(LISTED_IN_QUESTION)
            .map(|item| format!("• {}", display_name(item)))
            .collect();
        if items.len() > LISTED_IN_QUESTION {
            lines.push(format!("…and {} more", items.len() - LISTED_IN_QUESTION));
        }
        lines.join("\n")
    };
    let count = plural(items.len(), "item", "items");
    match action {
        BufferAction::Trash => Some(format!("Move {count} to the Trash?\n\n{}", list())),
        BufferAction::MoveTo => {
            let to = destination.map_or_else(String::new, |d| format!(" to {}", d.display()));
            Some(format!("Move {count}{to}?\n\n{}", list()))
        }
        BufferAction::OpenAll if items.len() > OPEN_ALL_CONFIRM_ABOVE => {
            Some(format!("Open {count} at once?"))
        }
        BufferAction::ShowInFolder if one_per_folder(items).len() > FOLDERS_CONFIRM_ABOVE => Some(
            format!("Open {} folder windows?", one_per_folder(items).len()),
        ),
        BufferAction::OpenInTerminal if terminal_folders(items).len() > FOLDERS_CONFIRM_ABOVE => {
            Some(format!(
                "Open {} terminal windows?",
                terminal_folders(items).len()
            ))
        }
        _ => None,
    }
}

/// Does `action` for `items`. `destination` is the folder for the actions that
/// need one ([`BufferAction::needs_destination`]). `progress` hears about the
/// slow ones as they go (see [`ops::Progress`]). Never panics; every problem
/// is part of the [`Outcome`].
pub fn run(
    context: &RunContext<'_>,
    action: BufferAction,
    items: &[PathBuf],
    destination: Option<&Path>,
    progress: Progress<'_>,
) -> Outcome {
    if items.is_empty() {
        return Outcome::failure("The file buffer is empty.");
    }
    let platform = context.platform;
    match action {
        BufferAction::OpenAll => {
            let report = ops::run_each(items, progress, |item| {
                platform.open_path(item).map_err(|e| e.to_string())
            });
            let message = summary(&report, "Opened", "open", "");
            outcome_of(report, message)
        }
        BufferAction::ShowInFolder => {
            let folders = one_per_folder(items);
            let report = ops::run_each(&folders, progress, |item| {
                platform.reveal_path(item).map_err(|e| e.to_string())
            });
            let mut message = summary(&report, "Showed", "show", "");
            message = message.replacen("item", "folder", 1);
            outcome_of(report, message)
        }
        BufferAction::CopyPaths => {
            let text = items
                .iter()
                .map(|item| item.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\n");
            match platform.set_clipboard_text(&text) {
                Ok(()) => Outcome {
                    message: format!("Copied {}.", plural(items.len(), "path", "paths")),
                    total: items.len(),
                    failed: 0,
                    done: items.to_vec(),
                },
                Err(err) => Outcome::failure(format!("Could not copy the paths: {err}")),
            }
        }
        BufferAction::CopyFiles => {
            let existing: Vec<PathBuf> = items
                .iter()
                .filter(|item| fs::symlink_metadata(item).is_ok())
                .cloned()
                .collect();
            if existing.is_empty() {
                return Outcome::failure("None of the items exist any more.");
            }
            match platform.set_clipboard_files(&existing) {
                Ok(()) => {
                    let missing = items.len() - existing.len();
                    let mut message = format!(
                        "Copied {} to the clipboard; paste them in your file manager.",
                        plural(existing.len(), "item", "items")
                    );
                    if missing > 0 {
                        message.push_str(&format!(" {missing} no longer exist."));
                    }
                    Outcome {
                        message,
                        total: items.len(),
                        failed: missing,
                        done: existing,
                    }
                }
                Err(err) => Outcome::failure(format!("Could not copy the files: {err}")),
            }
        }
        BufferAction::MoveTo | BufferAction::CopyTo => {
            let Some(dest) = destination else {
                return Outcome::failure("Pick a destination folder first.");
            };
            let to = format!("to {}", folder_name(dest));
            if action == BufferAction::MoveTo {
                let report = ops::move_items(items, dest, progress);
                let message = summary(&report, "Moved", "move", &to);
                outcome_of(report, message)
            } else {
                let report = ops::copy_items(items, dest, progress);
                let message = summary(&report, "Copied", "copy", &to);
                outcome_of(report, message)
            }
        }
        BufferAction::Trash => {
            let report = ops::trash_items(platform, items, progress);
            let message = summary(&report, "Moved", "move", "to the Trash");
            outcome_of(report, message)
        }
        BufferAction::Zip => match ops::zip_items(items, progress) {
            Ok(zipped) => {
                let mut message = format!(
                    "Compressed {} into {}.",
                    plural(zipped.files, "file", "files"),
                    display_name(&zipped.archive)
                );
                if zipped.skipped > 0 {
                    message.push_str(&format!(
                        " Left out {} that could not be read or are links.",
                        zipped.skipped
                    ));
                }
                Outcome {
                    message,
                    total: items.len(),
                    failed: 0,
                    done: items.to_vec(),
                }
            }
            Err(reason) => Outcome::failure(format!("Could not compress: {reason}")),
        },
        BufferAction::OpenInTerminal => {
            let folders = terminal_folders(items);
            let report = ops::run_each(&folders, progress, |folder| {
                platform
                    .open_terminal_in(folder, context.shell)
                    .map_err(|e| e.to_string())
            });
            let message = summary(&report, "Opened", "open", "").replacen("item", "terminal", 1);
            outcome_of(report, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;

    fn paths(items: &[&str]) -> Vec<PathBuf> {
        items.iter().map(PathBuf::from).collect()
    }

    fn context(platform: &MockPlatform) -> RunContext<'_> {
        // The mock only needs the config to hand it back.
        static SHELL: std::sync::OnceLock<ShellConfig> = std::sync::OnceLock::new();
        RunContext {
            platform,
            shell: SHELL.get_or_init(ShellConfig::default),
        }
    }

    fn quiet() -> impl FnMut(usize, usize, &str) {
        |_, _, _| {}
    }

    #[test]
    fn the_buffer_keeps_order_and_ignores_repeats() {
        let mut buffer = FileBuffer::default();
        assert_eq!(buffer.add("/a".into()), Added::Yes);
        assert_eq!(buffer.add("/b".into()), Added::Yes);
        assert_eq!(buffer.add("/a".into()), Added::Already);
        assert_eq!(buffer.items(), paths(&["/a", "/b"]));
        assert_eq!(buffer.remove_last(), Some(PathBuf::from("/b")));
        assert_eq!(buffer.remove(5), None);
        assert_eq!(buffer.remove(0), Some(PathBuf::from("/a")));
        assert!(buffer.is_empty());
        assert_eq!(buffer.remove_last(), None);
    }

    #[test]
    fn the_buffer_has_a_limit() {
        let mut buffer = FileBuffer::default();
        for i in 0..MAX_ITEMS {
            assert_eq!(buffer.add(PathBuf::from(format!("/f{i}"))), Added::Yes);
        }
        assert_eq!(buffer.add("/one-more".into()), Added::Full);
        assert_eq!(buffer.len(), MAX_ITEMS);
    }

    #[test]
    fn forgetting_drops_only_the_named_items() {
        let mut buffer = FileBuffer::default();
        for item in ["/a", "/b", "/c"] {
            buffer.add(item.into());
        }
        buffer.forget(&paths(&["/a", "/c", "/zzz"]));
        assert_eq!(buffer.items(), paths(&["/b"]));
        buffer.clear();
        assert!(buffer.is_empty());
    }

    #[test]
    fn action_keys_round_trip() {
        for action in BufferAction::ALL {
            assert_eq!(BufferAction::from_key(action.key()), Some(action));
            assert!(!action.label().is_empty());
        }
        assert_eq!(BufferAction::from_key("rm_rf"), None);
        let keys: std::collections::HashSet<_> =
            BufferAction::ALL.iter().map(|a| a.key()).collect();
        assert_eq!(keys.len(), BufferAction::ALL.len());
    }

    #[test]
    fn only_moving_actions_need_a_destination_and_a_confirmation() {
        let items = paths(&["/a/x.txt", "/a/y.txt"]);
        let dest = Path::new("/dest");
        for action in BufferAction::ALL {
            let asks = confirmation(action, &items, Some(dest)).is_some();
            assert_eq!(
                asks,
                matches!(action, BufferAction::Trash | BufferAction::MoveTo),
                "{action:?}"
            );
            assert_eq!(
                action.needs_destination(),
                matches!(action, BufferAction::MoveTo | BufferAction::CopyTo)
            );
        }
        let question = confirmation(BufferAction::Trash, &items, None).unwrap();
        assert!(question.contains("Move 2 items to the Trash"), "{question}");
        assert!(question.contains("x.txt") && question.contains("y.txt"));
        let question = confirmation(BufferAction::MoveTo, &items, Some(dest)).unwrap();
        assert!(
            question.contains("to /dest") || question.contains("dest"),
            "{question}"
        );
    }

    #[test]
    fn long_lists_are_abbreviated_in_the_question() {
        let items: Vec<PathBuf> = (0..9)
            .map(|i| PathBuf::from(format!("/a/f{i}.txt")))
            .collect();
        let question = confirmation(BufferAction::Trash, &items, None).unwrap();
        assert!(question.contains("f4.txt"));
        assert!(!question.contains("f5.txt"));
        assert!(question.contains("and 4 more"), "{question}");
    }

    #[test]
    fn opening_many_things_asks_first() {
        let many: Vec<PathBuf> = (0..11).map(|i| PathBuf::from(format!("/d{i}/f"))).collect();
        assert!(confirmation(BufferAction::OpenAll, &many, None).is_some());
        assert!(confirmation(BufferAction::OpenAll, &many[..10], None).is_none());
        assert!(confirmation(BufferAction::ShowInFolder, &many[..4], None).is_some());
        assert!(confirmation(BufferAction::ShowInFolder, &many[..3], None).is_none());
        assert!(confirmation(BufferAction::OpenInTerminal, &many[..4], None).is_some());
    }

    #[test]
    fn open_all_opens_every_item_in_order() {
        let platform = MockPlatform::empty();
        let items = paths(&["/a/1.txt", "/b/2.txt"]);
        let outcome = run(
            &context(&platform),
            BufferAction::OpenAll,
            &items,
            None,
            &mut quiet(),
        );
        assert!(outcome.is_ok());
        assert_eq!(outcome.message, "Opened 2 items.");
        assert_eq!(*platform.opened_paths.lock().unwrap(), items);
    }

    #[test]
    fn show_in_folder_reveals_one_item_per_folder() {
        let platform = MockPlatform::empty();
        let items = paths(&["/a/1.txt", "/a/2.txt", "/b/3.txt"]);
        let outcome = run(
            &context(&platform),
            BufferAction::ShowInFolder,
            &items,
            None,
            &mut quiet(),
        );
        assert_eq!(
            *platform.revealed.lock().unwrap(),
            paths(&["/a/1.txt", "/b/3.txt"])
        );
        assert_eq!(outcome.message, "Showed 2 folders.");
    }

    #[test]
    fn copy_paths_puts_one_path_per_line_on_the_clipboard() {
        let platform = MockPlatform::empty();
        let items = paths(&["/a/1.txt", "/b/2 x.txt"]);
        let outcome = run(
            &context(&platform),
            BufferAction::CopyPaths,
            &items,
            None,
            &mut quiet(),
        );
        assert_eq!(outcome.message, "Copied 2 paths.");
        let copied = platform.clipboard.lock().unwrap().clone();
        assert_eq!(copied.len(), 1);
        let lines: Vec<&str> = copied[0].lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("1.txt") && lines[1].ends_with("2 x.txt"));
    }

    #[test]
    fn copy_files_hands_the_clipboard_a_file_list() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        std::fs::write(&a, "x").unwrap();
        let gone = dir.path().join("gone.txt");

        let platform = MockPlatform::empty();
        let items = vec![a.clone(), gone];
        let outcome = run(
            &context(&platform),
            BufferAction::CopyFiles,
            &items,
            None,
            &mut quiet(),
        );
        assert_eq!(*platform.clipboard_files.lock().unwrap(), [vec![a]]);
        assert_eq!(outcome.total, 2);
        assert_eq!(outcome.failed, 1);
        assert!(
            outcome.message.contains("1 no longer exist"),
            "{}",
            outcome.message
        );

        let nothing = run(
            &context(&platform),
            BufferAction::CopyFiles,
            &[dir.path().join("none.txt")],
            None,
            &mut quiet(),
        );
        assert!(nothing.is_failure());
        assert_eq!(platform.clipboard_files.lock().unwrap().len(), 1);
    }

    #[test]
    fn open_in_terminal_uses_folders_and_the_parent_of_files() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("proj");
        std::fs::create_dir_all(&folder).unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "x").unwrap();
        let inside = folder.join("main.rs");
        std::fs::write(&inside, "x").unwrap();

        let platform = MockPlatform::empty();
        let outcome = run(
            &context(&platform),
            BufferAction::OpenInTerminal,
            &[folder.clone(), file, inside],
            None,
            &mut quiet(),
        );
        // `proj` itself, the temp dir (parent of note.txt); main.rs's parent is `proj` again.
        assert_eq!(
            *platform.terminal_dirs.lock().unwrap(),
            [folder, dir.path().to_path_buf()]
        );
        assert_eq!(outcome.message, "Opened 2 terminals.");
    }

    #[test]
    fn moving_and_copying_report_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("Archive");
        std::fs::create_dir_all(&dest).unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let platform = MockPlatform::empty();

        let copied = run(
            &context(&platform),
            BufferAction::CopyTo,
            &[a.clone(), b.clone()],
            Some(&dest),
            &mut quiet(),
        );
        assert_eq!(copied.message, "Copied 2 items to Archive.");
        assert!(a.exists() && dest.join("b.txt").exists());

        let moved = run(
            &context(&platform),
            BufferAction::MoveTo,
            &[a.clone(), b.clone(), dir.path().join("missing.txt")],
            Some(&dest),
            &mut quiet(),
        );
        assert_eq!(moved.total, 3);
        assert_eq!(moved.failed, 1);
        assert_eq!(moved.done, [a.clone(), b]);
        assert!(
            moved.message.starts_with("Moved 2 of 3 items to Archive."),
            "{}",
            moved.message
        );
        assert!(moved.message.contains("missing.txt"), "{}", moved.message);
        assert!(!a.exists());
        // The copies from before were not overwritten.
        assert!(dest.join("a (2).txt").exists());
    }

    #[test]
    fn a_move_without_a_destination_does_nothing() {
        let platform = MockPlatform::empty();
        let outcome = run(
            &context(&platform),
            BufferAction::MoveTo,
            &paths(&["/a"]),
            None,
            &mut quiet(),
        );
        assert!(outcome.is_failure());
    }

    #[test]
    fn trash_goes_through_the_platform_and_reports_refusals() {
        let platform = MockPlatform::empty();
        platform.trash_refuses.lock().unwrap().push("/t/b".into());
        let items = paths(&["/t/a", "/t/b", "/t/c"]);
        let outcome = run(
            &context(&platform),
            BufferAction::Trash,
            &items,
            None,
            &mut quiet(),
        );
        assert_eq!(outcome.total, 3);
        assert_eq!(outcome.failed, 1);
        assert_eq!(outcome.done, paths(&["/t/a", "/t/c"]));
        assert!(
            outcome
                .message
                .starts_with("Moved 2 of 3 items to the Trash."),
            "{}",
            outcome.message
        );
        assert!(outcome.message.contains("b:"), "{}", outcome.message);

        let all_refused = MockPlatform::empty();
        all_refused
            .trash_refuses
            .lock()
            .unwrap()
            .push("/t/a".into());
        let outcome = run(
            &context(&all_refused),
            BufferAction::Trash,
            &paths(&["/t/a"]),
            None,
            &mut quiet(),
        );
        assert!(outcome.is_failure());
        assert!(
            outcome
                .message
                .starts_with("Could not move the item to the Trash."),
            "{}",
            outcome.message
        );
    }

    #[test]
    fn zip_says_where_the_archive_went() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        let platform = MockPlatform::empty();
        let outcome = run(
            &context(&platform),
            BufferAction::Zip,
            &[dir.path().join("a.txt"), dir.path().join("b.txt")],
            None,
            &mut quiet(),
        );
        assert!(outcome.is_ok());
        assert_eq!(outcome.message, "Compressed 2 files into Archive.zip.");
        assert!(dir.path().join("Archive.zip").is_file());
    }

    #[test]
    fn an_empty_buffer_is_refused_for_every_action() {
        let platform = MockPlatform::empty();
        for action in BufferAction::ALL {
            let outcome = run(
                &context(&platform),
                action,
                &[],
                Some(Path::new("/x")),
                &mut quiet(),
            );
            assert!(outcome.is_failure(), "{action:?}");
        }
        assert!(platform.opened_paths.lock().unwrap().is_empty());
        assert!(platform.trashed.lock().unwrap().is_empty());
    }

    #[test]
    fn destinations_must_be_existing_folders() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();

        let typed = format!("{}{}", dir.path().display(), std::path::MAIN_SEPARATOR);
        assert_eq!(resolve_destination(&typed).unwrap(), PathBuf::from(&typed));
        assert!(resolve_destination("").is_err());
        assert!(resolve_destination("  ").is_err());
        assert!(resolve_destination("relative/dir")
            .unwrap_err()
            .contains("full path"));
        assert!(resolve_destination(&file.display().to_string())
            .unwrap_err()
            .contains("not a folder"));
        let missing = dir.path().join("nope").display().to_string();
        assert!(resolve_destination(&missing)
            .unwrap_err()
            .contains("does not exist"));
    }

    #[test]
    fn progress_is_reported_while_a_long_action_runs() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("d");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dir.path().join("a"), "a").unwrap();
        std::fs::write(dir.path().join("b"), "b").unwrap();
        let platform = MockPlatform::empty();
        let mut calls = Vec::new();
        run(
            &context(&platform),
            BufferAction::CopyTo,
            &[dir.path().join("a"), dir.path().join("b")],
            Some(&dest),
            &mut |done, total, _| calls.push((done, total)),
        );
        assert_eq!(calls, [(0, 2), (1, 2), (2, 2)]);
    }
}
