//! Fixtures for the extension tests: a fake network, temporary folders and
//! packages built in memory. No test here touches the real network or the
//! user's own folders.

use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use sevak_core::gallery_source::Pin;
use zip::write::SimpleFileOptions;

use super::package::Builder;
use super::store::{ExtensionStore, SharedTransport, StoreDirs};
use crate::net::{sha256_hex, FetchError, Transport};

pub(crate) const RAW: &str = "https://raw.githubusercontent.com/ninad-k/Sevak/v1.2.3/";

pub(crate) fn url(path: &str) -> String {
    format!("{RAW}{path}")
}

/// A network that serves canned files and records what was asked for.
#[derive(Default)]
pub(crate) struct Fake {
    pub(crate) files: Mutex<HashMap<String, Result<Vec<u8>, FetchError>>>,
    pub(crate) requests: Mutex<Vec<String>>,
}

impl Fake {
    pub(crate) fn put(&self, path: &str, bytes: Vec<u8>) {
        self.files.lock().unwrap().insert(url(path), Ok(bytes));
    }

    pub(crate) fn fail(&self, path: &str, error: FetchError) {
        self.files.lock().unwrap().insert(url(path), Err(error));
    }

    pub(crate) fn down(&self) {
        let mut files = self.files.lock().unwrap();
        for result in files.values_mut() {
            *result = Err(FetchError::Failed(
                "could not download it: offline".to_owned(),
            ));
        }
    }

    pub(crate) fn asked(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Transport for Fake {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, FetchError> {
        self.requests.lock().unwrap().push(url.to_owned());
        match self.files.lock().unwrap().get(url) {
            Some(Ok(body)) if body.len() > max_bytes => Err(FetchError::Failed(
                "the download is larger than expected".to_owned(),
            )),
            Some(Ok(body)) => Ok(body.clone()),
            Some(Err(err)) => Err(err.clone()),
            None => Err(FetchError::NotFound),
        }
    }

    fn latest_release_page(&self) -> Result<String, String> {
        Err("not asked".to_owned())
    }
}

pub(crate) struct World {
    pub(crate) tmp: tempfile::TempDir,
    pub(crate) net: Arc<Fake>,
    pub(crate) store: ExtensionStore,
    pub(crate) stopped: Arc<Mutex<Vec<String>>>,
}

pub(crate) fn platform() -> String {
    "linux-x86_64".to_owned()
}

impl World {
    pub(crate) fn new() -> Self {
        Self::on("0.1.0")
    }

    pub(crate) fn on(sevak_version: &str) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let dirs = StoreDirs {
            config_dir: root.join("config"),
            workflows: root.join("config").join("workflows"),
            plugins: root.join("config").join("plugins"),
            plugin_data: root.join("data").join("plugins"),
            workflow_data: root.join("data").join("workflows"),
            state: root.join("data"),
            approvals: root.join("data").join("script-plugin-approvals.json"),
        };
        let net = Arc::new(Fake::default());
        let transport: SharedTransport = net.clone();
        let store = ExtensionStore::with(
            dirs,
            transport,
            Pin::new("v1.2.3"),
            platform(),
            sevak_version.to_owned(),
        );
        let stopped = Arc::new(Mutex::new(Vec::new()));
        let seen = stopped.clone();
        store.set_quiesce(Arc::new(move |id: &str| {
            seen.lock().unwrap().push(id.to_owned());
        }));
        Self {
            tmp,
            net,
            store,
            stopped,
        }
    }

    pub(crate) fn plugins(&self) -> std::path::PathBuf {
        self.store.dirs().plugins.clone()
    }

    pub(crate) fn workflows(&self) -> std::path::PathBuf {
        self.store.dirs().workflows.clone()
    }

    /// Serves `entries` (and `themes`) as the gallery's lists.
    pub(crate) fn publish(&self, entries: Vec<Value>, themes: Vec<Value>) {
        self.net.put(
            "gallery/index.json",
            json!({"format": 2, "name": "Test", "entries": entries})
                .to_string()
                .into_bytes(),
        );
        self.net.put(
            "gallery/themes.json",
            json!({"version": 2, "themes": themes})
                .to_string()
                .into_bytes(),
        );
    }
}

pub(crate) fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut out);
    for (name, data) in files {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap();
    out.into_inner()
}

pub(crate) const WORKFLOW: &str = r#"
    name = "Docs"
    [[node]]
    id = "k"
    type = "keyword"
    keyword = "docs"
    [[node]]
    id = "o"
    type = "open_url"
    url = "https://example.com/?q={query}"
    [[connection]]
    from = "k"
    to = "o"
"#;

/// A gallery entry for a workflow package whose bytes are `zip`, served at
/// `gallery/packages/<id>.zip`.
pub(crate) fn workflow_entry(world: &World, id: &str, version: &str, zip: &[u8]) -> Value {
    world
        .net
        .put(&format!("gallery/packages/{id}.zip"), zip.to_vec());
    json!({
        "id": id, "kind": "workflow", "name": id.to_uppercase(), "description": "d",
        "author": "Sevak", "version": version,
        "source": format!("gallery/packages/{id}.zip"), "sha256": sha256_hex(zip)
    })
}

pub(crate) fn workflow_zip(id: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let manifest = format!("{id}/workflow.toml");
    let mut files: Vec<(String, Vec<u8>)> = vec![(manifest, WORKFLOW.as_bytes().to_vec())];
    for (name, data) in extra {
        files.push((format!("{id}/{name}"), data.to_vec()));
    }
    let borrowed: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    zip_of(&borrowed)
}

// ---- native fixtures ---------------------------------------------------------

pub(crate) fn native_manifest(version: &str, permissions: &str) -> String {
    format!(
        "protocol = 1\nid = \"script:tool\"\nkeyword = \"tool\"\nname = \"Tool\"\n\
         description = \"A tool.\"\n[extension]\nversion = \"{version}\"\nauthor = \"Ada\"\n\
         license = \"MIT\"\nmin_sevak = \"0.1.0\"\npermissions = [{permissions}]\n\
         [extension.binaries]\nlinux-x86_64 = \"bin/tool-linux\"\n\
         macos-aarch64 = \"bin/tool-macos\"\nwindows-x86_64 = \"bin/tool.exe\"\n"
    )
}

/// Builds the packages of a native extension, serves them and returns the
/// index entry for them.
pub(crate) fn native_entry(world: &World, version: &str, linux_bytes: &[u8]) -> Value {
    let manifest = native_manifest(version, "\"network\"");
    let mut builder = Builder::new(&manifest, "tool").unwrap();
    builder
        .add_binary("linux-x86_64", linux_bytes.to_vec())
        .unwrap();
    builder
        .add_binary("macos-aarch64", b"macho build".to_vec())
        .unwrap();
    builder
        .add_binary("windows-x86_64", b"mz build".to_vec())
        .unwrap();
    let mut platforms = serde_json::Map::new();
    for name in ["linux-x86_64", "macos-aarch64", "windows-x86_64"] {
        let bytes = builder.build(Some(name)).unwrap();
        let path = format!("gallery/extensions/tool/tool-{version}-{name}.sevakext");
        world.net.put(&path, bytes.clone());
        platforms.insert(
            name.to_owned(),
            json!({"source": path, "sha256": sha256_hex(&bytes)}),
        );
    }
    json!({
        "id": "tool", "kind": "native", "name": "Tool", "description": "A tool.",
        "author": "Ada", "version": version, "license": "MIT", "min_sevak": "0.1.0",
        "permissions": ["network"], "platforms": platforms,
        "repository": "https://github.com/example/tool"
    })
}

pub(crate) fn theme_fixture() -> (Vec<u8>, Value) {
    let nord =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gallery/themes/Nord.toml"))
            .unwrap();
    let entry = json!({
        "id": "nord", "name": "Nord", "author": "x", "description": "d", "mode": "dark",
        "url": "gallery/themes/Nord.toml", "sha256": sha256_hex(&nord)
    });
    (nord, entry)
}
