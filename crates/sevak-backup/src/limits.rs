//! Size caps. A backup file comes from outside (a download, a shared drive, a
//! USB stick), so every number that decides how much is read or written is
//! bounded here.

/// The biggest backup file that is opened.
pub const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
/// The most the files in a backup may add up to once unpacked.
pub const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
/// The largest single file.
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// The largest `manifest.json`.
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Entries in the archive, files and folders together.
pub const MAX_ENTRIES: usize = 3_000;
/// Files in one plugin or workflow folder.
pub const MAX_FOLDER_FILES: usize = 300;
/// A path inside the archive.
pub const MAX_PATH_BYTES: usize = 240;
/// Components in a path inside the archive.
pub const MAX_PATH_DEPTH: usize = 8;
/// The largest settings, snippets or web search file.
pub const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;
/// How many restore snapshots are kept for "Undo restore".
pub const KEEP_SNAPSHOTS: usize = 5;
