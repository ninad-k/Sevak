//! Tests: round trips, categories, the allowlist, hostile archives, merge and
//! replace, atomicity, undo, approvals and the schedule. Everything runs on
//! temporary folders; nothing here touches the real Sevak configuration.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{FixedOffset, TimeZone};
use sevak_core::Config;
use sevak_plugins::script::{Scanned as ScriptScanned, ScriptPluginHost};
use sevak_plugins::workflow::{NoSink, Scanned as WorkflowScanned, WorkflowHost};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::apply::Hooks;
use crate::archive::{build, read, Meta};
use crate::auto::{self, AutoConfig, Reason, Schedule};
use crate::backup::{contents, create, default_file_name, Destination};
use crate::category::{
    classify, config_key_category, is_sensitive_key, Category, Mode, LEFT_OUT_TABLES,
    NEVER_INCLUDED_FILES, SETTINGS_TABLES,
};
use crate::error::Error;
use crate::item::Item;
use crate::manifest::{Kind, FORMAT_VERSION};
use crate::plan::Change;
use crate::restore::{
    create_snapshot, latest_snapshot, preview, restore, restore_with, undo_restore, undo_with,
    RestoreOptions,
};
use crate::state::State;
use crate::util::{sha256_hex, Roots, Stamp};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

struct Env {
    _tmp: tempfile::TempDir,
    roots: Roots,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("config");
        let data_dir = tmp.path().join("data");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(&data_dir).unwrap();
        let roots = Roots {
            config_file: config_dir.join("config.toml"),
            config_dir,
            data_dir,
        };
        Self { _tmp: tmp, roots }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.roots.config_dir.join(rel)
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn config_text(&self) -> String {
        fs::read_to_string(&self.roots.config_file).unwrap_or_default()
    }

    fn config(&self) -> Config {
        Config::from_toml_str(&self.config_text()).unwrap()
    }

    /// Every file under the config folder (staging leftovers excluded), by
    /// relative path.
    fn tree(&self) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        walk(&self.roots.config_dir, &self.roots.config_dir, &mut out);
        out
    }

    fn data(&self, rel: &str, text: &str) {
        let path = self.roots.data_dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with(".sevak-") {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, out);
        } else {
            out.insert(rel, fs::read(&path).unwrap());
        }
    }
}

fn stamp_at(second: u32) -> Stamp {
    let moment = FixedOffset::east_opt(0)
        .unwrap()
        .with_ymd_and_hms(2026, 10, 4, 15, 30, second)
        .unwrap();
    Stamp::at(moment)
}

fn stamp() -> Stamp {
    stamp_at(45)
}

fn accept(_: &Config) -> Result<(), String> {
    Ok(())
}

fn options(second: u32) -> RestoreOptions<'static> {
    RestoreOptions {
        validate: &accept,
        stamp: stamp_at(second),
        expect_sha256: None,
    }
}

const PLUGIN: &str = "protocol = 1\nkeyword = \"hello\"\ncommand = [\"python3\", \"main.py\"]\n";
const PLUGIN_OTHER: &str = "protocol = 1\nkeyword = \"other\"\ncommand = [\"python3\", \"o.py\"]\n";
const WORKFLOW_SCRIPTED: &str = r#"
name = "Scripted"
[[node]]
id = "k"
type = "keyword"
keyword = "scr"
[[node]]
id = "run"
type = "run_script"
command = ["python3", "main.py"]
[[connection]]
from = "k"
to = "run"
"#;
const WORKFLOW_PLAIN: &str = r#"
name = "Plain"
[[node]]
id = "k"
type = "keyword"
keyword = "plain"
[[node]]
id = "o"
type = "open_url"
url = "https://example.com/{query}"
[[connection]]
from = "k"
to = "o"
"#;
const NORD: &str = "name = \"Nord\"\n[dark]\nbackground = \"#2e3440\"\n";

const CONFIG: &str = r#"
[general]
hotkey = "Alt+Space"
api_key = "SECRET-API-KEY-123"

[search]
max_results = 12
fallback_web_search = "g"

[appearance]
theme = "dark"
theme_file = "themes/Nord.toml"
custom_css = "my.css"

[plugins]
disabled = ["emoji"]

[onepassword]
enabled = true
account = "acct-SECRET-1P"

[snippets]
auto_expand = true
prefix = ";"

[[snippet]]
name = "Sig"
text = "Regards"

[[snippet]]
name = "Addr"
keyword = "addr"
text = "1 Main St"

[[web_search]]
keyword = "g"
name = "Google"
url = "https://www.google.com/search?q={query}"

[[web_search]]
keyword = "ddg"
name = "DuckDuckGo"
url = "https://duckduckgo.com/?q={query}"

[[hotkey]]
key = "Ctrl+Alt+F"
run = "apps:firefox.desktop"

[ai]
api_key = "AI-SECRET-KEY-999"
"#;

/// A configuration folder with something in every category, plus decoys of
/// everything that must never be backed up.
fn kitchen_sink() -> Env {
    let env = Env::new();
    env.write("config.toml", CONFIG);
    env.write("themes/Nord.toml", NORD);
    env.write("my.css", ":root { --accent: red; }");
    env.write("plugins/hello/plugin.toml", PLUGIN);
    env.write("plugins/hello/main.py", "print('hello')");
    env.write("plugins/hello/lib/util.py", "x = 1");
    env.write("plugins/other/plugin.toml", PLUGIN_OTHER);
    env.write("plugins/other/o.py", "print('other')");
    env.write("workflows/runner/workflow.toml", WORKFLOW_SCRIPTED);
    env.write("workflows/runner/main.py", "print('run')");
    env.write("workflows/plain/workflow.toml", WORKFLOW_PLAIN);

    // Decoys: none of these may end up in a backup.
    env.write("secrets.toml", "token = \"DECOY-SECRETS-TOML\"");
    env.write("api_key.txt", "DECOY-API-KEY-FILE");
    env.write("ai-key.txt", "DECOY-AI-KEY");
    env.write("1password-cache.json", "{\"DECOY\": 1}");
    env.write("plugins/hello/.env", "TOKEN=DECOY-ENV");
    env.write(
        "plugins/hello/token.json",
        "{\"DECOY\": \"TOKEN-IN-PLUGIN\"}",
    );
    env.write("plugins/hello/server.pem", "DECOY-PEM");
    env.write(
        "plugins/hello/node_modules/dep/index.js",
        "DECOY-NODE-MODULES",
    );
    env.write("plugins/hello/__pycache__/main.cpython.pyc", "DECOY-PYC");
    env.write("workflows/runner/credentials.json", "{\"DECOY\": \"CRED\"}");
    env.data("clipboard-history.json", "[\"DECOY-CLIPBOARD-HISTORY\"]");
    env.data("clipboard/clip-1.png", "DECOY-PNG");
    env.data("usage.json", "{\"DECOY\": \"usage\"}");
    env.data("history.db", "DECOY-ENCRYPTED-HISTORY-DB");
    env.data("logs/sevak.2026-10-04.log", "DECOY-LOG");
    env.data("hotkey-takeover.json", "{}");
    env.data("currency-rates.json", "{}");
    env
}

fn all_categories() -> Vec<Category> {
    Category::ALL.to_vec()
}

fn backup_bytes(env: &Env, categories: &[Category]) -> Vec<u8> {
    let collected = crate::collect::collect(&env.roots, categories).unwrap();
    build(
        &collected.items,
        &Meta {
            kind: Kind::Manual,
            categories,
            restore_of: &[],
            stamp: &stamp(),
        },
    )
    .unwrap()
}

#[cfg(unix)]
fn make_file(env: &Env, categories: &[Category], dest: &Path) -> PathBuf {
    create(
        &env.roots,
        categories,
        Kind::Manual,
        &stamp(),
        Destination::File(dest),
    )
    .unwrap()
    .path
}

// ---------------------------------------------------------------------------
// Round trip, categories
// ---------------------------------------------------------------------------

#[test]
fn a_backup_restores_into_a_fresh_install() {
    let source = kitchen_sink();
    let bytes = backup_bytes(&source, &all_categories());
    let backup = read(&bytes).unwrap();
    assert_eq!(backup.manifest.version, FORMAT_VERSION);
    assert_eq!(backup.manifest.categories, all_categories());

    let target = Env::new();
    let report = restore(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Replace,
        &options(1),
    )
    .unwrap();
    assert!(report.changed);

    // The configuration means the same thing, apart from what is left out.
    let (from, to) = (source.config(), target.config());
    assert_eq!(from.general, to.general);
    assert_eq!(from.search, to.search);
    assert_eq!(from.appearance, to.appearance);
    assert_eq!(from.plugins, to.plugins);
    assert_eq!(from.snippets, to.snippets);
    assert_eq!(from.snippet, to.snippet);
    assert_eq!(from.web_search, to.web_search);
    assert_eq!(from.hotkeys, to.hotkeys);
    assert_eq!(to.onepassword, Config::default().onepassword);

    // Files travel byte for byte.
    for rel in [
        "themes/Nord.toml",
        "my.css",
        "plugins/hello/plugin.toml",
        "plugins/hello/main.py",
        "plugins/hello/lib/util.py",
        "plugins/other/plugin.toml",
        "workflows/runner/workflow.toml",
        "workflows/runner/main.py",
        "workflows/plain/workflow.toml",
    ] {
        assert_eq!(
            fs::read(target.path(rel)).unwrap(),
            fs::read(source.path(rel)).unwrap(),
            "{rel}"
        );
    }
}

#[test]
fn only_the_chosen_categories_are_backed_up() {
    let source = kitchen_sink();
    let bytes = backup_bytes(&source, &[Category::Themes, Category::Snippets]);
    let backup = read(&bytes).unwrap();
    assert_eq!(
        backup.manifest.categories,
        vec![Category::Snippets, Category::Themes]
    );
    let paths: Vec<&str> = backup.items.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(
        paths,
        ["custom-css/my.css", "snippets.toml", "themes/Nord.toml"]
    );

    // Restoring a category the backup does not have is refused, not a no-op.
    let target = Env::new();
    let err = restore(
        &target.roots,
        &backup,
        &[Category::Plugins],
        Mode::Merge,
        &options(1),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Nothing(_)), "{err}");
    assert!(target.tree().is_empty());
}

#[test]
fn contents_lists_what_would_be_included_without_writing() {
    let source = kitchen_sink();
    let listing = contents(&source.roots, &all_categories()).unwrap();
    let by_category: BTreeMap<Category, &Vec<String>> = listing
        .categories
        .iter()
        .map(|c| (c.category, &c.entries))
        .collect();
    assert!(by_category[&Category::Settings]
        .iter()
        .any(|e| e == "[general]"));
    assert!(by_category[&Category::Plugins]
        .iter()
        .any(|e| e.starts_with("hello (")));
    assert!(listing.left_out.contains(&"[onepassword]".to_owned()));
    assert!(listing.left_out.contains(&"[ai]".to_owned()));
    assert!(!listing.categories.is_empty() && listing.bytes > 0);
}

#[test]
fn the_default_file_name_carries_the_time() {
    assert_eq!(
        default_file_name(&stamp()),
        "sevak-backup-20261004-153045.sevakbackup"
    );
}

#[test]
fn a_destination_without_an_extension_gets_one() {
    let source = kitchen_sink();
    let out = tempfile::tempdir().unwrap();
    let report = create(
        &source.roots,
        &[Category::Themes],
        Kind::Manual,
        &stamp(),
        Destination::File(&out.path().join("nested").join("mine")),
    )
    .unwrap();
    assert_eq!(
        report.path,
        out.path().join("nested").join("mine.sevakbackup")
    );
    assert!(read(&fs::read(&report.path).unwrap()).is_ok());
    // The last backup is remembered.
    let state = State::load(&source.roots);
    assert_eq!(state.last.unwrap().path, report.path.display().to_string());
}

#[cfg(unix)]
#[test]
fn a_backup_file_is_readable_by_its_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let source = kitchen_sink();
    let out = tempfile::tempdir().unwrap();
    let path = make_file(
        &source,
        &all_categories(),
        &out.path().join("b.sevakbackup"),
    );
    let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[test]
fn the_execute_bit_survives_but_nothing_wider() {
    use std::os::unix::fs::PermissionsExt;
    let source = kitchen_sink();
    fs::set_permissions(
        source.path("plugins/hello/main.py"),
        fs::Permissions::from_mode(0o4755),
    )
    .unwrap();
    let backup = read(&backup_bytes(&source, &[Category::Plugins])).unwrap();
    let target = Env::new();
    restore(
        &target.roots,
        &backup,
        &[Category::Plugins],
        Mode::Replace,
        &options(1),
    )
    .unwrap();
    let mode = |rel: &str| fs::metadata(target.path(rel)).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode("plugins/hello/main.py"), 0o755);
    assert_eq!(mode("plugins/hello/plugin.toml"), 0o644);
}

// ---------------------------------------------------------------------------
// The allowlist: nothing sensitive gets in
// ---------------------------------------------------------------------------

#[test]
fn secrets_and_private_data_never_reach_the_archive() {
    let source = kitchen_sink();
    let bytes = backup_bytes(&source, &all_categories());
    let backup = read(&bytes).unwrap();

    for item in &backup.items {
        let text = String::from_utf8_lossy(&item.data);
        assert!(
            !text.contains("DECOY") && !text.contains("SECRET"),
            "{} leaks: {text}",
            item.path
        );
        assert!(
            classify(&item.path).is_some(),
            "{} is not allowlisted",
            item.path
        );
    }
    // The decoy files themselves are not in it.
    let paths: Vec<&str> = backup.items.iter().map(|i| i.path.as_str()).collect();
    for banned in [
        "secrets.toml",
        "ai-key.txt",
        "plugins/hello/.env",
        "plugins/hello/token.json",
        "plugins/hello/server.pem",
        "workflows/runner/credentials.json",
    ] {
        assert!(!paths.contains(&banned), "{banned}");
    }
    assert!(!paths.iter().any(|p| p.contains("node_modules")
        || p.contains("__pycache__")
        || p.contains("history")
        || p.contains("clipboard")
        || p.contains("usage")
        || p.contains("approval")));

    // Not even compressed bytes mention them.
    let raw = String::from_utf8_lossy(&bytes);
    assert!(!raw.contains("SECRET"));
    // Sections that are not allowlisted are not there; secret-looking keys inside
    // allowlisted sections are dropped.
    let settings = String::from_utf8(
        backup
            .items
            .iter()
            .find(|i| i.path == "settings.toml")
            .unwrap()
            .data
            .clone(),
    )
    .unwrap();
    assert!(!settings.contains("onepassword"));
    assert!(!settings.contains("api_key"));
    assert!(!settings.contains("[ai]"));
    assert!(settings.contains("hotkey = \"Alt+Space\""));
}

#[test]
fn every_config_section_is_sorted_into_a_category_or_left_out_on_purpose() {
    // This test fails when someone adds a section to `Config` (an AI assistant's
    // `[ai]` with an API key, say): decide in `sevak_backup::category` whether it
    // belongs in a backup before it can slip in.
    let table = toml::Table::try_from(Config::default()).unwrap();
    for key in table.keys() {
        assert!(
            config_key_category(key).is_some() || LEFT_OUT_TABLES.contains(&key.as_str()),
            "the config section `{key}` is not classified for backups: add it to \
             SETTINGS_TABLES (if it is harmless) or to LEFT_OUT_TABLES (if it is sensitive)"
        );
    }
    for key in SETTINGS_TABLES {
        assert!(table.contains_key(*key), "`{key}` is not a config section");
    }
}

#[test]
fn no_real_option_is_mistaken_for_a_secret() {
    fn check(value: &toml::Value, path: &str) {
        match value {
            toml::Value::Table(table) => {
                for (key, child) in table {
                    let here = format!("{path}.{key}");
                    assert!(
                        !is_sensitive_key(key),
                        "{here} would be scrubbed from backups"
                    );
                    check(child, &here);
                }
            }
            toml::Value::Array(items) => items.iter().for_each(|item| check(item, path)),
            _ => {}
        }
    }
    let table = toml::Table::try_from(Config::default()).unwrap();
    for (key, value) in &table {
        if !LEFT_OUT_TABLES.contains(&key.as_str()) {
            check(value, key);
        }
    }
}

#[test]
fn the_files_sevak_keeps_for_itself_are_not_on_the_allowlist() {
    for name in NEVER_INCLUDED_FILES {
        assert_eq!(classify(name), None, "{name} must never pass the allowlist");
    }
    for denied in [
        "plugins/hello",
        "plugins/hello/.env",
        "plugins/../x.toml",
        "plugins//x/y",
        "plugins/hello/CON",
        "plugins/hello/a:b",
        "plugins/hello/a\\b",
        "plugins/Bad Name./x.py",
        "themes/Nord.TOML",
        "themes/.toml",
        "themes/a/b.toml",
        "settings.toml/x",
        "manifest.json",
        "workflows/w/node_modules/x.js",
        "workflows/w/secret.txt",
    ] {
        assert_eq!(classify(denied), None, "{denied}");
    }
    for allowed in [
        ("settings.toml", Category::Settings),
        ("snippets.toml", Category::Snippets),
        ("web-search.toml", Category::WebSearch),
        ("themes/Nord.toml", Category::Themes),
        ("custom-css/my.css", Category::Themes),
        ("plugins/hello/main.py", Category::Plugins),
        ("plugins/hello/lib/token_counter.py", Category::Plugins),
        ("workflows/runner/workflow.toml", Category::Workflows),
    ] {
        assert_eq!(classify(allowed.0), Some(allowed.1), "{}", allowed.0);
    }
}

#[test]
fn a_plugin_with_a_broken_manifest_is_left_out_with_a_warning() {
    let source = kitchen_sink();
    source.write("plugins/broken/plugin.toml", "this is not a manifest");
    source.write("plugins/no-manifest/main.py", "x");
    let collected = crate::collect::collect(&source.roots, &[Category::Plugins]).unwrap();
    assert!(collected.items.iter().all(|i| !i.path.contains("broken")));
    assert!(collected.warnings.iter().any(|w| w.contains("broken")));
    assert!(collected.warnings.iter().any(|w| w.contains("no-manifest")));
}

#[test]
fn an_oversized_plugin_is_left_out_whole() {
    let source = kitchen_sink();
    source.write("plugins/big/plugin.toml", PLUGIN_OTHER);
    let big = source.path("plugins/big/blob.bin");
    fs::write(big, vec![0u8; 9 * 1024 * 1024]).unwrap();
    let collected = crate::collect::collect(&source.roots, &[Category::Plugins]).unwrap();
    assert!(collected
        .items
        .iter()
        .all(|i| !i.path.starts_with("plugins/big/")));
    assert!(collected
        .warnings
        .iter()
        .any(|w| w.contains("big") && w.contains("larger")));
}

#[test]
fn links_are_not_followed_into_a_backup() {
    let source = kitchen_sink();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("target.txt"), "OUTSIDE-SECRET").unwrap();
    let link = source.path("plugins/hello/link.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path().join("target.txt"), &link).unwrap();
    #[cfg(windows)]
    if std::os::windows::fs::symlink_file(outside.path().join("target.txt"), &link).is_err() {
        // Creating links needs a privilege on Windows; nothing to test then.
        return;
    }
    let collected = crate::collect::collect(&source.roots, &[Category::Plugins]).unwrap();
    assert!(collected
        .items
        .iter()
        .all(|i| !i.path.ends_with("link.txt")));
    assert!(collected
        .items
        .iter()
        .all(|i| !String::from_utf8_lossy(&i.data).contains("OUTSIDE-SECRET")));
}

// ---------------------------------------------------------------------------
// Hostile and damaged archives
// ---------------------------------------------------------------------------

fn raw_zip(entries: &[(&str, &[u8], Option<u32>)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let base = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data, mode) in entries {
        let options = mode.map_or(base, |m| base.unix_permissions(m));
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn manifest_json(version: u32, files: &[(&str, &str, &[u8])]) -> Vec<u8> {
    let listed: Vec<serde_json::Value> = files
        .iter()
        .map(|(path, category, data)| {
            serde_json::json!({
                "path": path,
                "category": category,
                "size": data.len(),
                "sha256": sha256_hex(data),
            })
        })
        .collect();
    let mut categories: Vec<&str> = files.iter().map(|f| f.1).collect();
    categories.dedup();
    serde_json::to_vec(&serde_json::json!({
        "format": "sevak-backup",
        "version": version,
        "app_version": "0.1.0",
        "created_unix": 0,
        "created": "2026-10-04T15:30:45+00:00",
        "platform": "linux",
        "kind": "manual",
        "categories": categories,
        "files": listed,
    }))
    .unwrap()
}

/// A zip with a manifest listing `files` (and holding them).
fn crafted(files: &[(&str, &str, &[u8])]) -> Vec<u8> {
    let manifest = manifest_json(FORMAT_VERSION, files);
    let mut entries: Vec<(&str, &[u8], Option<u32>)> = vec![("manifest.json", &manifest, None)];
    entries.extend(files.iter().map(|(p, _, d)| (*p, *d, None)));
    raw_zip(&entries)
}

fn invalid(bytes: &[u8]) -> String {
    match read(bytes) {
        Err(Error::Invalid(message)) => message,
        Err(other) => panic!("expected an invalid backup, got: {other}"),
        Ok(_) => panic!("the hostile archive was accepted"),
    }
}

#[test]
fn a_well_formed_crafted_archive_is_accepted() {
    // Guards the helper: the hostile cases below differ from this in one way each.
    let bytes = crafted(&[("themes/Nord.toml", "themes", NORD.as_bytes())]);
    assert!(read(&bytes).is_ok());
}

#[test]
fn zip_slip_and_absolute_paths_are_refused() {
    for name in [
        "../evil.toml",
        "themes/../../evil.toml",
        "/etc/passwd",
        "C:/Windows/evil.toml",
        "themes\\..\\evil.toml",
        "plugins/p/../../../evil.py",
        "./themes/Nord.toml",
        "themes//Nord.toml",
    ] {
        let bytes = crafted(&[(name, "themes", NORD.as_bytes())]);
        let message = invalid(&bytes);
        assert!(message.contains("does not belong"), "{name}: {message}");
    }
}

#[test]
fn files_outside_the_allowlist_are_refused_even_when_the_manifest_lists_them() {
    for (name, category) in [
        ("config.toml", "settings"),
        ("script-plugin-approvals.json", "settings"),
        ("clipboard-history.json", "settings"),
        ("plugins/p/.env", "plugins"),
        ("plugins/p/token.json", "plugins"),
    ] {
        let bytes = crafted(&[(name, category, b"x")]);
        assert!(invalid(&bytes).contains("does not belong"), "{name}");
    }
}

#[test]
fn a_file_the_manifest_does_not_list_is_refused() {
    let manifest = manifest_json(1, &[("themes/Nord.toml", "themes", NORD.as_bytes())]);
    let bytes = raw_zip(&[
        ("manifest.json", &manifest, None),
        ("themes/Nord.toml", NORD.as_bytes(), None),
        ("themes/Extra.toml", b"name = \"x\"", None),
    ]);
    assert!(invalid(&bytes).contains("does not list"));
}

#[test]
fn a_missing_file_and_a_wrong_checksum_are_refused() {
    let manifest = manifest_json(1, &[("themes/Nord.toml", "themes", NORD.as_bytes())]);
    let missing = raw_zip(&[("manifest.json", &manifest, None)]);
    assert!(invalid(&missing).contains("missing"));

    let tampered = raw_zip(&[
        ("manifest.json", &manifest, None),
        ("themes/Nord.toml", b"name = \"Tampered\"\n", None),
    ]);
    assert!(invalid(&tampered).contains("checksum"));
}

#[test]
fn links_inside_the_archive_are_refused() {
    let manifest = manifest_json(1, &[]);
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer.start_file("manifest.json", options).unwrap();
    writer.write_all(&manifest).unwrap();
    writer
        .add_symlink("themes/Nord.toml", "/etc/passwd", options)
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    assert!(invalid(&bytes).contains("link"));
}

#[test]
fn huge_files_and_zip_bombs_are_refused() {
    // One file over the per-file cap, tiny once compressed.
    let big = vec![0u8; 9 * 1024 * 1024];
    let bytes = crafted(&[("plugins/p/blob.bin", "plugins", &big)]);
    assert!(bytes.len() < 100 * 1024, "the bomb should compress well");
    assert!(invalid(&bytes).contains("too large"));

    // Many files under the cap that add up past the total cap.
    let chunk = vec![0u8; 7 * 1024 * 1024];
    let names: Vec<String> = (0..10).map(|i| format!("plugins/p/blob{i}.bin")).collect();
    let files: Vec<(&str, &str, &[u8])> = names
        .iter()
        .map(|n| (n.as_str(), "plugins", chunk.as_slice()))
        .collect();
    let bytes = crafted(&files);
    assert!(invalid(&bytes).contains("far more data"));
}

#[test]
fn a_huge_manifest_is_refused() {
    let padding = vec![b' '; 2 * 1024 * 1024];
    let bytes = raw_zip(&[("manifest.json", &padding, None)]);
    assert!(invalid(&bytes).contains("too large"));
}

#[test]
fn garbage_truncated_and_foreign_files_are_refused() {
    assert!(matches!(read(b""), Err(Error::Invalid(_))));
    assert!(matches!(
        read(b"PK\x03\x04 definitely not a zip"),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(read(&[7u8; 4096]), Err(Error::Invalid(_))));

    let source = kitchen_sink();
    let good = backup_bytes(&source, &all_categories());
    assert!(read(&good).is_ok());
    for cut in [good.len() / 2, good.len() - 10, 30] {
        assert!(
            matches!(read(&good[..cut]), Err(Error::Invalid(_))),
            "cut at {cut}"
        );
    }
    // Bit flips inside the compressed data break the checksum or the stream.
    let mut flipped = good.clone();
    let middle = flipped.len() / 2;
    flipped[middle] ^= 0xff;
    assert!(read(&flipped).is_err());

    // A zip that is not ours.
    let other = raw_zip(&[("hello.txt", b"hi", None)]);
    assert!(invalid(&other).contains("manifest"));
    let foreign = raw_zip(&[(
        "manifest.json",
        br#"{"format": "something-else", "version": 1}"#,
        None,
    )]);
    assert!(invalid(&foreign).contains("not a Sevak backup"));
}

#[test]
fn a_bad_manifest_is_refused() {
    for body in [
        &b"not json"[..],
        br#"{"format": "sevak-backup"}"#,
        br#"{"format": "sevak-backup", "version": "one"}"#,
        br#"{"format": "sevak-backup", "version": 0}"#,
        br#"{"format": "sevak-backup", "version": 1, "files": 3}"#,
    ] {
        let bytes = raw_zip(&[("manifest.json", body, None)]);
        assert!(matches!(read(&bytes), Err(Error::Invalid(_))));
    }
}

#[test]
fn a_newer_backup_format_is_refused_with_a_clear_message() {
    let manifest = manifest_json(FORMAT_VERSION + 1, &[]);
    let bytes = raw_zip(&[("manifest.json", &manifest, None)]);
    let err = read(&bytes).unwrap_err();
    assert!(matches!(err, Error::TooNew { found, .. } if found == FORMAT_VERSION + 1));
    let message = err.to_string();
    assert!(message.contains("newer version of Sevak"), "{message}");
    assert!(message.contains("update Sevak"), "{message}");
}

#[test]
fn names_that_differ_only_in_case_are_refused() {
    let manifest = manifest_json(1, &[]);
    let bytes = raw_zip(&[
        ("manifest.json", &manifest, None),
        ("themes/a.toml", b"name = \"a\"", None),
        ("themes/A.toml", b"name = \"b\"", None),
    ]);
    assert!(invalid(&bytes).contains("twice"));
}

#[test]
fn contents_are_checked_against_their_schema() {
    // Settings with a value of the wrong type.
    let bad = b"[window]\nwidth = \"wide\"\n";
    assert!(invalid(&crafted(&[("settings.toml", "settings", bad)])).contains("not valid"));
    // Not TOML at all.
    assert!(invalid(&crafted(&[("settings.toml", "settings", b"[[[")])).contains("not valid"));
    // A web search without {query}.
    let engine = b"[[web_search]]\nkeyword = \"x\"\nname = \"X\"\nurl = \"https://x.test/\"\n";
    assert!(invalid(&crafted(&[("web-search.toml", "web_search", engine)])).contains("{query}"));
    // A snippet with a list for a text.
    let snippet = b"[[snippet]]\nname = \"a\"\ntext = [1, 2]\n";
    assert!(invalid(&crafted(&[("snippets.toml", "snippets", snippet)])).contains("snippets"));
    // A theme that is not TOML.
    assert!(invalid(&crafted(&[("themes/x.toml", "themes", b"= =")])).contains("not valid"));
    // A plugin without a manifest, with an invalid one; a workflow with a broken graph.
    assert!(invalid(&crafted(&[("plugins/p/main.py", "plugins", b"x")])).contains("plugin.toml"));
    assert!(
        invalid(&crafted(&[("plugins/p/plugin.toml", "plugins", b"nope")])).contains("not valid")
    );
    assert!(invalid(&crafted(&[(
        "workflows/w/workflow.toml",
        "workflows",
        b"name = \"x\"\n[[node]]\nid = \"a\"\ntype = \"nonsense\"\n"
    )]))
    .contains("not valid"));
}

#[test]
fn unknown_sections_and_secret_looking_keys_in_a_backup_are_ignored_with_a_warning() {
    let text = b"[general]\nhotkey = \"Alt+Space\"\napi_key = \"HOSTILE\"\n\n[onepassword]\nenabled = true\n\n[ai]\nkey = \"x\"\n";
    let bytes = crafted(&[("settings.toml", "settings", text)]);
    let backup = read(&bytes).unwrap();
    let joined = backup.warnings.join("\n");
    assert!(joined.contains("[onepassword]"), "{joined}");
    assert!(joined.contains("[ai]"), "{joined}");
    assert!(joined.contains("general.api_key"), "{joined}");

    // Restoring applies only the allowlisted, scrubbed part.
    let target = Env::new();
    restore(
        &target.roots,
        &backup,
        &[Category::Settings],
        Mode::Merge,
        &options(1),
    )
    .unwrap();
    let written = target.config_text();
    assert!(written.contains("Alt+Space"));
    assert!(!written.contains("HOSTILE"));
    assert!(!written.contains("[ai]"));
    assert_eq!(target.config().onepassword, Config::default().onepassword);
}

// ---------------------------------------------------------------------------
// Merge and replace
// ---------------------------------------------------------------------------

/// A target that already has its own settings, snippets, engines, themes and
/// plugins, so merge and replace can be told apart.
fn lived_in() -> Env {
    let env = Env::new();
    env.write(
        "config.toml",
        r#"# my comment
[general]
hotkey = "Ctrl+Space"
launch_at_login = true

[search]
max_results = 5

[onepassword]
account = "mine"

[[snippet]]
name = "Local"
text = "only here"

[[snippet]]
name = "Sig"
text = "old signature"

[[web_search]]
keyword = "mine"
name = "Mine"
url = "https://mine.test/?q={query}"

[[web_search]]
keyword = "g"
name = "Old Google"
url = "https://old.test/?q={query}"
"#,
    );
    env.write("themes/Local.toml", "name = \"Local\"\n");
    env.write("themes/Nord.toml", "name = \"Old Nord\"\n");
    env.write(
        "plugins/local/plugin.toml",
        PLUGIN_OTHER.replace("other", "local").as_str(),
    );
    env.write("plugins/local/o.py", "x");
    env.write("plugins/hello/plugin.toml", PLUGIN);
    env.write("plugins/hello/main.py", "print('old')");
    env.write("workflows/old/workflow.toml", WORKFLOW_PLAIN);
    env
}

#[test]
fn merge_adds_and_overwrites_but_keeps_what_the_backup_does_not_have() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = lived_in();
    restore(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Merge,
        &options(1),
    )
    .unwrap();
    let config = target.config();

    // Settings: keys of the backup win; keys it lacks stay.
    assert_eq!(config.general.hotkey, "Alt+Space");
    assert!(
        config.general.launch_at_login,
        "a key the backup does not set stays"
    );
    assert_eq!(config.search.max_results, 12);
    assert_eq!(
        config.onepassword.account, "mine",
        "left out sections are never touched"
    );
    assert_eq!(config.hotkeys.len(), 1);

    // Snippets: by name.
    let names: Vec<&str> = config.snippet.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Local", "Sig", "Addr"]);
    assert_eq!(config.snippet[1].text, "Regards");
    assert!(config.snippets.auto_expand);

    // Web searches: by keyword.
    let engines: Vec<(&str, &str)> = config
        .web_search
        .iter()
        .map(|e| (e.keyword.as_str(), e.name.as_str()))
        .collect();
    assert_eq!(
        engines,
        [("mine", "Mine"), ("g", "Google"), ("ddg", "DuckDuckGo")]
    );

    // Files: the backup's overwrite, the others stay.
    assert_eq!(
        fs::read_to_string(target.path("themes/Nord.toml")).unwrap(),
        NORD
    );
    assert!(target.path("themes/Local.toml").exists());
    assert!(target.path("plugins/local/plugin.toml").exists());
    assert_eq!(
        fs::read_to_string(target.path("plugins/hello/main.py")).unwrap(),
        "print('hello')"
    );
    assert!(target.path("workflows/old/workflow.toml").exists());
    assert!(target.path("workflows/runner/workflow.toml").exists());
    // The user's comment survived.
    assert!(target.config_text().contains("# my comment"));
}

#[test]
fn replace_makes_the_chosen_categories_exactly_like_the_backup() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = lived_in();
    restore(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Replace,
        &options(1),
    )
    .unwrap();
    let config = target.config();

    assert_eq!(config.general.hotkey, "Alt+Space");
    assert!(
        !config.general.launch_at_login,
        "a key the backup lacks goes back to its default"
    );
    assert_eq!(
        config.onepassword.account, "mine",
        "left out sections are never touched"
    );
    let names: Vec<&str> = config.snippet.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Sig", "Addr"]);
    let keywords: Vec<&str> = config
        .web_search
        .iter()
        .map(|e| e.keyword.as_str())
        .collect();
    assert_eq!(keywords, ["g", "ddg"]);

    assert!(!target.path("themes/Local.toml").exists());
    assert!(!target.path("plugins/local").exists());
    assert!(!target.path("workflows/old").exists());
    assert!(target.path("plugins/hello/lib/util.py").exists());
}

#[test]
fn only_selected_categories_are_touched() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = lived_in();
    let before_settings = target.config().general.clone();
    restore(
        &target.roots,
        &backup,
        &[Category::Themes],
        Mode::Replace,
        &options(1),
    )
    .unwrap();
    assert_eq!(target.config().general, before_settings);
    assert!(target.path("plugins/local/plugin.toml").exists());
    assert!(!target.path("themes/Local.toml").exists());
    assert_eq!(target.config().snippet.len(), 2);
}

#[test]
fn an_empty_category_in_a_backup_clears_it_on_replace_only() {
    let empty = Env::new();
    let backup = read(&backup_bytes(
        &empty,
        &[Category::Plugins, Category::Snippets],
    ))
    .unwrap();
    assert!(backup.has(Category::Plugins));

    let merged = lived_in();
    restore(
        &merged.roots,
        &backup,
        &[Category::Plugins, Category::Snippets],
        Mode::Merge,
        &options(1),
    )
    .unwrap();
    assert!(merged.path("plugins/local/plugin.toml").exists());
    assert_eq!(merged.config().snippet.len(), 2);

    let replaced = lived_in();
    restore(
        &replaced.roots,
        &backup,
        &[Category::Plugins, Category::Snippets],
        Mode::Replace,
        &options(1),
    )
    .unwrap();
    assert!(!replaced.path("plugins/local").exists());
    assert!(replaced.config().snippet.is_empty());
}

#[test]
fn the_preview_counts_what_would_change() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = lived_in();
    let before = target.tree();

    let merge = preview(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Merge,
        &accept,
    )
    .unwrap();
    let find = |category: Category| {
        merge
            .categories
            .iter()
            .find(|c| c.category == category)
            .unwrap()
            .clone()
    };
    let snippets = find(Category::Snippets);
    // Sig differs, Addr is new, Local is not mentioned (merge keeps it), and the
    // expansion options are new.
    assert_eq!(snippets.added, 2);
    assert_eq!(snippets.changed, 1);
    assert_eq!(snippets.removed, 0);
    let engines = find(Category::WebSearch);
    assert_eq!((engines.added, engines.changed, engines.removed), (1, 1, 0));
    let themes = find(Category::Themes);
    assert_eq!(
        (themes.added, themes.changed),
        (1, 1),
        "the stylesheet is new, Nord differs"
    );
    let plugins = find(Category::Plugins);
    assert_eq!((plugins.added, plugins.changed), (1, 1));
    assert!(plugins
        .items
        .iter()
        .any(|i| i.name == "hello" && i.change == Change::Changed && i.needs_approval));
    assert!(merge.needs_approval.iter().any(|n| n.contains("hello")));
    assert!(merge.categories.iter().all(|c| c.in_backup && c.selected));

    let replace = preview(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Replace,
        &accept,
    )
    .unwrap();
    let snippets = replace
        .categories
        .iter()
        .find(|c| c.category == Category::Snippets)
        .unwrap();
    assert_eq!(snippets.removed, 1, "Local would go");
    let plugins = replace
        .categories
        .iter()
        .find(|c| c.category == Category::Plugins)
        .unwrap();
    assert_eq!(plugins.removed, 1);
    assert!(!replace.nothing_to_change);

    // A preview writes nothing.
    assert_eq!(target.tree(), before);
    assert!(latest_snapshot(&target.roots).is_none());
}

#[test]
fn restoring_the_same_backup_twice_changes_nothing_the_second_time() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = lived_in();
    restore(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Merge,
        &options(1),
    )
    .unwrap();
    let after_first = target.tree();
    let report = restore(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Merge,
        &options(2),
    )
    .unwrap();
    assert!(!report.changed);
    assert!(report.snapshot.is_none());
    assert_eq!(target.tree(), after_first);
    let again = preview(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Merge,
        &accept,
    )
    .unwrap();
    assert!(again.nothing_to_change);
}

// ---------------------------------------------------------------------------
// Failure leaves things untouched
// ---------------------------------------------------------------------------

#[test]
fn a_config_with_a_mistake_blocks_only_the_categories_that_need_it() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = Env::new();
    target.write("config.toml", "[general\nhotkey = ");
    let before = target.tree();

    let err = restore(
        &target.roots,
        &backup,
        &[Category::Settings],
        Mode::Replace,
        &options(1),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Current(_)), "{err}");
    assert!(err.to_string().contains("config.toml"));
    assert_eq!(target.tree(), before);

    // Themes do not need the config file.
    restore(
        &target.roots,
        &backup,
        &[Category::Themes],
        Mode::Merge,
        &options(1),
    )
    .unwrap();
    assert!(target.path("themes/Nord.toml").exists());
    assert_eq!(target.config_text(), "[general\nhotkey = ");
}

#[test]
fn a_rejected_result_changes_nothing_and_takes_no_snapshot() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = lived_in();
    let before = target.tree();
    let refuse = |_: &Config| Err("the shortcut is taken".to_owned());
    let err = restore(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Replace,
        &RestoreOptions {
            validate: &refuse,
            stamp: stamp(),
            expect_sha256: None,
        },
    )
    .unwrap_err();
    assert!(
        matches!(&err, Error::Rejected(m) if m.contains("taken")),
        "{err}"
    );
    assert_eq!(target.tree(), before);
    assert!(latest_snapshot(&target.roots).is_none());
}

#[test]
fn a_file_that_changed_after_the_preview_is_not_applied() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = Env::new();
    let err = restore(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Merge,
        &RestoreOptions {
            validate: &accept,
            stamp: stamp(),
            expect_sha256: Some("0000"),
        },
    )
    .unwrap_err();
    assert!(matches!(err, Error::Changed));
    assert!(target.tree().is_empty());
}

#[test]
fn a_failure_in_the_middle_puts_everything_back() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();

    // Fail after every possible number of steps: each time the target must be
    // exactly as it was, and once the failure point is past the last step the
    // restore succeeds.
    let mut failures = 0;
    let mut succeeded = false;
    for fail_after in 1..60 {
        let target = lived_in();
        let before = target.tree();
        let result = restore_with(
            &target.roots,
            &backup,
            &all_categories(),
            Mode::Replace,
            &options(1),
            Hooks {
                fail_after: Some(fail_after),
            },
            true,
        );
        match result {
            Err(Error::Apply { rolled_back, .. }) => {
                assert!(rolled_back);
                failures += 1;
                assert_eq!(target.tree(), before, "failure at step {fail_after}");
                assert!(
                    !target
                        .roots
                        .config_dir
                        .read_dir()
                        .unwrap()
                        .flatten()
                        .any(|e| e
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".sevak-restore")),
                    "the staging folder is cleaned up"
                );
            }
            Ok(_) => {
                succeeded = true;
                break;
            }
            Err(other) => panic!("unexpected error at step {fail_after}: {other}"),
        }
    }
    assert!(
        failures >= 8,
        "the restore has many steps; only {failures} failed"
    );
    assert!(
        succeeded,
        "the injected failure point never ran past the last step"
    );
}

#[test]
fn a_real_file_system_failure_rolls_back_what_was_already_done() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = lived_in();
    // `workflows` is a file, so the workflow folders cannot be created. The
    // plugins and themes that come before it have already been swapped in.
    fs::remove_dir_all(target.path("workflows")).unwrap();
    fs::write(target.path("workflows"), "in the way").unwrap();
    let before = target.tree();
    let err = restore(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Merge,
        &options(1),
    )
    .unwrap_err();
    assert!(
        matches!(
            &err,
            Error::Apply {
                rolled_back: true,
                ..
            }
        ),
        "{err}"
    );
    assert_eq!(target.tree(), before);
}

// ---------------------------------------------------------------------------
// Safety snapshot and undo
// ---------------------------------------------------------------------------

#[test]
fn undo_puts_back_what_the_restore_replaced() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = lived_in();
    let config_before = target.config();
    let tree_before: BTreeMap<String, Vec<u8>> = target
        .tree()
        .into_iter()
        .filter(|(path, _)| path != "config.toml")
        .collect();

    let report = restore(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Replace,
        &options(1),
    )
    .unwrap();
    let snapshot = report.snapshot.expect("a safety copy is taken");
    assert!(snapshot.exists());
    let info = latest_snapshot(&target.roots).unwrap();
    assert_eq!(info.path, snapshot);
    assert_eq!(info.categories, all_categories());
    assert_ne!(target.config(), config_before);

    undo_restore(&target.roots, stamp_at(2)).unwrap();
    assert_eq!(target.config().general, config_before.general);
    assert_eq!(target.config().search, config_before.search);
    assert_eq!(target.config().snippet, config_before.snippet);
    assert_eq!(target.config().web_search, config_before.web_search);
    assert_eq!(target.config().onepassword, config_before.onepassword);
    let tree_after: BTreeMap<String, Vec<u8>> = target
        .tree()
        .into_iter()
        .filter(|(path, _)| path != "config.toml")
        .collect();
    assert_eq!(tree_after, tree_before);
    assert!(!snapshot.exists(), "an undone snapshot is spent");
    assert!(matches!(
        undo_restore(&target.roots, stamp_at(3)),
        Err(Error::Nothing(_))
    ));
}

#[test]
fn undo_of_a_merge_removes_what_was_added() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(
        &source,
        &[Category::Plugins, Category::Workflows],
    ))
    .unwrap();
    let target = lived_in();
    restore(
        &target.roots,
        &backup,
        &[Category::Plugins, Category::Workflows],
        Mode::Merge,
        &options(1),
    )
    .unwrap();
    assert!(target.path("plugins/other/plugin.toml").exists());
    undo_restore(&target.roots, stamp_at(2)).unwrap();
    assert!(!target.path("plugins/other").exists());
    assert!(!target.path("workflows/runner").exists());
    assert_eq!(
        fs::read_to_string(target.path("plugins/hello/main.py")).unwrap(),
        "print('old')"
    );
}

#[test]
fn undo_that_fails_leaves_the_restored_state_and_the_snapshot() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = lived_in();
    restore(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Replace,
        &options(1),
    )
    .unwrap();
    let restored = target.tree();
    let err = undo_with(
        &target.roots,
        stamp_at(2),
        Hooks {
            fail_after: Some(2),
        },
    )
    .unwrap_err();
    assert!(matches!(err, Error::Apply { .. }));
    assert_eq!(target.tree(), restored);
    assert!(
        latest_snapshot(&target.roots).is_some(),
        "the snapshot is kept"
    );
}

#[test]
fn a_snapshot_that_cannot_be_written_stops_the_restore() {
    let source = kitchen_sink();
    let backup = read(&backup_bytes(&source, &all_categories())).unwrap();
    let target = lived_in();
    // The snapshots folder is a file, so no safety copy can be made.
    fs::write(target.roots.snapshots_dir(), "in the way").unwrap();
    let before = target.tree();
    let err = restore(
        &target.roots,
        &backup,
        &all_categories(),
        Mode::Replace,
        &options(1),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Snapshot(_)), "{err}");
    assert_eq!(target.tree(), before);
}

#[test]
fn only_the_newest_snapshots_are_kept() {
    let source = kitchen_sink();
    let target = Env::new();
    for i in 0..8u32 {
        // Each restore differs from the last, so each one takes a snapshot.
        source.write("themes/Nord.toml", &format!("name = \"Nord {i}\"\n"));
        let backup = read(&backup_bytes(&source, &[Category::Themes])).unwrap();
        restore(
            &target.roots,
            &backup,
            &[Category::Themes],
            Mode::Replace,
            &options(10 + i),
        )
        .unwrap();
    }
    let kept = fs::read_dir(target.roots.snapshots_dir()).unwrap().count();
    assert_eq!(kept, crate::limits::KEEP_SNAPSHOTS);
    // And the newest one is the one undo uses.
    let newest = latest_snapshot(&target.roots).unwrap();
    assert!(
        newest.path.to_string_lossy().contains("153017"),
        "{:?}",
        newest.path
    );
}

#[test]
fn snapshots_are_ordinary_valid_backups_without_secrets() {
    let source = kitchen_sink();
    let target = lived_in();
    target.write("secrets.toml", "token = \"DECOY\"");
    let path = create_snapshot(&target.roots, &all_categories(), &stamp()).unwrap();
    let backup = read(&fs::read(path).unwrap()).unwrap();
    assert_eq!(backup.manifest.kind, Kind::Snapshot);
    assert!(backup
        .items
        .iter()
        .all(|i| !String::from_utf8_lossy(&i.data).contains("DECOY")));
    drop(source);
}

// ---------------------------------------------------------------------------
// Approvals: restored code is never trusted
// ---------------------------------------------------------------------------

fn script_host(roots: &Roots) -> ScriptPluginHost {
    ScriptPluginHost::new(
        roots.plugins_dir(),
        roots.data_dir.join("plugins"),
        roots.approvals_file(),
    )
}

fn workflow_host(roots: &Roots) -> WorkflowHost {
    WorkflowHost::new(
        roots.workflows_dir(),
        roots.data_dir.join("workflows"),
        roots.approvals_file(),
        Arc::new(NoSink),
    )
}

/// Whether the script plugin in `folder` is approved to run.
fn plugin_approved(roots: &Roots, folder: &str) -> bool {
    script_host(roots)
        .scan()
        .into_iter()
        .find_map(|scanned| match scanned {
            ScriptScanned::Plugin(candidate) if candidate.folder == folder => {
                Some(candidate.approved)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("plugin {folder} not found"))
}

/// Whether the workflow in `folder` may run (a workflow that runs no code always may).
fn workflow_approved(roots: &Roots, folder: &str) -> bool {
    workflow_host(roots)
        .scan()
        .into_iter()
        .find_map(|scanned| match scanned {
            WorkflowScanned::Workflow(candidate) if candidate.folder == folder => {
                Some(candidate.approved)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("workflow {folder} not found"))
}

fn approve_everything(roots: &Roots) {
    let scripts = script_host(roots);
    for scanned in scripts.scan() {
        if let ScriptScanned::Plugin(candidate) = scanned {
            scripts.approve(&candidate).unwrap();
        }
    }
    let workflows = workflow_host(roots);
    for scanned in workflows.scan() {
        if let WorkflowScanned::Workflow(candidate) = scanned {
            workflows.approve(&candidate).unwrap();
        }
    }
}

#[test]
fn restored_scripts_and_workflows_ask_for_approval_again_on_a_fresh_install() {
    let source = kitchen_sink();
    approve_everything(&source.roots);
    assert!(plugin_approved(&source.roots, "hello"));
    assert!(workflow_approved(&source.roots, "runner"));
    assert!(source.roots.approvals_file().exists());

    // The approval record is not in the backup.
    let bytes = backup_bytes(&source, &all_categories());
    assert!(!String::from_utf8_lossy(&bytes).contains("script-plugin-approvals"));
    let backup = read(&bytes).unwrap();
    assert!(backup.items.iter().all(|i| !i.path.contains("approval")));

    let fresh = Env::new();
    restore(
        &fresh.roots,
        &backup,
        &all_categories(),
        Mode::Replace,
        &options(1),
    )
    .unwrap();
    assert!(
        !fresh.roots.approvals_file().exists(),
        "nothing was approved by the restore"
    );
    assert!(!plugin_approved(&fresh.roots, "hello"));
    assert!(!plugin_approved(&fresh.roots, "other"));
    assert!(!workflow_approved(&fresh.roots, "runner"));
    // A workflow that runs no code has nothing to approve.
    assert!(workflow_approved(&fresh.roots, "plain"));

    // And the preview said so (on another fresh install, before restoring).
    let another = Env::new();
    let shown = preview(
        &another.roots,
        &backup,
        &all_categories(),
        Mode::Replace,
        &accept,
    )
    .unwrap();
    assert!(shown.needs_approval.iter().any(|n| n.contains("hello")));
    assert!(shown.needs_approval.iter().any(|n| n.contains("runner")));
    assert!(!shown.needs_approval.iter().any(|n| n.contains("plain")));
}

#[test]
fn a_restore_does_not_inherit_an_old_approval_for_the_same_name_and_command() {
    // The approval of a script plugin is bound to its command, not to its
    // files; a restore that swaps the script must therefore reset it itself.
    let env = kitchen_sink();
    let old_backup = read(&backup_bytes(
        &env,
        &[Category::Plugins, Category::Workflows],
    ))
    .unwrap();

    // The user later edits the scripts (same commands) and approves everything.
    env.write("plugins/hello/main.py", "print('edited')");
    env.write("workflows/runner/main.py", "print('edited')");
    approve_everything(&env.roots);
    assert!(plugin_approved(&env.roots, "hello"));
    assert!(plugin_approved(&env.roots, "other"));
    assert!(workflow_approved(&env.roots, "runner"));

    restore(
        &env.roots,
        &old_backup,
        &[Category::Plugins, Category::Workflows],
        Mode::Merge,
        &options(1),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(env.path("plugins/hello/main.py")).unwrap(),
        "print('hello')"
    );
    assert!(
        !plugin_approved(&env.roots, "hello"),
        "the swapped plugin must ask again"
    );
    assert!(
        !workflow_approved(&env.roots, "runner"),
        "the swapped workflow must ask again"
    );
    // What the restore left alone keeps its approval: no needless questions.
    assert!(plugin_approved(&env.roots, "other"));
}

#[test]
fn an_old_approval_does_not_revive_a_plugin_that_was_deleted_and_comes_back() {
    let env = kitchen_sink();
    approve_everything(&env.roots);
    let backup = read(&backup_bytes(&env, &[Category::Plugins])).unwrap();
    fs::remove_dir_all(env.path("plugins/hello")).unwrap();
    // The record still says "hello" may run `python3 main.py`.
    restore(
        &env.roots,
        &backup,
        &[Category::Plugins],
        Mode::Merge,
        &options(1),
    )
    .unwrap();
    assert!(env.path("plugins/hello/plugin.toml").exists());
    assert!(!plugin_approved(&env.roots, "hello"));
}

// ---------------------------------------------------------------------------
// Automatic backups
// ---------------------------------------------------------------------------

#[test]
fn the_schedule_file_is_forgiving_and_off_by_default() {
    let env = Env::new();
    let (config, problem) = AutoConfig::load(&env.roots);
    assert_eq!(config, AutoConfig::default());
    assert!(problem.is_none());
    assert!(!config.enabled());

    env.write("backup.toml", "schedule = \"hourly\"\nkeep = 999\n");
    let (config, _) = AutoConfig::load(&env.roots);
    assert_eq!(config.schedule, Schedule::Off, "an unknown value means off");
    assert_eq!(config.keep, auto::MAX_KEEP);

    env.write("backup.toml", "schedule = [");
    let (config, problem) = AutoConfig::load(&env.roots);
    assert_eq!(config.schedule, Schedule::Off);
    assert!(problem.unwrap().contains("backup.toml"));

    let wanted = AutoConfig {
        schedule: Schedule::Weekly,
        on_update: true,
        keep: 3,
        folder: "~/my backups".to_owned(),
    };
    wanted.save(&env.roots).unwrap();
    assert_eq!(AutoConfig::load(&env.roots).0, wanted);
    assert!(fs::read_to_string(env.path("backup.toml"))
        .unwrap()
        .contains("# Where they go"));
}

#[test]
fn folders_are_resolved_safely() {
    assert_eq!(auto::resolve_folder("").unwrap(), auto::default_folder());
    assert!(auto::default_folder().ends_with("Sevak backups"));
    assert!(auto::resolve_folder("relative/path").is_err());
    let absolute = std::env::temp_dir().join("x");
    assert_eq!(
        auto::resolve_folder(absolute.to_str().unwrap()).unwrap(),
        absolute
    );
    if dirs::home_dir().is_some() {
        assert!(auto::resolve_folder("~/b").unwrap().is_absolute());
    }
}

#[test]
fn a_backup_is_due_by_the_clock_or_after_an_update() {
    let daily = AutoConfig {
        schedule: Schedule::Daily,
        ..AutoConfig::default()
    };
    let weekly = AutoConfig {
        schedule: Schedule::Weekly,
        ..AutoConfig::default()
    };
    let none = State::default();
    let at = |unix| State {
        last_auto_unix: Some(unix),
        ..State::default()
    };
    assert_eq!(
        auto::due(&AutoConfig::default(), &none, 10_000_000, "1"),
        None
    );
    assert_eq!(auto::due(&daily, &none, 100, "1"), Some(Reason::Schedule));
    assert_eq!(auto::due(&daily, &at(1_000), 1_000 + 86_399, "1"), None);
    assert_eq!(
        auto::due(&daily, &at(1_000), 1_000 + 86_400, "1"),
        Some(Reason::Schedule)
    );
    assert_eq!(
        auto::due(&weekly, &at(1_000), 1_000 + 86_400 * 6, "1"),
        None
    );
    assert_eq!(
        auto::due(&weekly, &at(1_000), 1_000 + 86_400 * 7, "1"),
        Some(Reason::Schedule)
    );

    let on_update = AutoConfig {
        on_update: true,
        ..AutoConfig::default()
    };
    let seen = |v: &str| State {
        last_seen_version: Some(v.to_owned()),
        ..State::default()
    };
    assert_eq!(
        auto::due(&on_update, &none, 1, "2"),
        None,
        "first run: nothing to compare"
    );
    assert_eq!(auto::due(&on_update, &seen("2"), 1, "2"), None);
    assert_eq!(
        auto::due(&on_update, &seen("1"), 1, "2"),
        Some(Reason::Update)
    );
    // Off means off, whatever the version.
    assert_eq!(auto::due(&AutoConfig::default(), &seen("1"), 1, "2"), None);
}

#[test]
fn automatic_backups_are_made_when_due_and_only_the_newest_are_kept() {
    let source = kitchen_sink();
    let out = tempfile::tempdir().unwrap();
    let folder = out.path().join("auto");
    AutoConfig {
        schedule: Schedule::Daily,
        keep: 2,
        folder: folder.to_str().unwrap().to_owned(),
        ..AutoConfig::default()
    }
    .save(&source.roots)
    .unwrap();
    // A manual backup and an unrelated file in the same folder must survive pruning.
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("sevak-backup-20200101-000000.sevakbackup"),
        "manual",
    )
    .unwrap();
    fs::write(folder.join("notes.txt"), "mine").unwrap();

    let day = |d: u32, s: u32| {
        Stamp::at(
            FixedOffset::east_opt(0)
                .unwrap()
                .with_ymd_and_hms(2026, 10, d, 9, 0, s)
                .unwrap(),
        )
    };
    let first = auto::run_due(&source.roots, &day(1, 0), "1.0.0")
        .unwrap()
        .unwrap();
    assert_eq!(first.kind, Kind::Auto);
    assert!(first.path.starts_with(&folder));
    assert!(first
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("sevak-auto-20261001"));
    assert!(read(&fs::read(&first.path).unwrap()).is_ok());
    // Same day: not due again.
    assert!(auto::run_due(&source.roots, &day(1, 30), "1.0.0")
        .unwrap()
        .is_none());
    for d in 2..=4 {
        assert!(auto::run_due(&source.roots, &day(d, 0), "1.0.0")
            .unwrap()
            .is_some());
    }
    let mut names: Vec<String> = fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "notes.txt",
            "sevak-auto-20261003-090000.sevakbackup",
            "sevak-auto-20261004-090000.sevakbackup",
            "sevak-backup-20200101-000000.sevakbackup",
        ]
    );
    assert_eq!(State::load(&source.roots).last.unwrap().kind, Kind::Auto);
}

#[test]
fn a_backup_before_an_update_runs_once() {
    let source = kitchen_sink();
    let out = tempfile::tempdir().unwrap();
    AutoConfig {
        on_update: true,
        folder: out.path().to_str().unwrap().to_owned(),
        ..AutoConfig::default()
    }
    .save(&source.roots)
    .unwrap();
    // The first look records the version and backs up nothing.
    assert!(auto::run_due(&source.roots, &stamp_at(0), "1.0.0")
        .unwrap()
        .is_none());
    assert!(auto::run_due(&source.roots, &stamp_at(1), "1.0.0")
        .unwrap()
        .is_none());
    // A new version triggers one backup.
    assert!(auto::run_due(&source.roots, &stamp_at(2), "1.1.0")
        .unwrap()
        .is_some());
    assert!(auto::run_due(&source.roots, &stamp_at(3), "1.1.0")
        .unwrap()
        .is_none());
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 1);
}

#[test]
fn nothing_is_backed_up_automatically_unless_switched_on() {
    let source = kitchen_sink();
    assert!(auto::run_due(&source.roots, &stamp(), "1.0.0")
        .unwrap()
        .is_none());
    assert!(!auto::default_folder().join("never").exists());
}

#[test]
fn items_are_what_the_manifest_says() {
    // A tiny sanity check of the building blocks used by the hostile-archive helpers.
    let item = Item {
        path: "themes/a.toml".to_owned(),
        category: Category::Themes,
        data: b"name = \"a\"".to_vec(),
        executable: false,
    };
    let bytes = build(
        std::slice::from_ref(&item),
        &Meta {
            kind: Kind::Manual,
            categories: &[Category::Themes],
            restore_of: &[],
            stamp: &stamp(),
        },
    )
    .unwrap();
    let backup = read(&bytes).unwrap();
    assert_eq!(backup.items, vec![item]);
    assert_eq!(backup.manifest.created, "2026-10-04T15:30:45+00:00");
}
