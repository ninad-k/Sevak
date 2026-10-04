//! Finding script plugins on disk and deciding which of them run.
//!
//! The [`ScriptPluginHost`] sits beside the [`PluginRegistry`]: descriptor
//! factories are plain function pointers with no way to learn the config
//! directory, so the shell asks the host for its plugins and appends them to
//! the built-in ones. The host applies `[plugins] disabled` itself (the family
//! id `script` disables all script plugins, an instance id one), so settings
//! and the config file treat them like any other plugin.
//!
//! [`PluginRegistry`]: crate::PluginRegistry

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use sevak_core::{Config, Plugin};
use sevak_platform::PlatformProvider;

use super::approvals::{script_approval_key, ApprovalStore};
use super::manifest::{Manifest, ID_PREFIX, MANIFEST_FILE};
use super::plugin::ScriptPlugin;
use super::runner::Spec;
use crate::keywords::{KeywordOwners, KeywordUse, OwnerKind};
use crate::PluginInfo;

/// The family id: `[plugins] disabled = ["script"]` turns every script plugin off.
pub const FAMILY: &str = "script";

/// What "not now" remembers: this plugin in this state, so a change asks again.
fn decline_key(candidate: &Candidate) -> String {
    format!("{} {}", candidate.manifest.id, candidate.key)
}

/// A valid plugin folder.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The folder's name (not its path).
    pub folder: String,
    pub dir: PathBuf,
    pub manifest: Manifest,
    /// What the approval is bound to: a hash over the folder's location, the
    /// manifest and the script files (see `script_approval_key`).
    pub key: String,
    /// The user has allowed exactly this (these files, in this folder).
    pub approved: bool,
    /// The user allowed a plugin with this id before, but not this: its files
    /// changed, it moved, or the allowance dates from before approvals were
    /// bound to contents. The Allow dialog says so.
    pub reviewed_before: bool,
}

impl Candidate {
    /// The text of the Allow dialog (see `dialog::script_prompt`): the folder,
    /// what runs, what it is given, with everything the author wrote made safe
    /// to show.
    pub fn prompt(&self) -> String {
        super::dialog::script_prompt(
            &self.manifest,
            &self.folder,
            &self.dir.display().to_string(),
            &self.key,
            self.reviewed_before,
        )
    }
}

/// One folder found under the plugins directory.
#[derive(Debug, Clone)]
pub enum Scanned {
    Plugin(Box<Candidate>),
    /// A folder with a `plugin.toml` that cannot be used.
    Broken {
        folder: String,
        error: String,
    },
}

#[derive(Debug)]
pub struct ScriptPluginHost {
    /// `<config dir>/plugins`
    dir: PathBuf,
    /// Parent of the per-plugin data folders.
    data_dir: PathBuf,
    approvals: ApprovalStore,
    /// Plugins the user said "not now" to, until Sevak restarts.
    declined: Mutex<HashSet<String>>,
}

impl ScriptPluginHost {
    pub fn new(dir: PathBuf, data_dir: PathBuf, approvals_file: PathBuf) -> Self {
        Self {
            dir,
            data_dir,
            approvals: ApprovalStore::new(approvals_file),
            declined: Mutex::new(HashSet::new()),
        }
    }

    /// The folder users drop plugins into.
    pub fn plugins_dir(&self) -> &std::path::Path {
        &self.dir
    }

    /// Lists the plugin folders, sorted by name. A missing plugins directory is
    /// simply "no plugins"; folders without a `plugin.toml` are not plugins.
    pub fn scan(&self) -> Vec<Scanned> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut folders: Vec<(String, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().join(MANIFEST_FILE).is_file())
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path(),
                )
            })
            .filter(|(name, _)| !name.starts_with('.'))
            .collect();
        folders.sort();

        // Read every folder first: when two claim the same id, the one the
        // user has already allowed keeps it, so a look-alike folder cannot
        // take the id (and the approval) away from the original.
        type Read = Result<(Manifest, String), String>;
        let loaded: Vec<(String, PathBuf, Read)> = folders
            .into_iter()
            .map(|(folder, dir)| {
                let result = script_approval_key(&dir);
                (folder, dir, result)
            })
            .collect();
        let approvals = self.approvals.snapshot();
        let approved_now =
            |manifest: &Manifest, key: &str| approvals.is_approved(&manifest.id, key);
        let mut claimed: HashMap<String, String> = HashMap::new();
        for want_approved in [true, false] {
            for (folder, _, result) in &loaded {
                if let Ok((manifest, key)) = result {
                    if approved_now(manifest, key) == want_approved {
                        claimed
                            .entry(manifest.id.clone())
                            .or_insert_with(|| folder.clone());
                    }
                }
            }
        }

        let mut scanned = Vec::new();
        for (folder, dir, result) in loaded {
            let (manifest, key) = match result {
                Ok(loaded) => loaded,
                Err(error) => {
                    tracing::warn!(plugin = folder, "not loading this script plugin: {error}");
                    scanned.push(Scanned::Broken { folder, error });
                    continue;
                }
            };
            if let Some(first) = claimed.get(&manifest.id).filter(|first| **first != folder) {
                let error = format!(
                    "the id {} is already used by the folder {first}",
                    manifest.id
                );
                tracing::warn!(plugin = folder, "not loading this script plugin: {error}");
                scanned.push(Scanned::Broken { folder, error });
                continue;
            }
            for warning in &manifest.warnings {
                tracing::warn!(plugin = manifest.id, "{warning}");
            }
            let approved = approved_now(&manifest, &key);
            let reviewed_before = !approved && approvals.has_record(&manifest.id);
            scanned.push(Scanned::Plugin(Box::new(Candidate {
                folder,
                dir,
                manifest,
                key,
                approved,
                reviewed_before,
            })));
        }
        scanned
    }

    fn enabled(config: &Config, id: &str) -> bool {
        config.plugins.is_enabled(FAMILY) && config.plugins.is_enabled(id)
    }

    /// The plugins to run: valid, approved and not disabled in the config.
    pub fn plugins(
        &self,
        config: &Config,
        platform: &Arc<dyn PlatformProvider>,
    ) -> Vec<Arc<dyn Plugin>> {
        let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();
        for scanned in self.scan() {
            let Scanned::Plugin(candidate) = scanned else {
                continue;
            };
            let candidate = *candidate;
            let id = &candidate.manifest.id;
            if !Self::enabled(config, id) {
                tracing::info!(plugin = id, "script plugin skipped (disabled in config)");
                continue;
            }
            if !candidate.approved {
                tracing::info!(plugin = id, "script plugin skipped (not approved yet)");
                continue;
            }
            if let Err(err) = candidate.manifest.resolve_argv(&candidate.dir) {
                // Still loaded: the interpreter may be installed later.
                tracing::warn!(plugin = id, "this script plugin cannot start yet: {err}");
            }
            let spec = Spec {
                data_dir: self.data_dir.join(&candidate.folder),
                dir: candidate.dir,
                manifest: candidate.manifest,
                env: Vec::new(),
                expected_key: Some(candidate.key),
            };
            plugins.push(Arc::new(ScriptPlugin::new(spec, Arc::clone(platform))));
        }
        let ids: Vec<&str> = plugins.iter().map(|p| p.id()).collect();
        tracing::info!(loaded = ?ids, "script plugins loaded");
        plugins
    }

    /// Plugins waiting for the user's first yes: valid, enabled, not approved
    /// and not declined since Sevak started.
    pub fn pending(&self, config: &Config) -> Vec<Candidate> {
        let declined = self.declined.lock().unwrap_or_else(|p| p.into_inner());
        self.scan()
            .into_iter()
            .filter_map(|scanned| match scanned {
                Scanned::Plugin(c) => Some(*c),
                Scanned::Broken { .. } => None,
            })
            .filter(|c| !c.approved && Self::enabled(config, &c.manifest.id))
            .filter(|c| !declined.contains(&decline_key(c)))
            .collect()
    }

    /// Remembers that the user allowed `candidate` to run.
    pub fn approve(&self, candidate: &Candidate) -> Result<(), String> {
        self.approvals
            .approve(&candidate.manifest.id, &candidate.key)
            .map_err(|err| format!("could not save the approval: {err}"))
    }

    /// Stops asking about `candidate` until Sevak restarts (or its command changes).
    pub fn decline(&self, candidate: &Candidate) {
        self.declined
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(decline_key(candidate));
    }

    /// The keywords of the enabled script plugins (approved or not: they
    /// answer once allowed), for finding clashes.
    pub fn keyword_uses(&self, config: &Config) -> Vec<KeywordUse> {
        self.scan()
            .into_iter()
            .filter_map(|scanned| match scanned {
                Scanned::Plugin(c) if Self::enabled(config, &c.manifest.id) => {
                    Some(KeywordUse::new(
                        &c.manifest.keyword,
                        OwnerKind::ScriptPlugin,
                        c.manifest.id.clone(),
                        format!("script plugin {}", c.manifest.name),
                    ))
                }
                _ => None,
            })
            .collect()
    }

    /// Settings rows for every script plugin, including ones that are waiting
    /// for approval or cannot load (their description says why), and a warning
    /// when another plugin answers the same keyword (`owners`).
    pub fn catalog(&self, config: &Config, owners: &KeywordOwners) -> Vec<PluginInfo> {
        self.scan()
            .into_iter()
            .map(|scanned| match scanned {
                Scanned::Plugin(c) => {
                    let id = c.manifest.id.clone();
                    let mut description = c.manifest.description.clone();
                    if !c.approved {
                        description = format!(
                            "Waiting for your approval (restart Sevak or choose Reload index \
                             to be asked). {description}"
                        );
                    }
                    for (_, warning) in
                        owners.warnings_for(Some(&id), [c.manifest.keyword.as_str()])
                    {
                        description = format!("{description} {warning}");
                    }
                    PluginInfo {
                        enabled: c.approved && Self::enabled(config, &id),
                        id,
                        name: c.manifest.name.clone(),
                        description: description.trim().to_owned(),
                        keyword: Some(c.manifest.keyword.clone()),
                    }
                }
                Scanned::Broken { folder, error } => PluginInfo {
                    id: format!("{ID_PREFIX}{}", folder.to_lowercase().replace(' ', "-")),
                    name: folder,
                    description: format!("Not loaded: {error}"),
                    keyword: None,
                    enabled: false,
                },
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_util::MockPlatform;

    struct Fixture {
        _root: tempfile::TempDir,
        host: ScriptPluginHost,
        plugins: PathBuf,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("config").join("plugins");
        let host = ScriptPluginHost::new(
            plugins.clone(),
            root.path().join("data").join("plugins"),
            root.path().join("data").join("approvals.json"),
        );
        Fixture {
            _root: root,
            host,
            plugins,
        }
    }

    fn add(fixture: &Fixture, folder: &str, manifest: &str) {
        let dir = fixture.plugins.join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    }

    fn manifest(keyword: &str) -> String {
        format!("protocol = 1\nkeyword = \"{keyword}\"\ncommand = [\"prog\"]\n")
    }

    fn platform() -> Arc<dyn PlatformProvider> {
        MockPlatform::empty()
    }

    fn ids(plugins: &[Arc<dyn Plugin>]) -> Vec<String> {
        plugins.iter().map(|p| p.id().to_owned()).collect()
    }

    fn candidates(host: &ScriptPluginHost) -> Vec<Candidate> {
        host.scan()
            .into_iter()
            .filter_map(|s| match s {
                Scanned::Plugin(c) => Some(*c),
                Scanned::Broken { .. } => None,
            })
            .collect()
    }

    #[test]
    fn a_missing_directory_means_no_plugins() {
        let f = fixture();
        assert!(f.host.scan().is_empty());
        assert!(f.host.plugins(&Config::default(), &platform()).is_empty());
        assert!(f
            .host
            .catalog(&Config::default(), &KeywordOwners::default())
            .is_empty());
    }

    #[test]
    fn finds_folders_with_manifests_in_name_order() {
        let f = fixture();
        add(&f, "b-second", &manifest("b"));
        add(&f, "a-first", &manifest("a"));
        fs::create_dir_all(f.plugins.join("not-a-plugin")).unwrap();
        fs::write(f.plugins.join("loose-file.txt"), "x").unwrap();
        add(&f, ".hidden", &manifest("h"));
        let found: Vec<_> = candidates(&f.host)
            .iter()
            .map(|c| c.manifest.id.clone())
            .collect();
        assert_eq!(found, ["script:a-first", "script:b-second"]);
    }

    #[test]
    fn plugins_run_only_after_approval() {
        let f = fixture();
        add(&f, "hello", &manifest("hello"));
        let config = Config::default();
        assert!(f.host.plugins(&config, &platform()).is_empty());

        let pending = f.host.pending(&config);
        assert_eq!(pending.len(), 1);
        f.host.approve(&pending[0]).unwrap();

        let plugins = f.host.plugins(&config, &platform());
        assert_eq!(ids(&plugins), ["script:hello"]);
        assert_eq!(plugins[0].keyword(), Some("hello"));
        assert!(!plugins[0].global());
        assert!(f.host.pending(&config).is_empty());
    }

    #[test]
    fn changing_the_command_asks_again() {
        let f = fixture();
        add(&f, "hello", &manifest("hello"));
        let first = f.host.pending(&Config::default()).remove(0);
        f.host.approve(&first).unwrap();
        assert!(f.host.pending(&Config::default()).is_empty());

        add(
            &f,
            "hello",
            "protocol = 1\nkeyword = \"hello\"\ncommand = [\"something-else\"]\n",
        );
        assert_eq!(f.host.pending(&Config::default()).len(), 1);
        assert!(f.host.plugins(&Config::default(), &platform()).is_empty());
    }

    #[test]
    fn declining_silences_the_prompt_for_this_session_only() {
        let f = fixture();
        add(&f, "hello", &manifest("hello"));
        let config = Config::default();
        let pending = f.host.pending(&config).remove(0);
        f.host.decline(&pending);
        assert!(f.host.pending(&config).is_empty());
        // A new host (Sevak restarted) asks again.
        let again = ScriptPluginHost::new(
            f.plugins.clone(),
            f._root.path().join("data").join("plugins"),
            f._root.path().join("data").join("approvals.json"),
        );
        assert_eq!(again.pending(&config).len(), 1);
    }

    #[test]
    fn the_disabled_list_applies_to_instances_and_the_family() {
        let f = fixture();
        add(&f, "one", &manifest("one"));
        add(&f, "two", &manifest("two"));
        for c in candidates(&f.host) {
            f.host.approve(&c).unwrap();
        }
        let mut config = Config::default();
        assert_eq!(f.host.plugins(&config, &platform()).len(), 2);

        config.plugins.disabled = vec!["script:one".into()];
        assert_eq!(ids(&f.host.plugins(&config, &platform())), ["script:two"]);
        let rows = f.host.catalog(&config, &KeywordOwners::default());
        assert_eq!(
            rows.iter().map(|r| r.enabled).collect::<Vec<_>>(),
            [false, true]
        );

        config.plugins.disabled = vec!["script".into()];
        assert!(f.host.plugins(&config, &platform()).is_empty());
        // Disabled plugins are not asked about either.
        fs::remove_file(f._root.path().join("data").join("approvals.json")).unwrap();
        assert!(f.host.pending(&config).is_empty());
    }

    fn script_manifest(id: &str) -> String {
        format!(
            "protocol = 1\nid = \"{id}\"\nkeyword = \"k\"\ncommand = [\"python\", \"main.py\"]\n"
        )
    }

    fn add_script(f: &Fixture, folder: &str, id: &str, code: &str) {
        add(f, folder, &script_manifest(id));
        fs::write(f.plugins.join(folder).join("main.py"), code).unwrap();
    }

    fn allow_all(f: &Fixture) {
        for c in f.host.pending(&Config::default()) {
            f.host.approve(&c).unwrap();
        }
        assert!(f.host.pending(&Config::default()).is_empty());
    }

    #[test]
    fn an_unchanged_plugin_stays_approved_across_scans_and_restarts() {
        let f = fixture();
        add_script(&f, "hello", "script:hello", "print(1)");
        allow_all(&f);
        assert!(candidates(&f.host)[0].approved);
        let restarted = ScriptPluginHost::new(
            f.plugins.clone(),
            f._root.path().join("data").join("plugins"),
            f._root.path().join("data").join("approvals.json"),
        );
        assert!(restarted.pending(&Config::default()).is_empty());
        assert_eq!(restarted.plugins(&Config::default(), &platform()).len(), 1);
    }

    #[test]
    fn replacing_the_script_asks_again() {
        let f = fixture();
        add_script(&f, "hello", "script:hello", "print(1)");
        allow_all(&f);

        fs::write(f.plugins.join("hello").join("main.py"), "print(2)").unwrap();
        let pending = f.host.pending(&Config::default());
        assert_eq!(pending.len(), 1);
        assert!(
            pending[0].reviewed_before,
            "the dialog says it was allowed before"
        );
        assert!(f.host.plugins(&Config::default(), &platform()).is_empty());

        // Allowing the new contents makes them current; going back asks again.
        f.host.approve(&pending[0]).unwrap();
        assert!(f.host.pending(&Config::default()).is_empty());
        fs::write(f.plugins.join("hello").join("main.py"), "print(1)").unwrap();
        assert_eq!(f.host.pending(&Config::default()).len(), 1);
    }

    #[test]
    fn a_support_file_named_in_the_manifest_is_covered_too() {
        let f = fixture();
        add(
            &f,
            "hello",
            &format!(
                "{}files = [\"helper.py\"]\n",
                script_manifest("script:hello")
            ),
        );
        fs::write(f.plugins.join("hello").join("main.py"), "import helper").unwrap();
        fs::write(f.plugins.join("hello").join("helper.py"), "A = 1").unwrap();
        allow_all(&f);
        fs::write(f.plugins.join("hello").join("helper.py"), "A = 2").unwrap();
        assert_eq!(f.host.pending(&Config::default()).len(), 1);
    }

    #[test]
    fn a_second_folder_claiming_the_same_id_and_command_is_not_approved() {
        let f = fixture();
        add_script(&f, "b-original", "script:notes", "print(1)");
        allow_all(&f);

        // Sorts first, same id, same command, even the same script text.
        add_script(&f, "a-lookalike", "script:notes", "print(1)");
        let scanned = f.host.scan();
        let by_folder = |name: &str| {
            scanned
                .iter()
                .find(|s| match s {
                    Scanned::Plugin(c) => c.folder == name,
                    Scanned::Broken { folder, .. } => folder == name,
                })
                .unwrap()
        };
        assert!(
            matches!(by_folder("b-original"), Scanned::Plugin(c) if c.approved),
            "the allowed folder keeps the id and keeps running"
        );
        assert!(
            matches!(by_folder("a-lookalike"), Scanned::Broken { error, .. } if error.contains("already used"))
        );
        let ids = ids(&f.host.plugins(&Config::default(), &platform()));
        assert_eq!(ids, ["script:notes"]);

        // Without the original, the look-alike is just a new plugin to review.
        fs::remove_dir_all(f.plugins.join("b-original")).unwrap();
        let only = candidates(&f.host);
        assert_eq!(only.len(), 1);
        assert!(!only[0].approved);
        assert!(only[0].reviewed_before);
    }

    #[test]
    fn a_moved_folder_asks_again() {
        let f = fixture();
        add_script(&f, "old-place", "script:notes", "print(1)");
        allow_all(&f);
        fs::rename(f.plugins.join("old-place"), f.plugins.join("new-place")).unwrap();
        let pending = f.host.pending(&Config::default());
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].folder, "new-place");
    }

    #[test]
    fn an_approval_from_before_contents_were_bound_asks_once() {
        let f = fixture();
        add_script(&f, "hello", "script:hello", "print(1)");
        // What the previous version stored: the command line, no version field.
        let path = f._root.path().join("data").join("approvals.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"approved":{"script:hello":"Persistent/Sevak: python main.py"}}"#,
        )
        .unwrap();
        let pending = f.host.pending(&Config::default());
        assert_eq!(pending.len(), 1);
        assert!(pending[0].reviewed_before);
        f.host.approve(&pending[0]).unwrap();
        assert!(f.host.pending(&Config::default()).is_empty());
    }

    #[test]
    fn a_script_replaced_after_loading_is_not_started() {
        let f = fixture();
        add_script(&f, "hello", "script:hello", "print(1)");
        allow_all(&f);
        let candidate = candidates(&f.host).remove(0);
        let spec = Spec {
            data_dir: f._root.path().join("data").join("plugins").join("hello"),
            dir: candidate.dir.clone(),
            expected_key: Some(candidate.key.clone()),
            manifest: candidate.manifest,
            env: Vec::new(),
        };
        // `python` may not exist where the tests run; only the approval check matters.
        let check = |spec: &Spec| spec.command(None).err().unwrap_or_default();
        assert!(!check(&spec).contains("changed after"));
        fs::write(candidate.dir.join("main.py"), "print('other')").unwrap();
        assert!(check(&spec).contains("changed after"), "{}", check(&spec));
    }

    #[test]
    fn broken_and_duplicate_manifests_are_reported_not_loaded() {
        let f = fixture();
        add(&f, "a-good", &manifest("a"));
        add(&f, "b-broken", "protocol = 1\n");
        add(
            &f,
            "c-clash",
            "protocol = 1\nid = \"script:a-good\"\nkeyword = \"c\"\ncommand = [\"x\"]\n",
        );
        let scanned = f.host.scan();
        assert_eq!(scanned.len(), 3);
        assert!(matches!(&scanned[0], Scanned::Plugin(c) if c.manifest.id == "script:a-good"));
        assert!(matches!(&scanned[1], Scanned::Broken { error, .. } if error.contains("keyword")));
        assert!(
            matches!(&scanned[2], Scanned::Broken { error, .. } if error.contains("already used"))
        );

        let rows = f
            .host
            .catalog(&Config::default(), &KeywordOwners::default());
        assert_eq!(rows.len(), 3);
        assert!(rows[0].description.starts_with("Waiting for your approval"));
        assert!(!rows[1].enabled && rows[1].description.starts_with("Not loaded"));
        assert!(f.host.pending(&Config::default()).len() == 1);
    }
}
