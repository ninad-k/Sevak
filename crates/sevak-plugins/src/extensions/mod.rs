//! Extensions: writing them, packaging them, and installing them from the
//! gallery.
//!
//! | File | Concern |
//! |---|---|
//! | `package.rs` | the `.sevakext` package: build, read, verify |
//! | `plugin.rs` | `ext` / `store` in the launcher |
//! | `store.rs` | browse the gallery, install, update, uninstall; receipts and the catalog cache |
//!
//! The trust model is the script plugins' (`crate::script`): an installed
//! extension is a folder in `<config dir>/plugins` that does nothing until the
//! user allows it, with the approval bound to the folder's contents. A native
//! extension adds a compiled program to those contents; see
//! `crate::script::native` and `docs/writing-extensions-in-rust.md`.

pub mod package;
pub mod plugin;
pub mod store;

#[cfg(test)]
pub(crate) mod testing;

pub use package::{
    check_manifest, lint_manifest, summarize, Builder, ExtensionPackage, PackedFile, Summary,
    CHECKSUMS_FILE, EXTENSION,
};
pub use plugin::{ExtensionsHost, ExtensionsPlugin, Hooks, Report};
pub use store::{
    is_newer, Catalog, CatalogItem, CatalogView, ExtensionStore, InstalledItem, ItemKind, Outcome,
    Quiesce, Receipt, SharedTransport, State, StoreDirs,
};
