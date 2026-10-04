//! A small Sevak extension written in Rust.
//!
//! Type the extension's keyword and a name (`rh Ada`) and Sevak lists a few
//! greetings; Enter copies the one you pick. "Remember" saves the name in the
//! extension's own data folder, and the keyword alone lists the saved names.
//!
//! Everything Sevak-specific is in `main` and `answer`; the rest is ordinary
//! Rust. The protocol (JSON lines on stdin and stdout) is handled by
//! `sevak-extension-sdk`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sevak_extension_sdk::{plugin_data_dir, Action, Error, Extension, Icon, Item, Query};

/// The file the remembered names live in, inside the extension's data folder.
const NAMES_FILE: &str = "names.txt";

fn main() {
    let dir = plugin_data_dir();
    let for_query = dir.clone();
    Extension::new(move |query| answer(query, for_query.as_deref()))
        // The user picked a row whose action is `Action::custom(..)`.
        .on_execute(move |execute| remember(dir.as_deref(), &execute.payload))
        .run();
}

/// The rows for what the user typed after the keyword.
fn answer(query: &Query, data_dir: Option<&Path>) -> Result<Vec<Item>, Error> {
    if query.is_empty() {
        let mut rows = vec![Item::new("Type a name")
            .key("hint")
            .subtitle("For example: rh Ada")
            .icon(Icon::builtin("plugin"))];
        rows.extend(remembered(data_dir).into_iter().map(|name| {
            Item::new(format!("Hello again, {name}!"))
                .key(format!("again:{name}"))
                .copy_on_enter()
        }));
        return Ok(rows);
    }
    let name = query.text();
    Ok(vec![
        Item::new(format!("Hello, {name}!"))
            .key("hello")
            .subtitle("Enter copies the greeting")
            .icon(Icon::builtin("copy"))
            .copy_on_enter(),
        Item::new(format!("HELLO, {}!", name.to_uppercase()))
            .key("shout")
            .subtitle("Enter copies it, louder")
            .copy_on_enter(),
        Item::new(format!("Remember {name}"))
            .key("remember")
            .subtitle("Saved in the extension's data folder")
            .action(Action::custom(name)),
    ])
}

/// The names saved so far, oldest first. No data folder or no file is no names.
fn remembered(data_dir: Option<&Path>) -> Vec<String> {
    let Some(path) = names_path(data_dir) else {
        return Vec::new();
    };
    fs::read_to_string(path)
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Appends `name` to the saved names.
fn remember(data_dir: Option<&Path>, name: &str) -> Result<(), Error> {
    let path = names_path(data_dir).ok_or_else(|| Error::new("no data folder to save in"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", name.replace(['\r', '\n'], " "))?;
    Ok(())
}

fn names_path(data_dir: Option<&Path>) -> Option<PathBuf> {
    data_dir.map(|dir| dir.join(NAMES_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A data folder that is removed when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir()
                .join(format!("sevak-ext-{label}-{}-{nanos}", std::process::id()));
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn titles(items: &[Item]) -> Vec<String> {
        items.iter().map(|item| format!("{item:?}")).collect()
    }

    #[test]
    fn a_name_gets_three_rows() {
        let rows = answer(&Query::new("Ada"), None).unwrap();
        assert_eq!(rows.len(), 3);
        let text = titles(&rows).join("\n");
        assert!(text.contains("Hello, Ada!"));
        assert!(text.contains("HELLO, ADA!"));
        assert!(text.contains("Remember Ada"));
        assert!(rows.iter().all(|row| row.problems().is_empty()));
    }

    #[test]
    fn the_keyword_alone_shows_a_hint() {
        let rows = answer(&Query::new("  "), None).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(titles(&rows)[0].contains("Type a name"));
    }

    #[test]
    fn remembered_names_come_back() {
        let dir = TempDir::new("names");
        assert!(remembered(Some(&dir.0)).is_empty());
        remember(Some(&dir.0), "Ada").unwrap();
        remember(Some(&dir.0), "Grace\nHopper").unwrap();
        assert_eq!(remembered(Some(&dir.0)), ["Ada", "Grace Hopper"]);
        let rows = answer(&Query::new(""), Some(&dir.0)).unwrap();
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn remembering_without_a_data_folder_is_an_error() {
        assert!(remember(None, "Ada").is_err());
    }
}
