//! The content of `gallery/` and `examples/`, checked end to end and offline.
//!
//! * the index matches the committed packages (hash, address, tags, folder),
//! * every gallery workflow validates, stays inside the gallery's safety policy
//!   (no app launches, no system or terminal commands, links only to known
//!   hosts, scripts that only use the standard library) and does what its
//!   description says when it runs,
//! * every gallery script plugin has a loadable manifest, a keyword nothing else
//!   uses and a script that answers in the right shape.
//!
//! Nothing here touches the network, the clipboard or the desktop: the workflows
//! run against a recording platform. The Python scripts run for real; a
//! computer without Python 3 skips those checks (set `SEVAK_REQUIRE_PYTHON=1`
//! to fail instead, as CI could).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use sevak_core::gallery_source::Pin;
use sevak_core::{AppEntry, Config, IconData, IconSource, LaunchTarget};
use sevak_platform::{PasteOutcome, PlatformError, PlatformProvider, Result as PlatformResult};
use sevak_plugins::keywords::KeywordOwners;
use sevak_plugins::net::sha256_hex;
use sevak_plugins::script::{Format, Launch, Manifest, Mode};
use sevak_plugins::workflow::gallery::{parse_index, Entry, Kind, MAX_PACKAGE_BYTES};
use sevak_plugins::workflow::{Ctx, NodeKind, OutputSink, RunReport, Runtime, Workflow};

/// Where the pinned release's files live. The index itself names files by a
/// path relative to the repository root; the parser joins them to this tag.
const RAW: &str = "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/";
const TREE: &str = "https://github.com/ninad-k/Sevak/tree/main/examples/";

/// Hosts a gallery workflow may open. Adding a host is a review decision.
const ALLOWED_HOSTS: &[&str] = &[
    "duckduckgo.com",
    "stackoverflow.com",
    "developer.mozilla.org",
    "crates.io",
    "docs.rs",
    "www.npmjs.com",
    "pypi.org",
    "github.com",
    "en.wikipedia.org",
    "www.openstreetmap.org",
    "www.google.com",
];

/// Python modules the gallery's scripts may import: text, math, hashing,
/// randomness and time only. Nothing that reaches the network or other programs.
/// `base64` (decode a JWT's segments) and `unicodedata` (character names for
/// the Unicode lookup, accent folding for slugs) are pure in-memory data
/// transforms with no I/O, added for the second plugin pack.
const ALLOWED_PYTHON_IMPORTS: &[&str] = &[
    "base64",
    "colorsys",
    "datetime",
    "hashlib",
    "json",
    "math",
    "os",
    "random",
    "re",
    "secrets",
    "string",
    "sys",
    "time",
    "unicodedata",
    "uuid",
    "zlib",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn examples(kind: Kind) -> PathBuf {
    repo().join("examples").join(match kind {
        Kind::Workflow => "workflows",
        Kind::Plugin => "plugins",
        Kind::Native => "extensions",
    })
}

fn pin() -> Pin {
    Pin::new("v1.2.3").unwrap()
}

fn all_entries() -> Vec<Entry> {
    let text = std::fs::read_to_string(repo().join("gallery/index.json")).unwrap();
    let parsed = parse_index(&text, &pin()).unwrap();
    assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
    parsed.entries
}

/// The workflows and script plugins (one zip each). Native extensions have one
/// package per platform and are checked by `native_extensions_are_real_packages`.
fn index() -> Vec<Entry> {
    all_entries()
        .into_iter()
        .filter(|e| e.kind != Kind::Native)
        .collect()
}

fn entries_of(kind: Kind) -> Vec<Entry> {
    index().into_iter().filter(|e| e.kind == kind).collect()
}

// ---- the index and the packages ---------------------------------------------

/// A small gallery with one native extension, built here: the packages, the
/// index entry and the folder layout the real gallery uses.
fn small_native_gallery(
    tamper: impl FnOnce(&mut serde_json::Value, &Path),
) -> (tempfile::TempDir, Vec<Entry>) {
    use sevak_plugins::extensions::Builder;

    let tmp = tempfile::tempdir().unwrap();
    let manifest = "protocol = 1\nid = \"script:tool\"\nkeyword = \"tool\"\nname = \"Tool\"\n\
        description = \"A tool.\"\n[extension]\nversion = \"1.2.3\"\nauthor = \"Ada\"\n\
        license = \"MIT\"\nmin_sevak = \"0.1.0\"\npermissions = [\"network\"]\n\
        repository = \"https://github.com/example/tool\"\n[extension.binaries]\n\
        linux-x86_64 = \"bin/tool-linux\"\nmacos-aarch64 = \"bin/tool-macos\"\n";
    let mut builder = Builder::new(manifest, "tool").unwrap();
    builder.add_binary("linux-x86_64", b"elf".to_vec()).unwrap();
    builder
        .add_binary("macos-aarch64", b"macho".to_vec())
        .unwrap();
    let dir = tmp.path().join("extensions/tool");
    std::fs::create_dir_all(&dir).unwrap();
    let mut platforms = serde_json::Map::new();
    for platform in ["linux-x86_64", "macos-aarch64"] {
        let bytes = builder.build(Some(platform)).unwrap();
        let file = format!("tool-1.2.3-{platform}.sevakext");
        std::fs::write(dir.join(&file), &bytes).unwrap();
        platforms.insert(
            platform.to_owned(),
            serde_json::json!({
                "source": format!("gallery/extensions/tool/{file}"),
                "sha256": sha256_hex(&bytes)
            }),
        );
    }
    let mut entry = serde_json::json!({
        "id": "tool", "kind": "native", "name": "Tool", "description": "A tool.",
        "author": "Ada", "version": "1.2.3", "license": "MIT", "min_sevak": "0.1.0",
        "permissions": ["network"], "repository": "https://github.com/example/tool",
        "platforms": platforms
    });
    tamper(&mut entry, &dir);
    let index = serde_json::json!({"format": 2, "entries": [entry]}).to_string();
    let parsed = parse_index(&index, &pin()).unwrap();
    assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
    (tmp, parsed.entries)
}

#[test]
fn the_native_package_checks_pass_for_a_consistent_gallery() {
    let (tmp, entries) = small_native_gallery(|_, _| {});
    check_native_entries(&entries, tmp.path());
}

/// What a test does to the entry (and the files) before the checks run.
type Tamper = Box<dyn FnOnce(&mut serde_json::Value, &Path)>;

#[test]
fn the_native_package_checks_catch_each_kind_of_mismatch() {
    let cases: Vec<(&str, Tamper)> = vec![
        (
            "hash",
            Box::new(|entry, _| {
                entry["platforms"]["linux-x86_64"]["sha256"] = serde_json::json!("0".repeat(64));
            }),
        ),
        (
            "version",
            Box::new(|entry, _| entry["version"] = "1.2.4".into()),
        ),
        (
            "author",
            Box::new(|entry, _| entry["author"] = "Mallory".into()),
        ),
        (
            "permissions",
            Box::new(|entry, _| {
                entry["permissions"] = serde_json::json!(["network", "filesystem"])
            }),
        ),
        (
            "unlisted package",
            Box::new(|_, dir| {
                std::fs::write(dir.join("tool-0.0.1-linux-x86_64.sevakext"), b"x").unwrap()
            }),
        ),
    ];
    for (what, tamper) in cases {
        // A version change renames the files the entry expects, so build the
        // gallery first and let the tamper step run on the entry only.
        let (tmp, entries) = small_native_gallery(tamper);
        let result = std::panic::catch_unwind(|| check_native_entries(&entries, tmp.path()));
        assert!(result.is_err(), "{what} should be reported");
    }
}

/// Native extensions (`kind: "native"`): every platform's package is committed,
/// matches its hash, is a package this Sevak accepts, carries a program for that
/// platform and says in its own manifest what the index entry says (the page
/// shows the entry, the Allow dialog shows the manifest). Passes when the index
/// has none yet.
#[test]
fn native_extensions_are_real_packages() {
    check_native_entries(&all_entries(), &repo().join("gallery"));
}

/// The checks behind [`native_extensions_are_real_packages`], over `gallery`
/// (a folder laid out like the repository's `gallery/`), so they can be tried
/// on a small gallery built in a test.
fn check_native_entries(entries: &[Entry], gallery: &Path) {
    use sevak_plugins::extensions::ExtensionPackage;

    const MAX_NATIVE_PACKAGE_BYTES: usize = 10 * 1024 * 1024;
    let mut listed: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for entry in entries.iter().filter(|e| e.kind == Kind::Native) {
        let id = &entry.id;
        assert_eq!(entry.folder_name(), id, "{id}: folder");
        assert!(!entry.platforms.is_empty(), "{id}: no platforms");
        for (platform, artifact) in &entry.platforms {
            let file = artifact.source.rsplit('/').next().unwrap().to_owned();
            assert_eq!(
                artifact.source,
                format!("{RAW}gallery/extensions/{id}/{file}"),
                "{id} {platform}: source"
            );
            assert_eq!(
                file,
                format!("{id}-{}-{platform}.sevakext", entry.version),
                "{id} {platform}: file name is <id>-<version>-<platform>.sevakext"
            );
            listed.entry(id.clone()).or_default().insert(file.clone());
            let path = gallery.join("extensions").join(id).join(&file);
            let bytes = std::fs::read(&path).unwrap_or_else(|err| panic!("{id} {platform}: {err}"));
            assert!(
                bytes.len() <= MAX_NATIVE_PACKAGE_BYTES,
                "{id} {platform}: package too large"
            );
            assert_eq!(artifact.sha256, sha256_hex(&bytes), "{id} {platform}: hash");

            let package = ExtensionPackage::read(&bytes, id)
                .unwrap_or_else(|err| panic!("{id} {platform}: {err}"));
            assert_eq!(
                package.platforms(),
                std::slice::from_ref(platform),
                "{id} {platform}: a gallery package carries exactly its own platform's program"
            );
            let native = &package.native;
            assert_eq!(native.version, entry.version, "{id}: version");
            assert_eq!(native.author, entry.author, "{id}: author");
            assert_eq!(
                Some(&native.license),
                entry.license.as_ref(),
                "{id}: license"
            );
            assert_eq!(native.min_sevak, entry.min_sevak, "{id}: min_sevak");
            assert_eq!(native.repository, entry.repository, "{id}: repository");
            let (mut a, mut b) = (native.permissions.clone(), entry.permissions.clone());
            a.sort();
            b.sort();
            assert_eq!(a, b, "{id}: permissions");
            let manifest = Manifest::parse_for(&package.manifest_text, id, platform)
                .unwrap_or_else(|err| panic!("{id} {platform}: {err}"));
            assert_eq!(manifest.id, format!("script:{id}"), "{id}: manifest id");
            assert_eq!(manifest.name, entry.name, "{id}: name");
        }
    }
    // No package in the folder without an entry.
    let root = gallery.join("extensions");
    if root.is_dir() {
        for dir in std::fs::read_dir(&root).unwrap() {
            let dir = dir.unwrap();
            let id = dir.file_name().to_string_lossy().into_owned();
            let files = listed
                .get(&id)
                .unwrap_or_else(|| panic!("extensions/{id} has no native entry in index.json"));
            for file in std::fs::read_dir(dir.path()).unwrap() {
                let name = file.unwrap().file_name().to_string_lossy().into_owned();
                assert!(
                    files.contains(&name),
                    "extensions/{id}/{name} is not listed"
                );
            }
        }
    }
}

#[test]
fn the_index_describes_the_committed_packages() {
    let text = std::fs::read_to_string(repo().join("gallery/index.json")).unwrap();
    let raw: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(raw["format"], 2, "the committed index is format 2");
    let raw_entries: Vec<&Value> = raw["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] != "native")
        .collect();
    let entries = index();
    assert_eq!(entries.len(), raw_entries.len(), "an entry was skipped");
    assert!(entries.iter().filter(|e| e.kind == Kind::Workflow).count() >= 10);
    assert!(entries.iter().filter(|e| e.kind == Kind::Plugin).count() >= 6);

    let mut listed_zips = HashSet::new();
    for (entry, raw) in entries.iter().zip(raw_entries) {
        let id = &entry.id;
        // The package installs into the folder named like the entry, from the
        // committed zip, whose bytes are the ones the hash covers.
        assert_eq!(entry.folder_name(), id, "{id}: folder");
        // In the file the source is a path, never an address; after parsing it
        // is the address at the pinned release.
        assert_eq!(
            raw["source"],
            format!("gallery/packages/{id}.zip"),
            "{id}: source"
        );
        assert_eq!(
            entry.source,
            format!("{RAW}gallery/packages/{id}.zip"),
            "{id}"
        );
        let zip = std::fs::read(repo().join(format!("gallery/packages/{id}.zip")))
            .unwrap_or_else(|err| panic!("{id}: {err}"));
        assert!(zip.len() <= MAX_PACKAGE_BYTES, "{id}: package too large");
        assert_eq!(
            entry.sha256,
            sha256_hex(&zip),
            "{id}: re-run gallery_pack and `node scripts/gallery-check.mjs --update`"
        );
        listed_zips.insert(format!("{id}.zip"));

        let folder = examples(entry.kind).join(id);
        assert!(folder.is_dir(), "{id}: no folder {}", folder.display());
        let kind_dir = if entry.kind == Kind::Workflow {
            "workflows"
        } else {
            "plugins"
        };
        assert_eq!(
            entry.homepage.as_deref(),
            Some(format!("{TREE}{kind_dir}/{id}").as_str()),
            "{id}: homepage"
        );

        for (field, value) in [
            ("name", &entry.name),
            ("description", &entry.description),
            ("author", &entry.author),
            ("version", &entry.version),
        ] {
            assert!(!value.trim().is_empty(), "{id}: {field} is empty");
        }
        assert!(entry.name.chars().count() <= 60, "{id}: name too long");
        assert!(
            (30..=300).contains(&entry.description.chars().count()),
            "{id}: description should be one or two sentences"
        );
        // Tags: present, and every one kept by the parser (lower case, short).
        let tags = raw["tags"]
            .as_array()
            .unwrap_or_else(|| panic!("{id}: no tags"));
        assert!(!tags.is_empty(), "{id}: no tags");
        assert_eq!(
            tags.len(),
            entry.tags.len(),
            "{id}: an invalid tag was dropped"
        );
        assert!(entry.tags.len() <= 8);
    }
    // No package may sit in the folder without an entry.
    for file in std::fs::read_dir(repo().join("gallery/packages")).unwrap() {
        let name = file.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            listed_zips.contains(&name),
            "{name} is not listed in index.json"
        );
    }
}

/// What a user gets: every package installs through the gallery code into a
/// fresh folder, and the hosts then load the folders. Exactly the packages that
/// run a script or paste into another app wait for permission; nothing else does.
#[test]
fn installed_packages_load_through_the_hosts_and_only_scripts_need_permission() {
    use sevak_plugins::script::{Scanned as ScannedPlugin, ScriptPluginHost};
    use sevak_plugins::workflow::gallery::{install_bytes, Dirs};
    use sevak_plugins::workflow::{NoSink, Scanned, WorkflowHost};

    let root = tempfile::tempdir().unwrap();
    let workflows = root.path().join("workflows");
    let plugins = root.path().join("plugins");
    let dirs = Dirs {
        workflows: &workflows,
        plugins: &plugins,
    };
    let entries = index();
    for entry in &entries {
        let zip = std::fs::read(repo().join(format!("gallery/packages/{}.zip", entry.id))).unwrap();
        install_bytes(entry, &zip, &dirs).unwrap_or_else(|err| panic!("{}: {err}", entry.id));
    }

    let host = WorkflowHost::new(
        workflows,
        root.path().join("data/workflows"),
        root.path().join("approvals.json"),
        Arc::new(NoSink),
    );
    let scanned = host.scan();
    let installed = entries.iter().filter(|e| e.kind == Kind::Workflow).count();
    assert_eq!(scanned.len(), installed);
    for item in &scanned {
        match item {
            Scanned::Workflow(candidate) => {
                assert!(candidate.warnings.is_empty(), "{}", candidate.folder);
                // Approval is needed exactly where a node can run code, open
                // things or type into another app (`Paste`).
                let needs_approval = candidate.workflow.needs_approval();
                assert_eq!(
                    candidate.approval_key.is_some(),
                    needs_approval,
                    "{}",
                    candidate.folder
                );
                assert_eq!(candidate.approved, !needs_approval, "{}", candidate.folder);
            }
            Scanned::Broken { folder, error } => panic!("{folder}: {error}"),
        }
    }
    let mut waiting: Vec<String> = host
        .pending(&Config::default())
        .into_iter()
        .map(|c| c.folder)
        .collect();
    waiting.sort();
    // The workflows that paste into another app wait too, besides the script.
    assert_eq!(
        waiting,
        ["markdown-tools", "selection-toolkit", "tidy-text"]
    );

    let script_host = ScriptPluginHost::new(
        plugins,
        root.path().join("data/plugins"),
        root.path().join("plugin-approvals.json"),
    );
    let scanned = script_host.scan();
    assert_eq!(scanned.len(), entries.len() - installed);
    for item in &scanned {
        if let ScannedPlugin::Broken { folder, error } = item {
            panic!("{folder}: {error}");
        }
    }
    // Every script plugin waits for permission before anything runs.
    assert_eq!(script_host.pending(&Config::default()).len(), scanned.len());
}

// ---- workflows: policy -------------------------------------------------------

fn load_workflow(entry: &Entry) -> (Workflow, PathBuf) {
    let dir = examples(Kind::Workflow).join(&entry.id);
    let text = std::fs::read_to_string(dir.join("workflow.toml")).unwrap();
    let workflow = Workflow::from_toml(&text).unwrap_or_else(|err| panic!("{}: {err}", entry.id));
    (workflow, dir)
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    // The Wikipedia workflow takes the language from a variable.
    Some(host.replace("{var:lang}", "en"))
}

/// Why `script` is not an acceptable gallery script, if it is not.
fn script_problem(name: &str, text: &str, allowed_imports: &[String]) -> Option<String> {
    if text.len() > 20 * 1024 {
        return Some("longer than 20 KiB; the gallery keeps scripts small enough to read".into());
    }
    let denied: &[&str] = match name.rsplit('.').next() {
        Some("py") => &[
            "os.system",
            "os.popen",
            "os.spawn",
            "os.exec",
            "os.kill",
            "os.remove",
            "os.unlink",
            "os.rmdir",
            "os.rename",
            "exec(",
            "eval(",
            "__import__",
        ],
        Some("js" | "mjs") => &[
            "child_process",
            "require(",
            "import(",
            "eval(",
            "Function(",
            "fetch(",
            "XMLHttpRequest",
        ],
        Some("ps1") => &[
            "Invoke-WebRequest",
            "Invoke-RestMethod",
            "Invoke-Expression",
            "Start-Process",
            "DownloadString",
            "WebClient",
            "iex ",
            "iwr ",
            "irm ",
        ],
        other => return Some(format!("scripts of kind {other:?} are not accepted")),
    };
    if let Some(word) = denied.iter().find(|word| text.contains(**word)) {
        return Some(format!("uses {word:?}"));
    }
    if name.ends_with(".js") || name.ends_with(".mjs") {
        // This is a review guard, not a JavaScript sandbox. Only simple static
        // imports of approved local helpers or explicitly reviewed Node modules.
        let import = regex::Regex::new(r#"^import\s+.+\s+from\s+["']([^"']+)["'];?$"#).unwrap();
        for line in text.lines().map(str::trim) {
            if line.starts_with("import") {
                let module = import.captures(line).map(|m| m[1].to_owned());
                if !module.as_ref().is_some_and(|m| allowed_imports.contains(m)) {
                    return Some(format!("import is not in the allow list: {line}"));
                }
            }
        }
    }
    if name.ends_with(".py") {
        for line in text.lines() {
            let line = line.trim_start();
            // Files are only ever opened for reading, as bytes.
            if line.contains("open(") && !line.contains("\"rb\"") {
                return Some(format!("opens a file other than read-only binary: {line}"));
            }
            let module = line
                .strip_prefix("import ")
                .or_else(|| line.strip_prefix("from "))
                .map(|rest| rest.split([' ', '.', ',']).next().unwrap_or_default());
            if let Some(module) = module {
                if !ALLOWED_PYTHON_IMPORTS.contains(&module) {
                    return Some(format!(
                        "imports {module:?}, which is not in the allow list"
                    ));
                }
            }
        }
    }
    None
}

#[test]
fn javascript_imports_must_be_explicitly_reviewed() {
    let allowed = vec!["./colors.mjs".to_owned()];
    assert!(script_problem(
        "main.mjs",
        "import { results } from \"./colors.mjs\";",
        &allowed
    )
    .is_none());
    for source in [
        "import { results } from \"./unlisted.mjs\";",
        "import http from \"node:http\";",
        "import(\"node:fs\");",
        "eval(query);",
    ] {
        assert!(
            script_problem("main.mjs", source, &allowed).is_some(),
            "{source}"
        );
    }
}

#[test]
fn gallery_workflows_validate_and_stay_within_the_policy() {
    let mut keywords: BTreeMap<String, String> = BTreeMap::new();
    let owners = KeywordOwners::builtin(&Config::default());
    for entry in entries_of(Kind::Workflow) {
        let id = entry.id.as_str();
        let (workflow, dir) = load_workflow(&entry);
        // Not even a warning: nothing unreachable, no trigger leading nowhere.
        let problems = workflow.validate();
        assert!(problems.is_empty(), "{id}: {problems:?}");
        assert!(
            workflow.keyword_problems(&owners, None).is_empty(),
            "{id}: keyword clash"
        );
        assert_eq!(workflow.name, entry.name, "{id}: name");
        assert_eq!(workflow.author, "Sevak", "{id}");
        assert_eq!(workflow.version, entry.version, "{id}");
        assert!(workflow.enabled);

        let mut runs_code = false;
        for node in &workflow.nodes {
            match &node.kind {
                NodeKind::Keyword { keyword, .. } => {
                    let key = keyword.to_lowercase();
                    if let Some(other) = keywords.insert(key.clone(), id.to_owned()) {
                        panic!("{id} and {other} both use the keyword {key:?}");
                    }
                }
                NodeKind::OpenUrl { url } => {
                    let url = url.trim();
                    if url.starts_with("{query|raw}") || url.starts_with("https://{query|raw}") {
                        // Only the link opener passes whole addresses on, and it
                        // guards them with a regular expression first.
                        assert_eq!(id, "open-selected-link", "{id}/{}", node.id);
                        continue;
                    }
                    let host = host_of(url)
                        .unwrap_or_else(|| panic!("{id}/{}: not an https link: {url}", node.id));
                    assert!(
                        ALLOWED_HOSTS.contains(&host.as_str()),
                        "{id}/{}: {host} is not an allowed host",
                        node.id
                    );
                }
                NodeKind::RunScript {
                    command, script, ..
                } => {
                    runs_code = true;
                    assert!(command.is_empty(), "{id}/{}: use a script file", node.id);
                    let name = script.as_deref().unwrap();
                    let text = std::fs::read_to_string(dir.join(name))
                        .unwrap_or_else(|err| panic!("{id}: {name}: {err}"));
                    if let Some(problem) = script_problem(name, &text, &[]) {
                        panic!("{id}: {name} {problem}");
                    }
                }
                NodeKind::Selection { .. }
                | NodeKind::Transform { .. }
                | NodeKind::Conditional { .. }
                | NodeKind::SetVariable { .. }
                | NodeKind::Copy { .. }
                | NodeKind::Paste { .. }
                | NodeKind::Notification { .. }
                | NodeKind::LargeType { .. }
                | NodeKind::TextView { .. } => {}
                other => panic!(
                    "{id}/{}: {} is not allowed in the gallery",
                    node.id,
                    other.type_name()
                ),
            }
        }
        // The description is honest about code.
        let says_no_code = entry.description.contains("Runs no code");
        assert_eq!(says_no_code, !runs_code, "{id}: description vs. nodes");
        if runs_code {
            assert!(
                entry.tags.iter().any(|t| t.starts_with("needs-")),
                "{id}: tag needs-<tool>"
            );
            assert!(entry.description.contains("after you allow it"), "{id}");
        } else {
            assert!(
                entry.tags.iter().any(|t| t == "no-code"),
                "{id}: tag no-code"
            );
        }
        // Only the files a workflow needs.
        for file in std::fs::read_dir(&dir).unwrap() {
            let name = file.unwrap().file_name().to_string_lossy().into_owned();
            assert!(
                name == "workflow.toml" || name.ends_with(".py"),
                "{id}: stray file {name}"
            );
        }
    }
}

// ---- workflows: running them -------------------------------------------------

#[derive(Default)]
struct Recorder {
    urls: Mutex<Vec<String>>,
    clipboard: Mutex<Vec<String>>,
    pasted: Mutex<Vec<String>>,
}

impl PlatformProvider for Recorder {
    fn list_applications(&self) -> PlatformResult<Vec<AppEntry>> {
        Ok(Vec::new())
    }
    fn launch(&self, _target: &LaunchTarget) -> PlatformResult<()> {
        Ok(())
    }
    fn load_icon(&self, _source: &IconSource, _size: u32) -> PlatformResult<IconData> {
        Err(PlatformError::Unsupported("icons in tests"))
    }
    fn open_url(&self, url: &str) -> PlatformResult<()> {
        self.urls.lock().unwrap().push(url.to_owned());
        Ok(())
    }
    fn set_clipboard_text(&self, text: &str) -> PlatformResult<()> {
        self.clipboard.lock().unwrap().push(text.to_owned());
        Ok(())
    }
    // The default would write the real clipboard.
    fn paste_text(&self, text: &str, _restore: bool) -> PlatformResult<PasteOutcome> {
        self.pasted.lock().unwrap().push(text.to_owned());
        Ok(PasteOutcome::Pasted)
    }
}

#[derive(Default)]
struct Shown {
    notes: Mutex<Vec<(String, String)>>,
    large: Mutex<Vec<String>>,
    views: Mutex<Vec<(String, String)>>,
}

impl OutputSink for Shown {
    fn notify(&self, heading: &str, body: &str) {
        self.notes
            .lock()
            .unwrap()
            .push((heading.to_owned(), body.to_owned()));
    }
    fn large_type(&self, text: &str) {
        self.large.lock().unwrap().push(text.to_owned());
    }
    fn text_view(&self, heading: &str, text: &str) {
        self.views
            .lock()
            .unwrap()
            .push((heading.to_owned(), text.to_owned()));
    }
}

/// What one run did.
struct Outcome {
    report: RunReport,
    urls: Vec<String>,
    clipboard: Vec<String>,
    pasted: Vec<String>,
    notes: Vec<(String, String)>,
    large: Vec<String>,
    views: Vec<(String, String)>,
}

fn run_with(folder: &str, start: &str, arg: &str, vars: &[(&str, &str)]) -> Outcome {
    let entry = entries_of(Kind::Workflow)
        .into_iter()
        .find(|e| e.id == folder)
        .unwrap_or_else(|| panic!("{folder} is not in the gallery"));
    let (workflow, dir) = load_workflow(&entry);
    let platform = Arc::new(Recorder::default());
    let shown = Arc::new(Shown::default());
    let data = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(
        folder,
        dir,
        data.path().to_path_buf(),
        workflow,
        platform.clone(),
        shown.clone(),
        &Config::default(),
    );
    let mut ctx = Ctx::with_arg(arg);
    for (name, value) in vars {
        ctx.vars.insert((*name).to_owned(), (*value).to_owned());
    }
    let report = runtime.run_blocking(start, ctx);
    let urls = platform.urls.lock().unwrap().clone();
    let clipboard = platform.clipboard.lock().unwrap().clone();
    let pasted = platform.pasted.lock().unwrap().clone();
    let notes = shown.notes.lock().unwrap().clone();
    let large = shown.large.lock().unwrap().clone();
    let views = shown.views.lock().unwrap().clone();
    Outcome {
        report,
        urls,
        clipboard,
        pasted,
        notes,
        large,
        views,
    }
}

fn run(folder: &str, start: &str, arg: &str) -> Outcome {
    run_with(folder, start, arg, &[])
}

fn opened(folder: &str, start: &str, arg: &str) -> Vec<String> {
    let out = run(folder, start, arg);
    assert!(
        out.report.errors.is_empty(),
        "{folder}/{start}: {:?}",
        out.report.errors
    );
    out.urls
}

#[test]
fn the_search_workflows_open_the_right_pages_with_the_text_encoded() {
    let text = "rust lifetimes & more/é";
    let encoded = "rust%20lifetimes%20%26%20more%2F%C3%A9";
    for (folder, start, base) in [
        ("dev-search", "kw-so", "https://stackoverflow.com/search?q="),
        (
            "dev-search",
            "kw-mdn",
            "https://developer.mozilla.org/en-US/search?q=",
        ),
        ("dev-search", "kw-crate", "https://crates.io/search?q="),
        (
            "dev-search",
            "kw-docsrs",
            "https://docs.rs/releases/search?query=",
        ),
        ("dev-search", "kw-npm", "https://www.npmjs.com/search?q="),
        ("dev-search", "kw-pypi", "https://pypi.org/search/?q="),
        ("github-search", "kw-repos", "https://github.com/search?q="),
        ("github-search", "kw-issues", "https://github.com/search?q="),
        ("github-search", "kw-users", "https://github.com/search?q="),
        (
            "wikipedia-search",
            "kw-wiki",
            "https://en.wikipedia.org/w/index.php?search=",
        ),
        (
            "maps-search",
            "kw-osm",
            "https://www.openstreetmap.org/search?query=",
        ),
        (
            "maps-search",
            "kw-google",
            "https://www.google.com/maps/search/?api=1&query=",
        ),
    ] {
        let urls = opened(folder, start, text);
        assert_eq!(urls.len(), 1, "{folder}/{start}");
        assert!(urls[0].starts_with(base), "{folder}/{start}: {}", urls[0]);
        assert!(urls[0].contains(encoded), "{folder}/{start}: {}", urls[0]);
    }
    // The GitHub searches differ in what they look for.
    assert!(opened("github-search", "kw-repos", "x")[0].ends_with("&type=repositories"));
    assert!(opened("github-search", "kw-issues", "x")[0].ends_with("&type=issues"));
    assert!(opened("github-search", "kw-users", "x")[0].ends_with("&type=users"));
    // The Wikipedia language is a variable (an edit in the builder, or a trigger's).
    assert_eq!(
        run_with("wikipedia-search", "kw-wiki", "Bach", &[("lang", "de")]).urls,
        ["https://de.wikipedia.org/w/index.php?search=Bach"]
    );
    // The text cannot change which site opens, whatever it contains.
    let urls = opened("dev-search", "kw-so", "x#evil.example/?y=1&z=2 ");
    assert!(
        urls[0].starts_with("https://stackoverflow.com/search?q=x%23evil.example%2F%3Fy%3D1"),
        "{urls:?}"
    );
}

#[test]
fn open_selected_link_opens_only_web_and_mail_addresses() {
    let open = |arg: &str| run("open-selected-link", "selection", arg);
    for (selected, expected) in [
        (
            "https://example.com/docs?x=1&y=2",
            "https://example.com/docs?x=1&y=2",
        ),
        ("  http://example.com/a \n", "http://example.com/a"),
        ("HTTPS://EXAMPLE.COM", "HTTPS://EXAMPLE.COM"),
        ("mailto:ada@example.com", "mailto:ada@example.com"),
        ("example.com/docs", "https://example.com/docs"),
        ("www.rust-lang.org", "https://www.rust-lang.org"),
        (
            "docs.example.co.uk:8080/a?b=c#d",
            "https://docs.example.co.uk:8080/a?b=c#d",
        ),
    ] {
        let out = open(selected);
        assert!(
            out.report.errors.is_empty(),
            "{selected}: {:?}",
            out.report.errors
        );
        assert_eq!(out.urls, [expected], "{selected:?}");
        assert!(out.notes.is_empty(), "{selected:?}");
    }
    for refused in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "ftp://example.com/a",
        "data:text/html,hi",
        "hello world",
        "https://example.com/a b",
        "example",
        "C:\\Windows\\System32",
        "",
    ] {
        let out = open(refused);
        assert!(out.urls.is_empty(), "{refused:?} opened {:?}", out.urls);
        assert_eq!(
            out.notes.len(),
            1,
            "{refused:?} should be refused with a notification"
        );
    }
}

#[test]
fn markdown_helpers_format_the_selection() {
    let md = |start: &str, arg: &str| run("markdown-tools", start, arg);

    let out = md("link", "https://www.example.com/a_(b)?q=1");
    assert!(out.report.errors.is_empty(), "{:?}", out.report.errors);
    assert_eq!(
        out.clipboard,
        ["[example.com](https://www.example.com/a_%28b%29?q=1)"]
    );
    assert_eq!(out.notes.len(), 1);
    // Only the first of several links is used, and the port stays in the label.
    let out = md("link", "http://localhost:8080/x\nhttps://other.example");
    assert_eq!(out.clipboard, ["[localhost:8080](http://localhost:8080/x)"]);

    assert_eq!(md("bold", "hello there").pasted, ["**hello there**"]);
    assert_eq!(md("code", "x + y").pasted, ["`x + y`"]);
    assert_eq!(md("code", "a\nb").pasted, ["```\na\nb\n```"]);
    assert_eq!(md("quote", "one\ntwo\n\n").pasted, ["> one\n> two"]);
    assert_eq!(md("quote", "single").pasted, ["> single"]);
    assert_eq!(
        md("list", "one\n  two  \n\nthree\n").pasted,
        ["- one\n- two\n\n- three"]
    );
    assert_eq!(md("list", "solo").pasted, ["- solo"]);
}

#[test]
fn decode_and_encode_use_the_transform_nodes() {
    let tool = |start: &str, arg: &str| run("decode-tools", start, arg);

    let out = tool("b64-decode", " SGVsbG8sIFNldmFrIQ==\n");
    assert_eq!(
        out.views,
        [("Decode Base64".to_owned(), "Hello, Sevak!".to_owned())]
    );
    // URL-safe and unpadded input decodes too.
    assert_eq!(
        tool("b64-decode", "4pyTIMOgIGxhIG1vZGU").views[0].1,
        "\u{2713} \u{e0} la mode"
    );
    let out = tool("url-decode", "a%20b%26c%2Fd+e");
    assert_eq!(out.views[0].1, "a b&c/d+e");

    let out = tool("b64-encode", "Hello, Sevak!");
    assert_eq!(out.clipboard, ["SGVsbG8sIFNldmFrIQ=="]);
    assert_eq!(out.notes.len(), 1);
    assert_eq!(
        tool("url-encode", "a b&c/\u{e9}").clipboard,
        ["a%20b%26c%2F%C3%A9"]
    );

    // Garbage stops with an error instead of showing something wrong.
    let out = tool("b64-decode", "***not base64***");
    assert!(out.views.is_empty());
    assert_eq!(out.report.errors.len(), 1, "{:?}", out.report.errors);
    assert_eq!(out.report.errors[0].node, "b64-decode-do");
    let out = tool("url-decode", "%FF%FE");
    assert!(out.views.is_empty() && out.report.errors.len() == 1);
}

// ---- Python ------------------------------------------------------------------

/// A Python 3 interpreter as Sevak would find one (`py -3` on Windows, where
/// `python3` can be a Store stub that opens the Store), or None.
fn python() -> Option<Vec<String>> {
    let candidates: &[&[&str]] = if cfg!(windows) {
        &[&["py", "-3"], &["python"]]
    } else {
        &[&["python3"], &["python"]]
    };
    for candidate in candidates {
        let output = Command::new(candidate[0])
            .args(&candidate[1..])
            .arg("--version")
            .output();
        if let Ok(output) = output {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if output.status.success() && text.trim_start().starts_with("Python 3") {
                return Some(candidate.iter().map(|part| (*part).to_owned()).collect());
            }
        }
    }
    None
}

fn require_python() -> Option<Vec<String>> {
    let found = python();
    if found.is_none() {
        assert!(
            std::env::var_os("SEVAK_REQUIRE_PYTHON").is_none(),
            "SEVAK_REQUIRE_PYTHON is set but no Python 3 was found"
        );
        eprintln!("skipping: no Python 3 on this computer");
    }
    found
}

#[test]
fn the_selection_toolkit_runs_its_script() {
    if require_python().is_none() {
        return;
    }
    let tool = |start: &str, arg: &str| run("selection-toolkit", start, arg);

    let out = tool("stats", "one two\nthree");
    assert!(out.report.errors.is_empty(), "{:?}", out.report.errors);
    assert_eq!(out.large, ["3 words \u{b7} 13 characters \u{b7} 2 lines"]);
    assert_eq!(
        tool("stats", "x").large,
        ["1 word \u{b7} 1 character \u{b7} 1 line"]
    );

    let out = tool("pretty", "{\"a\":[1,2],\"b\":\"\u{e9}\"}");
    assert_eq!(out.views.len(), 1);
    assert_eq!(
        out.views[0].1,
        "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": \"\u{e9}\"\n}"
    );
    // Invalid JSON is explained, not hidden.
    assert!(tool("pretty", "{nope").views[0]
        .1
        .starts_with("Not valid JSON:"));

    assert_eq!(
        tool("minify", "{ \"a\" : [1, 2] ,\n \"b\": null }").pasted,
        ["{\"a\":[1,2],\"b\":null}"]
    );
    // Nothing is pasted over the selection when it is not JSON.
    let out = tool("minify", "{nope");
    assert!(out.pasted.is_empty());
    assert_eq!(out.report.errors.len(), 1, "{:?}", out.report.errors);

    let seconds = &tool("time", "1700000000").views[0].1;
    assert!(seconds.contains("1700000000 Unix seconds"), "{seconds}");
    assert!(
        seconds.contains("UTC:      2023-11-14 22:13:20"),
        "{seconds}"
    );
    assert!(
        seconds.contains("ISO 8601: 2023-11-14T22:13:20Z"),
        "{seconds}"
    );
    let millis = &tool("time", "1700000000123").views[0].1;
    assert!(
        millis.contains("Unix milliseconds") && millis.contains("2023-11-14T22:13:20Z"),
        "{millis}"
    );
    let date = &tool("time", "2023-11-14T22:13:20Z").views[0].1;
    assert!(date.contains("Unix seconds:      1700000000"), "{date}");
    assert!(date.contains("Unix milliseconds: 1700000000000"), "{date}");
    let naive = &tool("time", "2023-11-14").views[0].1;
    assert!(
        naive.contains("As UTC:") && naive.contains("Unix seconds:      1699920000"),
        "{naive}"
    );
    assert!(tool("time", "tomorrow-ish").views[0]
        .1
        .starts_with("Select a Unix timestamp"));
}

// ---- script plugins ----------------------------------------------------------

#[test]
fn gallery_plugins_load_have_free_keywords_and_stay_within_the_policy() {
    let owners = KeywordOwners::builtin(&Config::default());
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for entry in entries_of(Kind::Plugin) {
        let id = entry.id.as_str();
        let dir = examples(Kind::Plugin).join(id);
        let manifest = Manifest::load(&dir).unwrap_or_else(|err| panic!("{id}: {err}"));
        assert!(
            manifest.warnings.is_empty(),
            "{id}: {:?}",
            manifest.warnings
        );
        let expected_mode = if id == "pomodoro" {
            Mode::Persistent
        } else {
            Mode::Oneshot
        };
        assert_eq!(manifest.mode, expected_mode, "{id}");
        assert!(
            matches!(manifest.format, Format::Sevak | Format::Alfred),
            "{id}"
        );
        assert!(!manifest.description.is_empty(), "{id}");
        assert!(
            manifest.name == entry.name,
            "{id}: name differs from the index"
        );
        let Launch::Script(_) = &manifest.launch else {
            panic!("{id}: use `script`, not `command`, so the interpreter is Sevak's choice");
        };
        let support = manifest.support_files();
        let mut imports: Vec<String> = manifest.files.iter().map(|f| format!("./{f}")).collect();
        // Pomodoro saves only its documented timer state in SEVAK_PLUGIN_DATA.
        if id == "pomodoro" {
            imports.extend(["node:fs", "node:path", "node:readline"].map(str::to_owned));
        }
        for file in &support {
            let text =
                std::fs::read_to_string(dir.join(file)).unwrap_or_else(|err| panic!("{id}: {err}"));
            if let Some(problem) = script_problem(file, &text, &imports) {
                panic!("{id}: {file} {problem}");
            }
        }
        // The keyword is free: no built-in, search engine, workflow or other plugin.
        let keyword = manifest.keyword.to_lowercase();
        assert!(
            owners.owners_of(&keyword, None).is_empty(),
            "{id}: {keyword:?} is taken"
        );
        if let Some(other) = seen.insert(keyword.clone(), id.to_owned()) {
            panic!("{id} and {other} both use {keyword:?}");
        }
        for workflow in entries_of(Kind::Workflow) {
            let (workflow, _) = load_workflow(&workflow);
            for (_, used) in workflow.keyword_nodes() {
                assert!(
                    !used.eq_ignore_ascii_case(&keyword),
                    "{id}: a workflow uses {keyword:?}"
                );
            }
        }
        assert!(
            entry.description.contains("Offline")
                || entry.description.contains("needs ")
                || entry.description.contains("Requires "),
            "{id}"
        );
        assert!(
            entry.tags.iter().any(|t| t.starts_with("needs-")),
            "{id}: tag needs-<tool>"
        );
        for file in std::fs::read_dir(&dir).unwrap() {
            let name = file.unwrap().file_name().to_string_lossy().into_owned();
            assert!(
                matches!(name.as_str(), "plugin.toml" | "README.md" | "LICENSE")
                    || support.contains(&name),
                "{id}: stray file {name}"
            );
        }
    }
}

/// Runs a plugin script with the query as its argument and returns its items.
fn ask(python: &[String], plugin: &str, script: &str, query: &str) -> Vec<Value> {
    let dir = examples(Kind::Plugin).join(plugin);
    let output = Command::new(&python[0])
        .args(&python[1..])
        .arg("-u")
        .arg(script)
        .arg(query)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{plugin} {query:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.is_ascii(),
        "{plugin}: the output must be plain ASCII JSON"
    );
    let json: Value =
        serde_json::from_str(&text).unwrap_or_else(|err| panic!("{plugin}: {err}: {text}"));
    let items = json["items"].as_array().unwrap().clone();
    assert!(!items.is_empty() && items.len() <= 50, "{plugin} {query:?}");
    for item in &items {
        assert!(item["title"]
            .as_str()
            .is_some_and(|t| !t.is_empty() && t.chars().count() <= 200));
        assert!(item["key"].as_str().is_some_and(|k| !k.is_empty()));
        assert_eq!(item["action"]["type"], "copy_text", "{plugin}: {item}");
        assert!(item["action"]["text"].as_str().is_some());
    }
    items
}

fn titles(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .map(|i| i["title"].as_str().unwrap().to_owned())
        .collect()
}

fn item<'a>(items: &'a [Value], key: &str) -> &'a str {
    items
        .iter()
        .find(|i| i["key"] == key)
        .unwrap_or_else(|| panic!("no row {key}"))["title"]
        .as_str()
        .unwrap()
}

#[test]
fn the_password_generator_makes_random_passwords_of_the_right_shape() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "password-generator", "password.py", query);

    let items = ask("");
    let strong = item(&items, "strong");
    assert_eq!(strong.len(), 20);
    assert!(strong.chars().any(|c| c.is_ascii_lowercase()));
    assert!(strong.chars().any(|c| c.is_ascii_uppercase()));
    assert!(strong.chars().any(|c| c.is_ascii_digit()));
    assert!(strong.chars().any(|c| "!@#$%^&*-_=+?".contains(c)));
    let plain = item(&items, "letters-digits");
    assert!(plain.len() == 20 && plain.chars().all(|c| c.is_ascii_alphanumeric()));
    let easy = item(&items, "easy");
    assert!(
        easy.len() == 16
            && easy
                .chars()
                .all(|c| c.is_ascii_alphanumeric() && !"0O1lI".contains(c))
    );
    let pin = item(&items, "pin");
    assert!(pin.len() == 6 && pin.chars().all(|c| c.is_ascii_digit()));
    let hex = item(&items, "hex");
    assert!(hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit()));
    // Fresh every time.
    assert_ne!(strong, item(&ask(""), "strong"));

    // A length changes the rows that have one; limits hold.
    let items = ask("pw 40");
    assert_eq!(item(&items, "strong").len(), 40);
    assert_eq!(item(&items, "pin").len(), 6);
    assert_eq!(item(&ask("9"), "pin").len(), 9);
    assert_eq!(item(&ask("100000"), "strong").len(), 128);
    assert_eq!(item(&ask("1"), "strong").len(), 4);
}

#[test]
fn the_id_generator_makes_well_formed_ids() {
    let Some(py) = require_python() else { return };
    let items = ask(&py, "id-generator", "ids.py", "");
    let uuid4 = item(&items, "uuid4");
    let uuid7 = item(&items, "uuid7");
    for uuid in [uuid4, uuid7] {
        let parts: Vec<&str> = uuid.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12],
            "{uuid}"
        );
        assert!(uuid
            .chars()
            .all(|c| c == '-' || c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert!(
            "89ab".contains(parts[3].chars().next().unwrap()),
            "variant of {uuid}"
        );
    }
    assert!(uuid4.split('-').nth(2).unwrap().starts_with('4'));
    assert!(uuid7.split('-').nth(2).unwrap().starts_with('7'));
    // A version 7 id starts with the current time in milliseconds.
    let millis = u64::from_str_radix(&uuid7.replace('-', "")[..12], 16).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(now.abs_diff(millis) < 60_000, "{millis} vs {now}");
    let ulid = item(&items, "ulid");
    assert!(
        ulid.len() == 26
            && ulid
                .chars()
                .all(|c| "0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(c))
    );
    assert!(
        ulid.starts_with('0'),
        "a ULID of today starts with a small digit"
    );
    let nanoid = item(&items, "nanoid");
    assert!(
        nanoid.len() == 21
            && nanoid
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    );
    let compact = item(&items, "uuid4-compact");
    assert!(compact.len() == 32 && compact.chars().all(|c| c.is_ascii_hexdigit()));
    let all = titles(&items);
    assert_eq!(all.iter().collect::<HashSet<_>>().len(), all.len());
}

#[test]
fn the_color_converter_converts_between_formats() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "color-converter", "color.py", query);
    for query in [
        "#ff8800",
        "ff8800",
        "#f80",
        "rgb(255, 136, 0)",
        "rgb 255 136 0",
        "255,136,0",
        "hsl(32, 100%, 50%)",
        "hsl(32deg 100% 50%)",
        "hsv(32 100% 100%)",
        "rgb(100% 53.3% 0%)",
    ] {
        let items = ask(query);
        assert_eq!(item(&items, "hex"), "#ff8800", "{query}");
        assert_eq!(item(&items, "rgb"), "rgb(255, 136, 0)", "{query}");
        assert_eq!(item(&items, "hsl"), "hsl(32, 100%, 50%)", "{query}");
        assert_eq!(item(&items, "hsv"), "hsv(32, 100%, 100%)", "{query}");
        assert_eq!(
            item(&items, "contrast"),
            "2.39:1 on white, 8.77:1 on black",
            "{query}"
        );
    }
    let items = ask("Tomato");
    assert_eq!(item(&items, "hex"), "#ff6347");
    assert_eq!(item(&items, "name"), "tomato");
    let items = ask("rgba(0, 0, 0, 0.5)");
    assert_eq!(item(&items, "hex"), "#00000080");
    assert_eq!(item(&items, "rgb"), "rgba(0, 0, 0, 0.5)");
    assert_eq!(item(&items, "hsl"), "hsla(0, 0%, 0%, 0.5)");
    assert_eq!(
        item(&ask("white"), "contrast"),
        "1.00:1 on white, 21.00:1 on black"
    );
    // Not a color, or nothing typed yet: one hint row, never a crash.
    assert_eq!(titles(&ask("not a color")), ["That is not a color I know"]);
    assert_eq!(titles(&ask("")), ["Type a color"]);
    assert_eq!(titles(&ask("rgb(1, 2)")), ["That is not a color I know"]);
    assert_eq!(titles(&ask("#12345")), ["That is not a color I know"]);
}

#[test]
fn the_lorem_ipsum_plugin_is_deterministic_and_bounded() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "lorem-ipsum", "lorem.py", query);
    let text = |items: &[Value]| items[0]["action"]["text"].as_str().unwrap().to_owned();

    assert_eq!(ask("").len(), 6);
    let three = ask("3 paragraphs");
    assert_eq!(three.len(), 1);
    let body = text(&three);
    assert_eq!(body.split("\n\n").count(), 3);
    assert!(body.starts_with("Lorem ipsum dolor sit amet, consectetur adipiscing elit."));
    assert_eq!(three[0]["view"], "text");
    assert_eq!(three[0]["text"].as_str().unwrap(), body);
    assert_eq!(
        text(&ask("3 paragraphs")),
        body,
        "the same request gives the same text"
    );
    assert_eq!(text(&ask("3 p")), body);

    let words = text(&ask("8 words"));
    assert_eq!(words.split(' ').count(), 8);
    assert!(words.starts_with("Lorem ipsum dolor sit amet") && words.ends_with('.'));
    assert_eq!(text(&ask("40")).split(' ').count(), 40);
    assert_eq!(text(&ask("100000 words")).split(' ').count(), 500);
    let sentences = text(&ask("2 sentences"));
    assert_eq!(sentences.matches(". ").count(), 1);
    assert!(sentences.ends_with('.'));
    // An unknown unit falls back to the overview.
    assert_eq!(ask("12 furlongs").len(), 6);
}

#[test]
fn the_hash_calculator_matches_known_digests_for_text_and_files() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "hash-calculator", "hashes.py", query);
    let check = |items: &[Value]| {
        assert_eq!(item(items, "md5"), "5d41402abc4b2a76b9719d911017c592");
        assert_eq!(
            item(items, "sha1"),
            "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        );
        assert_eq!(
            item(items, "sha256"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(item(items, "crc32"), "3610a686");
        assert_eq!(item(items, "sha512").len(), 128);
        assert_eq!(item(items, "sha3_256").len(), 64);
    };
    let items = ask("hello");
    check(&items);
    assert!(items[0]["subtitle"]
        .as_str()
        .unwrap()
        .contains("text (5 bytes"));

    // The same bytes in a file, named by absolute path (also in quotes).
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hello.txt");
    std::fs::write(&file, b"hello").unwrap();
    let path = file.to_string_lossy().into_owned();
    let items = ask(&path);
    check(&items);
    assert!(items[0]["subtitle"]
        .as_str()
        .unwrap()
        .contains("file hello.txt (5 bytes)"));
    check(&ask(&format!("\"{path}\"")));
    // A path that does not exist is just text, and a relative name never reads a file.
    let missing = dir
        .path()
        .join("missing.txt")
        .to_string_lossy()
        .into_owned();
    assert!(ask(&missing)[0]["subtitle"]
        .as_str()
        .unwrap()
        .contains("text ("));
    assert!(ask("plugin.toml")[0]["subtitle"]
        .as_str()
        .unwrap()
        .contains("text ("));
    // Unicode is hashed as UTF-8.
    assert_eq!(
        item(&ask("h\u{e9}llo"), "sha256"),
        sha256_hex("h\u{e9}llo".as_bytes())
    );
    assert_eq!(
        titles(&ask("")),
        ["Type some text, or the full path of a file"]
    );
}

// ---- the second plugin pack ----------------------------------------------------

#[test]
fn the_jwt_decoder_decodes_without_verifying() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "jwt-decoder", "jwt.py", query);
    let old = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4iLCJpYXQiOjE1MTYyMzkwMjIsImV4cCI6MTUxNjI0MjYyMn0.sig";
    for query in [
        old.to_owned(),
        format!("Bearer {old}"),
        format!("\"{old}\""),
    ] {
        let items = ask(&query);
        assert_eq!(
            item(&items, "header"),
            "{\"alg\": \"HS256\", \"typ\": \"JWT\"}"
        );
        assert!(item(&items, "payload").contains("\"sub\": \"1234567890\""));
        assert!(item(&items, "iat").starts_with("Issued: 2018-01-18 01:30:22 UTC ("));
        assert!(item(&items, "exp").starts_with("Expires: 2018-01-18 02:30:22 UTC ("));
        assert_eq!(item(&items, "status"), "Expired");
        assert_eq!(item(&items, "sub"), "sub: 1234567890");
        assert!(item(&items, "signature").contains("NOT verified"));
    }
    // The pretty JSON is offered in the text view.
    let items = ask(old);
    assert!(items[0]["text"].as_str().unwrap().contains("\n  \"alg\""));
    // A token that expires in 2100 is not expired; "alg none" is called out.
    let future = ask("eyJhbGciOiJub25lIn0.eyJleHAiOjQxMDI0NDQ4MDAsImlzcyI6InNldmFrIn0.");
    assert!(item(&future, "status").starts_with("Not expired"));
    assert!(item(&future, "exp").contains("2100-01-01 00:00:00 UTC (in "));
    assert_eq!(item(&future, "iss"), "iss: sevak");
    assert!(item(&future, "signature").starts_with("No signature"));
    // Not a token, or nothing typed: one hint row.
    assert_eq!(titles(&ask("")), ["Paste a JWT to decode it"]);
    for bad in ["abc", "a.b.c", "e30.e30", "!!.!!.!!", "e30.W10.x"] {
        assert_eq!(
            titles(&ask(bad)),
            ["That does not look like a JWT"],
            "{bad}"
        );
    }
}

#[test]
fn the_base_converter_converts_between_bases() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "base-converter", "base.py", query);
    for query in [
        "255",
        "0xff",
        "0XFF",
        "0b11111111",
        "0o377",
        "ff 16",
        "11111111 2",
        "377 8",
        "2_5_5",
    ] {
        let items = ask(query);
        assert_eq!(item(&items, "dec"), "255", "{query}");
        assert_eq!(item(&items, "hex"), "0xff", "{query}");
        assert_eq!(item(&items, "bin"), "0b11111111", "{query}");
        assert_eq!(item(&items, "oct"), "0o377", "{query}");
        assert_eq!(item(&items, "bin-grouped"), "1111 1111", "{query}");
        assert_eq!(item(&items, "b36"), "73", "{query}");
        assert_eq!(item(&items, "size"), "8 bits, 1 byte", "{query}");
    }
    let items = ask("-10");
    assert_eq!(item(&items, "hex"), "-0xa");
    assert_eq!(item(&items, "bin"), "-0b1010");
    assert_eq!(item(&ask("0"), "bin"), "0b0");
    assert_eq!(item(&ask("zz 36"), "dec"), "1295");
    assert_eq!(item(&ask("256"), "hex-grouped"), "01 00");
    // A huge number is refused rather than printed; so is nonsense.
    let huge = "9".repeat(100);
    for bad in [
        huge.as_str(),
        "xyz",
        "12 99",
        "0xfg",
        "0b102",
        "0xff 10",
        "1 2 3",
    ] {
        assert_eq!(
            titles(&ask(bad)),
            ["That is not a number I can convert"],
            "{bad}"
        );
    }
    assert_eq!(titles(&ask("")), ["Type a number"]);
}

#[test]
fn the_cron_explainer_explains_and_lists_future_runs() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "cron-explainer", "cron.py", query);
    for (expression, words) in [
        ("*/15 * * * *", "Every 15 minutes"),
        ("* * * * *", "Every minute"),
        ("30 9 * * mon-fri", "At 09:30 on Monday to Friday"),
        ("30 9 * * 1-5", "At 09:30 on Monday to Friday"),
        ("0 0 * * 7", "At 00:00 on Sunday"),
        ("0 9,17 * * 1", "At 09:00 and 17:00 on Monday"),
        ("5 * * * *", "At minute 5 of every hour"),
        ("0 0 1 * *", "At 00:00 on the 1st of the month"),
        (
            "0 12 1,15 jan,jul *",
            "At 12:00 on the 1st and 15th of the month in January and July",
        ),
        ("@daily", "At 00:00"),
        ("@hourly", "At minute 0 of every hour"),
    ] {
        let items = ask(expression);
        assert_eq!(item(&items, "explain"), words, "{expression}");
        assert_eq!(
            items.len(),
            6,
            "{expression}: the explanation and five runs"
        );
    }
    // Runs are in the future, in order, and match the schedule.
    let items = ask("30 9 * * mon-fri");
    let runs: Vec<String> = (0..5)
        .map(|i| item(&items, &format!("next-{i}")).to_owned())
        .collect();
    // "Mon 2026-10-05 09:30": the date part sorts in time order.
    let mut sorted = runs.clone();
    sorted.sort_by_key(|run| run[4..].to_owned());
    assert_eq!(runs, sorted);
    for run in &runs {
        assert!(run.ends_with(" 09:30"), "{run}");
        assert!(!run.starts_with("Sat") && !run.starts_with("Sun"), "{run}");
    }
    // Day-of-month and weekday together run when either matches.
    assert!(item(&ask("0 0 13 * fri"), "explain").contains("cron runs when either matches"));
    // February 30th never happens.
    assert_eq!(
        item(&ask("0 0 30 2 *"), "never"),
        "No run in the next 8 years"
    );
    for bad in [
        "* * * *",
        "60 * * * *",
        "* 24 * * *",
        "* * 0 * *",
        "* * * 13 *",
        "*/0 * * * *",
        "a b c d e",
        "5-1 * * * *",
    ] {
        assert_eq!(
            titles(&ask(bad)),
            ["That is not a valid cron expression"],
            "{bad}"
        );
    }
    assert_eq!(ask("").len(), 4);
}

#[test]
fn the_regex_tester_lists_matches_and_groups() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "regex-tester", "regex.py", query);
    let items = ask(r"(?P<area>\d+)-(\d+) :: call 555-1234 or 1-2");
    assert_eq!(item(&items, "count"), "2 matches");
    assert_eq!(item(&items, "m0"), "555-1234");
    assert_eq!(item(&items, "m0g1"), "555");
    assert_eq!(item(&items, "m0g2"), "1234");
    assert_eq!(item(&items, "m1"), "1-2");
    let all = items[0]["action"]["text"].as_str().unwrap();
    assert_eq!(all, "555-1234\n1-2");
    let named = items.iter().find(|i| i["key"] == "m0g1").unwrap();
    assert!(named["subtitle"].as_str().unwrap().contains("(area)"));
    assert_eq!(item(&ask(r"(?i)HELLO :: say hello"), "count"), "1 match");
    // The first " :: " splits pattern from text; a lone "::" works too.
    assert_eq!(item(&ask(r"\w+::\w+ :: a::b"), "count"), "1 match");
    assert_eq!(item(&ask("a|b::xxbxa"), "count"), "2 matches");
    assert_eq!(titles(&ask("zzz :: abc")), ["No match"]);
    // An optional group that did not take part has no value.
    assert_eq!(item(&ask("a(b)? :: a"), "m0g1"), "(no value)");
    // Invalid patterns are explained, never a crash.
    assert!(titles(&ask("(oops :: abc"))[0].starts_with("Invalid pattern:"));
    assert!(titles(&ask("[a-"))[0].starts_with("Invalid pattern:"));
    assert_eq!(
        titles(&ask("abc"))[0],
        "Valid pattern. Add \" :: \" and some text to test it"
    );
    assert_eq!(
        titles(&ask(""))[0],
        "Type a pattern, then \" :: \", then the text"
    );
    let long = format!("a :: {}", "a".repeat(2001));
    assert_eq!(
        titles(&ask(&long))[0],
        "Text is longer than 2000 characters"
    );
    // Many matches are capped, not dumped.
    assert!(ask(&format!(". :: {}", "x".repeat(1500))).len() <= 50);
}

#[test]
fn the_unicode_lookup_describes_characters_and_searches_names() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "unicode-lookup", "unicode_lookup.py", query);
    for query in ["U+1F600", "u+1f600", "0x1F600", "\\u{1F600}", "\u{1F600}"] {
        let items = ask(query);
        assert_eq!(item(&items, "name-1F600"), "GRINNING FACE", "{query}");
        assert_eq!(item(&items, "char-1F600"), "\u{1F600}", "{query}");
        assert_eq!(item(&items, "code-1F600"), "U+1F600", "{query}");
        assert_eq!(item(&items, "utf8-1F600"), "F0 9F 98 80", "{query}");
        assert_eq!(item(&items, "html-1F600"), "&#x1F600;", "{query}");
        assert_eq!(item(&items, "js-1F600"), "\\u{1F600}", "{query}");
        assert_eq!(item(&items, "py-1F600"), "\\U0001f600", "{query}");
        assert_eq!(item(&items, "dec-1F600"), "128512", "{query}");
    }
    let items = ask("\u{e9}");
    assert_eq!(item(&items, "name-E9"), "LATIN SMALL LETTER E WITH ACUTE");
    assert_eq!(item(&items, "utf8-E9"), "C3 A9");
    assert_eq!(item(&items, "nfd-E9"), "U+0065 + U+0301");
    assert_eq!(
        item(&ask("a"), "name-61"),
        "LATIN SMALL LETTER A",
        "one ASCII character is a lookup"
    );
    // Several pasted characters give one row each.
    assert_eq!(ask("\u{2192}\u{2190}").len(), 2);
    // A name search finds the obvious character first.
    let hits = ask("right arrow");
    assert!(
        titles(&hits)[0].ends_with("Rightwards Arrow"),
        "{:?}",
        titles(&hits)
    );
    assert_eq!(hits[0]["action"]["text"], "\u{2192}");
    assert!(ask("snowman")
        .iter()
        .any(|i| i["action"]["text"] == "\u{2603}"));
    assert_eq!(titles(&ask("qqqq zzzz")), ["No character with that name"]);
    assert_eq!(titles(&ask("U+110000")), ["Code points end at U+10FFFF"]);
    assert_eq!(ask("").len(), 1);
}

#[test]
fn the_http_status_reference_explains_codes_groups_and_words() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "http-status", "status.py", query);
    let items = ask("404");
    assert_eq!(titles(&items), ["404 Not Found"]);
    assert!(items[0]["subtitle"]
        .as_str()
        .unwrap()
        .starts_with("Client error."));
    assert_eq!(titles(&ask("418")), ["418 I'm a teapot"]);
    assert_eq!(titles(&ask("teapot")), ["418 I'm a teapot"]);
    for query in ["5", "5xx", "5x", "server"] {
        let codes = titles(&ask(query));
        assert!(codes.len() >= 6, "{query}");
        assert!(
            codes.iter().all(|t| t.starts_with('5')),
            "{query}: {codes:?}"
        );
    }
    let redirects = titles(&ask("redirect"));
    assert!(redirects.contains(&"301 Moved Permanently".to_owned()));
    assert!(redirects.contains(&"304 Not Modified".to_owned()));
    // Words search names and meanings.
    assert!(titles(&ask("timeout"))
        .iter()
        .any(|t| t == "504 Gateway Timeout"));
    assert!(titles(&ask("rate limit"))
        .iter()
        .any(|t| t == "429 Too Many Requests"));
    // A code that is not standard is said so, not guessed at.
    assert!(titles(&ask("499"))[0].contains("not a standard client error code"));
    assert_eq!(titles(&ask("xyzzy")), ["No status code matches"]);
    assert_eq!(ask("").len(), 14);
}

#[test]
fn the_port_reference_knows_common_ports() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "port-reference", "ports.py", query);
    assert_eq!(titles(&ask("443")), ["443  HTTPS (TCP)"]);
    assert_eq!(ask("443")[0]["action"]["text"], "443");
    assert_eq!(titles(&ask("postgres")), ["5432  PostgreSQL (TCP)"]);
    assert!(titles(&ask("ssh")).iter().any(|t| t.starts_with("22 ")));
    assert!(titles(&ask("dev server")).len() >= 3);
    assert!(titles(&ask("5173"))[0].contains("Vite"));
    // A number that is not listed is classified, not invented.
    assert_eq!(titles(&ask("999"))[0], "999 is not in this list");
    assert!(ask("999")[0]["subtitle"]
        .as_str()
        .unwrap()
        .contains("well-known"));
    assert!(ask("50000")[0]["subtitle"]
        .as_str()
        .unwrap()
        .contains("ephemeral"));
    assert!(ask("20000")[0]["subtitle"]
        .as_str()
        .unwrap()
        .contains("registered"));
    assert_eq!(titles(&ask("70000")), ["Ports go from 0 to 65535"]);
    assert_eq!(
        titles(&ask("nonesuch")),
        ["No port with that name in the list"]
    );
    assert_eq!(ask("").len(), 11);
}

#[test]
fn the_chmod_calculator_converts_octal_and_symbolic_modes() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "chmod-calculator", "chmod.py", query);
    for (query, octal, symbolic) in [
        ("755", "755", "rwxr-xr-x"),
        ("0644", "644", "rw-r--r--"),
        ("rwxr-xr-x", "755", "rwxr-xr-x"),
        ("-rw-r--r--", "644", "rw-r--r--"),
        ("drwxr-xr-x", "755", "rwxr-xr-x"),
        ("000", "000", "---------"),
        ("4755", "4755", "rwsr-xr-x"),
        ("rwSr--r--", "4644", "rwSr--r--"),
        ("2775", "2775", "rwxrwsr-x"),
        ("1777", "1777", "rwxrwxrwt"),
        ("rwxrwxrwT", "1776", "rwxrwxrwT"),
    ] {
        let items = ask(query);
        assert_eq!(item(&items, "octal"), octal, "{query}");
        assert_eq!(item(&items, "symbolic"), symbolic, "{query}");
        assert_eq!(
            item(&items, "command"),
            format!("chmod {octal} file"),
            "{query}"
        );
    }
    let items = ask("640");
    assert_eq!(item(&items, "owner"), "Owner: read, write");
    assert_eq!(item(&items, "group"), "Group: read");
    assert_eq!(item(&items, "others"), "Others: nothing");
    assert!(item(&ask("4755"), "special-4000").starts_with("setuid"));
    assert!(item(&ask("1777"), "special-1000").starts_with("sticky"));
    assert!(item(&ask("777"), "warn").starts_with("World-writable"));
    // Digits out of range, wrong lengths, misplaced special letters and the like.
    for bad in [
        "888",
        "75",
        "75555",
        "rwxrwxrw",
        "rwxr-xr-q",
        "rwtr-xr-x",
        "rwxr-xr-s",
        "u+x",
    ] {
        assert_eq!(
            titles(&ask(bad)),
            ["Type 755, 0644, 4755 or rwxr-xr-x"],
            "{bad}"
        );
    }
    assert_eq!(ask("").len(), 5);
}

#[test]
fn the_date_calculator_counts_and_shifts_dates() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "date-calculator", "days.py", query);
    let items = ask("2026-01-01 2026-12-31");
    assert_eq!(item(&items, "days"), "364 days");
    assert_eq!(item(&items, "breakdown"), "11 months, 30 days");
    assert_eq!(item(&items, "weeks"), "52 weeks and 0 days");
    assert_eq!(item(&items, "business"), "260 weekdays (Monday to Friday)");
    // Words, slashes, either order, and "to" all work.
    for query in [
        "2026-01-01 to 2026-12-31",
        "2026/1/1 until 2026-12-31",
        "2026-12-31 2026-01-01",
    ] {
        assert_eq!(item(&ask(query), "days"), "364 days", "{query}");
    }
    // Month arithmetic clamps to the end of a short month.
    assert_eq!(
        item(&ask("2026-01-31 2026-03-01"), "breakdown"),
        "1 month, 1 day"
    );
    assert_eq!(
        item(&ask("2024-02-29 2025-02-28"), "breakdown"),
        "11 months, 30 days"
    );
    assert_eq!(
        item(&ask("2000-01-01 2026-10-04"), "breakdown"),
        "26 years, 9 months, 3 days"
    );
    assert_eq!(item(&ask("2026-05-05 2026-05-05"), "days"), "the same day");
    // Adding and subtracting days.
    let items = ask("2026-03-01 + 45");
    assert_eq!(item(&items, "date"), "Wednesday 2026-04-15");
    assert_eq!(item(&items, "iso"), "2026-04-15");
    assert_eq!(item(&items, "week"), "Day 105 of 2026, ISO week 16");
    assert_eq!(item(&ask("2026-03-01 - 1 day"), "iso"), "2026-02-28");
    assert_eq!(item(&ask("2024-02-28 + 1"), "iso"), "2024-02-29");
    // Relative to today: the answer depends on the clock, so check its shape.
    let overview = ask("");
    let today = item(&overview, "today");
    assert!(today.contains(' ') && today.len() > 12, "{today}");
    let plus = ask("+90");
    assert!(item(&plus, "date").contains(" 20"));
    assert!(plus[0]["subtitle"]
        .as_str()
        .unwrap()
        .starts_with("90 days after "));
    assert_eq!(ask("today")[0]["title"], "today");
    assert_eq!(ask("tomorrow")[0]["title"], "in 1 day");
    assert_eq!(ask("yesterday")[0]["title"], "1 day ago");
    for bad in [
        "soon",
        "2026-13-01",
        "2026-02-30",
        "2026-01-01 2026-02-30",
        "0000-01-01",
    ] {
        assert_eq!(titles(&ask(bad)), ["I could not read that"], "{bad}");
    }
    assert_eq!(item(&ask("+99999999"), "bad"), "I could not read that");
}

#[test]
fn the_slugify_plugin_makes_slugs_and_counts_text() {
    let Some(py) = require_python() else { return };
    let ask = |query: &str| ask(&py, "slugify", "slug.py", query);
    let items = ask("  Hello, World! Caf\u{e9} \u{2013} Stra\u{df}e  ");
    assert_eq!(item(&items, "slug"), "hello-world-cafe-strasse");
    assert_eq!(item(&items, "snake"), "hello_world_cafe_strasse");
    assert_eq!(item(&items, "short"), "hello-world-cafe-strasse");
    assert_eq!(item(&items, "filename"), "Hello-World-Cafe-Strasse");
    assert_eq!(item(&items, "words"), "5 words");
    assert_eq!(item(&items, "bytes"), "31 bytes in UTF-8");
    assert_eq!(
        item(&ask("one two three four five six seven eight"), "short"),
        "one-two-three-four-five-six"
    );
    assert_eq!(item(&ask("a/b\\c:d*e?"), "filename"), "abcde");
    assert_eq!(item(&ask("***"), "slug"), "(nothing left after cleaning)");
    assert_eq!(item(&ask("***"), "filename"), "untitled");
    assert_eq!(item(&ask("x"), "words"), "1 word");
    assert_eq!(item(&ask("\u{4f60}\u{597d}"), "bytes"), "6 bytes in UTF-8");
    assert_eq!(titles(&ask(""))[0], "Type or paste some text");
    assert_eq!(
        titles(&ask(&"a".repeat(5001)))[0],
        "Text is longer than 5000 characters"
    );
}
