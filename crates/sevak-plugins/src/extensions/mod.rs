//! Extensions: writing them, packaging them, and installing them from the
//! gallery.
//!
//! | File | Concern |
//! |---|---|
//! | `package.rs` | the `.sevakext` package: build, read, verify |
//!
//! The trust model is the script plugins' (`crate::script`): an installed
//! extension is a folder in `<config dir>/plugins` that does nothing until the
//! user allows it, with the approval bound to the folder's contents. A native
//! extension adds a compiled program to those contents; see
//! `crate::script::native` and `docs/writing-extensions-in-rust.md`.

pub mod package;

pub use package::{
    check_manifest, lint_manifest, summarize, Builder, ExtensionPackage, PackedFile, Summary,
    CHECKSUMS_FILE, EXTENSION,
};
