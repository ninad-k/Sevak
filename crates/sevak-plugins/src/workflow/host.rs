//! Finding workflows on disk and deciding which of them run.
//!
//! The [`WorkflowHost`] sits next to the script plugin host: the shell asks it
//! for the plugins of every workflow that may run and adds them to the
//! engine's. Like script plugins, a workflow that can run code needs the
//! user's yes first, remembered per workflow and per *what it can run* (see
//! [`approval_key`]); the answers share the script plugins' approval file.
//!
//! A workflow that only opens links, copies or shows text needs no approval:
//! it cannot do more than the standard result actions can.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sevak_core::{Config, HotkeyBinding, Plugin};
use sevak_platform::PlatformProvider;
use sha2::{Digest, Sha256};

use super::exec::{Ctx, OutputSink, Runtime};
use super::model::{slug, valid_folder_name, Accepts, Node, NodeKind, Workflow, FAMILY, FILE};
use super::plugins::{run_id, FilterPlugin, KeywordPlugin, TriggersPlugin};
use super::templates;
use super::validate::{error_summary, Problem};
use crate::keywords::{workflow_key, KeywordOwners, KeywordUse, OwnerKind};
use crate::script::{relative_inside, ApprovalStore};

/// A workflow folder that loaded and is valid.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The folder's name (its id in `--trigger`).
    pub folder: String,
    pub dir: PathBuf,
    pub workflow: Workflow,
    /// Warnings only: errors make the folder [`Scanned::Broken`].
    pub warnings: Vec<Problem>,
    /// What the user has to allow, or `None` when the workflow runs nothing.
    pub approval_key: Option<String>,
    /// The user has allowed exactly this (always true without a key).
    pub approved: bool,
}

impl Candidate {
    /// The id approvals and the plugin family use.
    pub fn id(&self) -> String {
        format!("{FAMILY}:{}", self.folder)
    }

    /// What the approval dialog tells the user the workflow can do.
    pub fn describe(&self) -> String {
        describe(&self.workflow, &self.dir)
    }
}

/// One folder found under the workflows directory.
#[derive(Debug, Clone)]
pub enum Scanned {
    Workflow(Box<Candidate>),
    /// A folder with a `workflow.toml` that cannot be used.
    Broken {
        folder: String,
        error: String,
    },
}

/// A workflow as the settings list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub folder: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub enabled: bool,
    /// It can run code, so it waits for the user's yes.
    pub needs_approval: bool,
    pub approved: bool,
    pub keywords: Vec<String>,
    pub nodes: usize,
    /// Everything suspicious about it, keyword clashes included.
    pub warnings: usize,
    /// The keyword clashes among those warnings, as sentences.
    pub keyword_warnings: Vec<String>,
    /// Why it cannot load.
    pub error: Option<String>,
}

/// A workflow opened for editing.
#[derive(Debug, Clone, Serialize)]
pub struct Loaded {
    pub folder: String,
    pub workflow: Workflow,
    pub problems: Vec<Problem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Saved {
    pub folder: String,
    pub problems: Vec<Problem>,
}

pub struct WorkflowHost {
    /// `<config dir>/workflows`
    dir: PathBuf,
    /// Parent of the per-workflow data folders.
    data_dir: PathBuf,
    approvals: ApprovalStore,
    /// Workflows the user said "not now" to, until Sevak restarts.
    declined: Mutex<HashSet<String>>,
    sink: Arc<dyn OutputSink>,
    /// The workflows that may run now, by folder (for `--trigger`).
    runtimes: Mutex<HashMap<String, Arc<Runtime>>>,
}

impl std::fmt::Debug for WorkflowHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowHost")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl WorkflowHost {
    pub fn new(
        dir: PathBuf,
        data_dir: PathBuf,
        approvals_file: PathBuf,
        sink: Arc<dyn OutputSink>,
    ) -> Self {
        Self {
            dir,
            data_dir,
            approvals: ApprovalStore::new(approvals_file),
            declined: Mutex::new(HashSet::new()),
            sink,
            runtimes: Mutex::new(HashMap::new()),
        }
    }

    /// The folder workflows live in.
    pub fn workflows_dir(&self) -> &Path {
        &self.dir
    }

    fn folder_dir(&self, folder: &str) -> Result<PathBuf, String> {
        if valid_folder_name(folder) {
            Ok(self.dir.join(folder))
        } else {
            Err("that is not a valid workflow folder name".to_owned())
        }
    }

    /// Lists the workflow folders, sorted by name. A missing workflows
    /// directory is simply "no workflows"; folders without a `workflow.toml`
    /// are not workflows.
    pub fn scan(&self) -> Vec<Scanned> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut folders: Vec<(String, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().join(FILE).is_file())
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path(),
                )
            })
            .filter(|(name, _)| !name.starts_with('.'))
            .collect();
        folders.sort();

        folders
            .into_iter()
            .map(|(folder, dir)| self.read_folder(folder, dir))
            .collect()
    }

    fn read_folder(&self, folder: String, dir: PathBuf) -> Scanned {
        let broken = |error: String| {
            tracing::warn!(workflow = folder, "not loading this workflow: {error}");
            Scanned::Broken {
                folder: folder.clone(),
                error,
            }
        };
        if !valid_folder_name(&folder) {
            return broken(
                "the folder name may only use letters, digits, spaces, - _ .".to_owned(),
            );
        }
        let text = match fs::read_to_string(dir.join(FILE)) {
            Ok(text) => text,
            Err(err) => return broken(format!("cannot read {FILE}: {err}")),
        };
        let workflow = match Workflow::from_toml(&text) {
            Ok(workflow) => workflow,
            Err(error) => return broken(error),
        };
        let problems = workflow.validate();
        if let Some(error) = error_summary(&problems) {
            return broken(error);
        }
        let warnings: Vec<Problem> = problems.into_iter().filter(|p| !p.is_error()).collect();
        let key = approval_key(&workflow, &dir);
        let approved = key.as_deref().is_none_or(|key| {
            self.approvals
                .is_approved(&format!("{FAMILY}:{folder}"), key)
        });
        Scanned::Workflow(Box::new(Candidate {
            folder,
            dir,
            workflow,
            warnings,
            approval_key: key,
            approved,
        }))
    }

    fn enabled(config: &Config, candidate: &Candidate) -> bool {
        candidate.workflow.enabled
            && config.plugins.is_enabled(FAMILY)
            && config.plugins.is_enabled(&candidate.id())
    }

    /// The plugins of every workflow that may run: valid, enabled, and either
    /// harmless or approved. Also refreshes what `--trigger` can start.
    pub fn plugins(
        &self,
        config: &Config,
        platform: &Arc<dyn PlatformProvider>,
    ) -> Vec<Arc<dyn Plugin>> {
        let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();
        let mut runtimes = HashMap::new();
        for scanned in self.scan() {
            let Scanned::Workflow(candidate) = scanned else {
                continue;
            };
            let candidate = *candidate;
            let id = candidate.id();
            if !Self::enabled(config, &candidate) {
                tracing::info!(workflow = id, "workflow skipped (disabled)");
                continue;
            }
            if !candidate.approved {
                tracing::info!(workflow = id, "workflow skipped (not approved yet)");
                continue;
            }
            let runtime = Runtime::new(
                &candidate.folder,
                candidate.dir.clone(),
                self.data_dir.join(&candidate.folder),
                candidate.workflow.clone(),
                Arc::clone(platform),
                Arc::clone(&self.sink),
                config,
            );
            for node in &candidate.workflow.nodes {
                let plugin: Option<Arc<dyn Plugin>> = match &node.kind {
                    NodeKind::Keyword { .. } => KeywordPlugin::new(runtime.clone(), node)
                        .map(|plugin| Arc::new(plugin) as Arc<dyn Plugin>),
                    NodeKind::ScriptFilter { .. } => {
                        FilterPlugin::new(runtime.clone(), node, Arc::clone(platform))
                            .map(|plugin| Arc::new(plugin) as Arc<dyn Plugin>)
                    }
                    _ => None,
                };
                if let Some(plugin) = plugin {
                    if config.plugins.is_enabled(plugin.id()) {
                        plugins.push(plugin);
                    }
                }
            }
            if let Some(plugin) = TriggersPlugin::new(runtime.clone()) {
                plugins.push(Arc::new(plugin));
            }
            runtimes.insert(candidate.folder.clone(), runtime);
        }
        let ids: Vec<&str> = plugins.iter().map(|p| p.id()).collect();
        tracing::info!(loaded = ?ids, "workflow plugins loaded");
        *self.runtimes.lock().unwrap_or_else(|p| p.into_inner()) = runtimes;
        plugins
    }

    /// The keywords of the enabled workflows (approved or not: they answer
    /// once allowed), for finding clashes.
    pub fn keyword_uses(&self, config: &Config) -> Vec<KeywordUse> {
        let mut uses = Vec::new();
        for scanned in self.scan() {
            let Scanned::Workflow(candidate) = scanned else {
                continue;
            };
            if !Self::enabled(config, &candidate) {
                continue;
            }
            let owner = format!(
                "workflow {}",
                display_name(&candidate.workflow, &candidate.folder)
            );
            for keyword in candidate.workflow.keywords() {
                uses.push(KeywordUse::new(
                    keyword,
                    OwnerKind::Workflow,
                    workflow_key(&candidate.folder),
                    owner.clone(),
                ));
            }
        }
        uses
    }

    /// The `[[hotkey]]`-style bindings for the hotkey triggers of workflows
    /// that may run. They are never written to the config file.
    pub fn hotkey_bindings(&self, config: &Config) -> Vec<HotkeyBinding> {
        let mut bindings = Vec::new();
        for scanned in self.scan() {
            let Scanned::Workflow(candidate) = scanned else {
                continue;
            };
            if !candidate.approved || !Self::enabled(config, &candidate) {
                continue;
            }
            for node in &candidate.workflow.nodes {
                if let NodeKind::Hotkey { key } = &node.kind {
                    bindings.push(HotkeyBinding {
                        key: key.trim().to_owned(),
                        query: None,
                        run: Some(run_id(&candidate.folder, &node.id)),
                    });
                }
            }
        }
        bindings
    }

    /// `sevak --trigger <folder>/<node> [text]`: starts an external trigger of
    /// a workflow that may run.
    pub fn trigger(&self, folder: &str, node: &str, arg: &str) -> Result<(), String> {
        let runtime = self
            .runtimes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(folder)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "No workflow \"{folder}\" is ready to run (is it installed, enabled and \
                     allowed?)."
                )
            })?;
        match runtime.workflow.node(node).map(|n| &n.kind) {
            Some(NodeKind::External {}) => runtime.start(node, Ctx::with_arg(arg)),
            Some(_) => Err(format!(
                "\"{node}\" in \"{folder}\" is not an external trigger."
            )),
            None => Err(format!("The workflow \"{folder}\" has no node \"{node}\".")),
        }
    }

    // ---- approval ----------------------------------------------------------

    /// Workflows waiting for the user's first yes: valid, enabled, able to run
    /// code, not approved and not declined since Sevak started.
    pub fn pending(&self, config: &Config) -> Vec<Candidate> {
        let declined = self.declined.lock().unwrap_or_else(|p| p.into_inner());
        self.scan()
            .into_iter()
            .filter_map(|scanned| match scanned {
                Scanned::Workflow(candidate) => Some(*candidate),
                Scanned::Broken { .. } => None,
            })
            .filter(|c| !c.approved && Self::enabled(config, c))
            .filter(|c| {
                !declined.contains(&format!(
                    "{} {}",
                    c.id(),
                    c.approval_key.clone().unwrap_or_default()
                ))
            })
            .collect()
    }

    /// Remembers that the user allowed `candidate` to run.
    pub fn approve(&self, candidate: &Candidate) -> Result<(), String> {
        let Some(key) = &candidate.approval_key else {
            return Ok(());
        };
        self.approvals
            .approve(&candidate.id(), key)
            .map_err(|err| format!("could not save the approval: {err}"))
    }

    /// Stops asking about `candidate` until Sevak restarts (or it changes).
    pub fn decline(&self, candidate: &Candidate) {
        self.declined
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(format!(
                "{} {}",
                candidate.id(),
                candidate.approval_key.clone().unwrap_or_default()
            ));
    }

    // ---- the settings page -------------------------------------------------

    /// Every workflow folder as a settings row, broken ones included.
    pub fn summaries(&self, config: &Config, owners: &KeywordOwners) -> Vec<Summary> {
        self.scan()
            .into_iter()
            .map(|scanned| match scanned {
                Scanned::Workflow(c) => {
                    let keywords: Vec<String> = c.workflow.keywords().map(str::to_owned).collect();
                    let keyword_warnings: Vec<String> = owners
                        .warnings_for(
                            Some(&workflow_key(&c.folder)),
                            keywords.iter().map(String::as_str),
                        )
                        .into_iter()
                        .map(|(_, message)| message)
                        .collect();
                    Summary {
                        enabled: Self::enabled(config, &c),
                        needs_approval: c.approval_key.is_some(),
                        approved: c.approved,
                        keywords,
                        nodes: c.workflow.nodes.len(),
                        warnings: c.warnings.len() + keyword_warnings.len(),
                        keyword_warnings,
                        name: display_name(&c.workflow, &c.folder),
                        description: c.workflow.description.clone(),
                        author: c.workflow.author.clone(),
                        version: c.workflow.version.clone(),
                        error: None,
                        folder: c.folder,
                    }
                }
                Scanned::Broken { folder, error } => Summary {
                    name: folder.clone(),
                    description: String::new(),
                    author: String::new(),
                    version: String::new(),
                    enabled: false,
                    needs_approval: false,
                    approved: false,
                    keywords: Vec::new(),
                    nodes: 0,
                    warnings: 0,
                    keyword_warnings: Vec::new(),
                    error: Some(error),
                    folder,
                },
            })
            .collect()
    }

    /// Opens `folder` for editing, whatever its problems: the builder shows
    /// them so they can be fixed.
    pub fn load(&self, folder: &str) -> Result<Loaded, String> {
        let dir = self.folder_dir(folder)?;
        let text = fs::read_to_string(dir.join(FILE))
            .map_err(|err| format!("cannot read the workflow: {err}"))?;
        let workflow = Workflow::from_toml(&text)?;
        let problems = workflow.validate();
        Ok(Loaded {
            folder: folder.to_owned(),
            workflow,
            problems,
        })
    }

    /// Checks `workflow` without saving it.
    pub fn check(&self, workflow: &Workflow) -> Vec<Problem> {
        workflow.validate()
    }

    /// Writes `workflow` to `folder` (a new folder is made from the workflow's
    /// name when `folder` is `None`). A workflow with errors is not written.
    pub fn save(&self, folder: Option<&str>, workflow: &Workflow) -> Result<Saved, String> {
        let problems = workflow.validate();
        if let Some(error) = error_summary(&problems) {
            return Err(format!("The workflow has problems: {error}"));
        }
        let text = workflow.to_toml()?;
        let folder = match folder {
            Some(folder) => {
                self.folder_dir(folder)?;
                folder.to_owned()
            }
            None => self.unused_folder(&workflow.name),
        };
        let dir = self.dir.join(&folder);
        fs::create_dir_all(&dir).map_err(|err| format!("could not create the folder: {err}"))?;
        write_atomic(&dir.join(FILE), &text)
            .map_err(|err| format!("could not save the workflow: {err}"))?;
        Ok(Saved { folder, problems })
    }

    /// A folder name for a workflow called `name` that does not exist yet.
    fn unused_folder(&self, name: &str) -> String {
        let base = slug(name);
        let mut candidate = base.clone();
        let mut n = 1;
        while self.dir.join(&candidate).exists() {
            n += 1;
            candidate = format!("{base}-{n}");
        }
        candidate
    }

    /// Creates a new workflow folder from template `template_id`.
    pub fn create_from_template(&self, template_id: &str) -> Result<Saved, String> {
        let template = templates::find(template_id).ok_or_else(|| "unknown template".to_owned())?;
        let saved = self.save(None, &template.workflow)?;
        let dir = self.dir.join(&saved.folder);
        for (path, contents) in &template.files {
            let target = relative_inside(&dir, path)
                .ok_or_else(|| "a template file has a bad path".to_owned())?;
            write_atomic(&target, contents)
                .map_err(|err| format!("could not write {path}: {err}"))?;
        }
        Ok(saved)
    }

    /// Deletes a workflow folder (the user confirmed in the settings window).
    pub fn delete(&self, folder: &str) -> Result<(), String> {
        let dir = self.folder_dir(folder)?;
        if !dir.join(FILE).is_file() {
            return Err("that folder is not a workflow".to_owned());
        }
        fs::remove_dir_all(&dir).map_err(|err| format!("could not delete the workflow: {err}"))
    }

    /// Switches a workflow on or off by rewriting its `enabled` field.
    pub fn set_enabled(&self, folder: &str, enabled: bool) -> Result<(), String> {
        let loaded = self.load(folder)?;
        let mut workflow = loaded.workflow;
        if workflow.enabled == enabled {
            return Ok(());
        }
        workflow.enabled = enabled;
        let dir = self.folder_dir(folder)?;
        write_atomic(&dir.join(FILE), &workflow.to_toml()?)
            .map_err(|err| format!("could not save the workflow: {err}"))
    }

    /// Stops every workflow run in progress (Sevak is quitting or reloading).
    pub fn shutdown(&self) {
        for runtime in self
            .runtimes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
        {
            runtime.shutdown();
        }
    }
}

fn display_name(workflow: &Workflow, folder: &str) -> String {
    let name = workflow.name.trim();
    if name.is_empty() {
        folder.to_owned()
    } else {
        name.to_owned()
    }
}

/// Writes `text` next to `path` and renames it into place.
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let written = fs::write(&temp, text).and_then(|()| fs::rename(&temp, path));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// What the user approves: everything that decides *what runs*, as a SHA-256.
/// It covers the nodes that run code or commands (their whole configuration),
/// the connections (they decide what each receives and in what order), the
/// workflow's variables (exported to the scripts) and the contents of every
/// script file those nodes name. It leaves out layout, titles and the nodes
/// that cannot run code, so moving a box or retitling a node does not ask
/// again, while editing a script does.
///
/// `None` for a workflow that runs nothing.
pub fn approval_key(workflow: &Workflow, dir: &Path) -> Option<String> {
    if !workflow.needs_approval() {
        return None;
    }
    let mut hasher = Sha256::new();
    let mut feed = |label: &str, bytes: &[u8]| {
        hasher.update(label.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    };
    for node in workflow.nodes.iter().filter(|n| n.kind.needs_approval()) {
        feed("node", node.id.as_bytes());
        feed(
            "kind",
            serde_json::to_string(&node.kind)
                .unwrap_or_default()
                .as_bytes(),
        );
        for file in script_files(node) {
            match relative_inside(dir, &file).map(fs::read) {
                Some(Ok(bytes)) => feed("file", &bytes),
                _ => feed("missing", file.as_bytes()),
            }
        }
    }
    for conn in &workflow.connections {
        feed("from", conn.from.as_bytes());
        feed("port", conn.port.as_bytes());
        feed("to", conn.to.as_bytes());
    }
    for (name, value) in &workflow.variables {
        feed("var", name.as_bytes());
        feed("value", value.as_bytes());
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Some(format!("sha256:{hex}"))
}

/// The files in the workflow folder a script node starts: its `script`, and
/// any argument of its command that is a relative path (`["python3", "main.py"]`).
fn script_files(node: &Node) -> Vec<String> {
    let (command, script) = match &node.kind {
        NodeKind::RunScript {
            command, script, ..
        }
        | NodeKind::ScriptFilter {
            command, script, ..
        } => (command.as_slice(), script.as_deref()),
        // The file an open-file node starts, when it ships in the folder.
        NodeKind::OpenFile { path } => {
            return if relative_inside(Path::new(""), path).is_some() {
                vec![path.clone()]
            } else {
                Vec::new()
            };
        }
        _ => return Vec::new(),
    };
    let mut files: Vec<String> = script.iter().map(|s| (*s).to_owned()).collect();
    files.extend(
        command
            .iter()
            .filter(|arg| relative_inside(Path::new(""), arg).is_some())
            .cloned(),
    );
    files
}

/// A plain-language summary of what a workflow can do, for the approval dialog.
pub fn describe(workflow: &Workflow, dir: &Path) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut starts: BTreeSet<String> = BTreeSet::new();
    let mut sees_selection = false;
    for node in &workflow.nodes {
        match &node.kind {
            NodeKind::Keyword { keyword, .. } => {
                starts.insert(format!("typing the keyword {}", keyword.trim()));
            }
            NodeKind::ScriptFilter { keyword, .. } => {
                starts.insert(format!("typing the keyword {}", keyword.trim()));
            }
            NodeKind::Hotkey { key } => {
                starts.insert(format!("the shortcut {}", key.trim()));
            }
            NodeKind::Selection { accepts } => {
                sees_selection = true;
                let kinds: Vec<&str> = accepts
                    .iter()
                    .map(|kind| match kind {
                        Accepts::Text => "text",
                        Accepts::Url => "links",
                        Accepts::File => "files",
                    })
                    .collect();
                starts.insert(format!(
                    "Universal Actions on selected {}",
                    kinds.join(", ")
                ));
            }
            NodeKind::External {} => {
                starts.insert(format!("sevak --trigger {}", node.id));
            }
            _ => {}
        }
    }
    if !starts.is_empty() {
        lines.push(format!(
            "Starts from: {}",
            starts.into_iter().collect::<Vec<_>>().join("; ")
        ));
    }
    if sees_selection {
        lines.push(
            "It will receive the text, links or files you select in other apps when you use \
             Universal Actions on it."
                .to_owned(),
        );
    }
    let mut runs = Vec::new();
    for node in workflow.nodes.iter().filter(|n| n.kind.needs_approval()) {
        runs.push(match &node.kind {
            NodeKind::RunScript {
                command, script, ..
            }
            | NodeKind::ScriptFilter {
                command, script, ..
            } => match script {
                Some(script) if command.is_empty() => format!("runs the script {script}"),
                _ => format!("runs {}", command.join(" ")),
            },
            NodeKind::LaunchApp { app, .. } => format!("starts the application {app}"),
            NodeKind::OpenFile { path } => format!("opens the file or folder {path}"),
            NodeKind::SystemCommand { command } => {
                format!("runs the system command {command}")
            }
            NodeKind::TerminalCommand { command } => {
                format!("runs in a terminal: {command}")
            }
            _ => continue,
        });
    }
    if !runs.is_empty() {
        lines.push(format!("It {}.", runs.join("; and ")));
    }
    lines.push(format!("Folder: {}", dir.display()));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;
    use crate::workflow::exec::NoSink;

    struct Fixture {
        root: tempfile::TempDir,
        host: WorkflowHost,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let host = host_at(&root);
        Fixture { root, host }
    }

    fn host_at(root: &tempfile::TempDir) -> WorkflowHost {
        WorkflowHost::new(
            root.path().join("config").join("workflows"),
            root.path().join("data").join("workflows"),
            root.path().join("data").join("approvals.json"),
            Arc::new(NoSink),
        )
    }

    fn platform() -> Arc<dyn PlatformProvider> {
        MockPlatform::empty()
    }

    fn write(f: &Fixture, folder: &str, toml: &str) -> PathBuf {
        let dir = f.root.path().join("config").join("workflows").join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(FILE), toml).unwrap();
        dir
    }

    const HARMLESS: &str = r#"
        name = "Harmless"
        [[node]]
        id = "k"
        type = "keyword"
        keyword = "harm"
        [[node]]
        id = "o"
        type = "open_url"
        url = "https://example.com/{query}"
        [[connection]]
        from = "k"
        to = "o"
    "#;

    fn scripted(command: &str) -> String {
        format!(
            r#"
            name = "Scripted"
            [[node]]
            id = "k"
            type = "keyword"
            keyword = "scr"
            x = 10
            [[node]]
            id = "run"
            type = "run_script"
            command = {command}
            [[connection]]
            from = "k"
            to = "run"
            "#
        )
    }

    fn ids(plugins: &[Arc<dyn Plugin>]) -> Vec<String> {
        plugins.iter().map(|p| p.id().to_owned()).collect()
    }

    #[test]
    fn a_missing_directory_means_no_workflows() {
        let f = fixture();
        assert!(f.host.scan().is_empty());
        assert!(f.host.plugins(&Config::default(), &platform()).is_empty());
        assert!(f
            .host
            .summaries(&Config::default(), &KeywordOwners::default())
            .is_empty());
        assert!(f.host.hotkey_bindings(&Config::default()).is_empty());
    }

    #[test]
    fn workflows_without_scripts_run_without_approval() {
        let f = fixture();
        write(&f, "harmless", HARMLESS);
        let plugins = f.host.plugins(&Config::default(), &platform());
        assert_eq!(ids(&plugins), ["workflow:harmless:k"]);
        assert_eq!(plugins[0].keyword(), Some("harm"));
        assert!(f.host.pending(&Config::default()).is_empty());
    }

    #[test]
    fn workflows_that_run_code_wait_for_approval() {
        let f = fixture();
        write(&f, "scripted", &scripted(r#"["python3", "main.py"]"#));
        let config = Config::default();
        assert!(f.host.plugins(&config, &platform()).is_empty());
        let pending = f.host.pending(&config);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id(), "workflow:scripted");
        assert!(pending[0].describe().contains("runs python3 main.py"));

        f.host.approve(&pending[0]).unwrap();
        assert_eq!(
            ids(&f.host.plugins(&config, &platform())),
            ["workflow:scripted:k"]
        );
        assert!(f.host.pending(&config).is_empty());
    }

    #[test]
    fn changing_what_runs_asks_again_but_moving_boxes_does_not() {
        let f = fixture();
        let dir = write(&f, "scripted", &scripted(r#"["python3", "main.py"]"#));
        fs::write(dir.join("main.py"), "print(1)").unwrap();
        let config = Config::default();
        f.host.approve(&f.host.pending(&config)[0]).unwrap();
        assert!(f.host.pending(&config).is_empty());

        // Dragging the trigger elsewhere, retitling, renaming: still approved.
        let moved = scripted(r#"["python3", "main.py"]"#)
            .replace("x = 10", "x = 400\n title = \"New title\"")
            .replace("Scripted", "Renamed");
        write(&f, "scripted", &moved);
        assert!(f.host.pending(&config).is_empty(), "layout is not behavior");

        // A different command asks again...
        write(&f, "scripted", &scripted(r#"["python3", "other.py"]"#));
        assert_eq!(f.host.pending(&config).len(), 1);
        // ...and so does the same command with a changed script file.
        write(&f, "scripted", &scripted(r#"["python3", "main.py"]"#));
        assert!(f.host.pending(&config).is_empty());
        fs::write(dir.join("main.py"), "print('evil')").unwrap();
        assert_eq!(f.host.pending(&config).len(), 1);
        assert!(f.host.plugins(&config, &platform()).is_empty());
    }

    #[test]
    fn opening_a_file_needs_approval_and_a_bundled_file_is_hashed() {
        let opening = |path: &str| {
            format!(
                r#"
                name = "Opens"
                [[node]]
                id = "k"
                type = "keyword"
                keyword = "opn"
                [[node]]
                id = "f"
                type = "open_file"
                path = "{path}"
                [[connection]]
                from = "k"
                to = "f"
                "#
            )
        };
        let f = fixture();
        let dir = write(&f, "opens", &opening("payload.bat"));
        fs::write(dir.join("payload.bat"), "echo 1").unwrap();
        let config = Config::default();
        // Not loaded until the user has allowed it, and the dialog says what opens.
        assert!(f.host.plugins(&config, &platform()).is_empty());
        let pending = f.host.pending(&config);
        assert_eq!(pending.len(), 1);
        assert!(pending[0]
            .describe()
            .contains("opens the file or folder payload.bat"));

        f.host.approve(&pending[0]).unwrap();
        assert!(f.host.pending(&config).is_empty());
        // Replacing the file the node starts asks again.
        fs::write(dir.join("payload.bat"), "echo evil").unwrap();
        assert_eq!(f.host.pending(&config).len(), 1);
    }

    #[test]
    fn rewiring_or_new_variables_ask_again() {
        let f = fixture();
        write(&f, "scripted", &scripted(r#"["tool"]"#));
        let config = Config::default();
        f.host.approve(&f.host.pending(&config)[0]).unwrap();
        let with_variable = format!("{}\n[variables]\nX = \"1\"\n", scripted(r#"["tool"]"#));
        write(&f, "scripted", &with_variable);
        assert_eq!(f.host.pending(&config).len(), 1);
    }

    #[test]
    fn approval_keys_ignore_everything_that_cannot_run_code() {
        let dir = Path::new("wf");
        let a = Workflow::from_toml(&scripted(r#"["tool"]"#)).unwrap();
        let mut b = a.clone();
        b.name = "Another name".into();
        b.nodes[0].x = 999.0;
        b.nodes[1].title = "Run it".into();
        assert_eq!(approval_key(&a, dir), approval_key(&b, dir));
        assert!(approval_key(&a, dir).unwrap().starts_with("sha256:"));
        // A node that cannot run code does not count...
        let mut c = a.clone();
        c.nodes.push(Node {
            id: "extra".into(),
            title: String::new(),
            x: 0.0,
            y: 0.0,
            kind: NodeKind::Copy { text: "x".into() },
        });
        assert_eq!(approval_key(&a, dir), approval_key(&c, dir));
        // ...a missing script is not the same as an empty one.
        let mut d = a.clone();
        if let NodeKind::RunScript {
            command, script, ..
        } = &mut d.nodes[1].kind
        {
            command.clear();
            *script = Some("run.py".into());
        }
        assert_ne!(approval_key(&a, dir), approval_key(&d, dir));
        // Harmless workflows have no key at all.
        assert_eq!(
            approval_key(&Workflow::from_toml(HARMLESS).unwrap(), dir),
            None
        );
    }

    #[test]
    fn declining_silences_the_prompt_for_this_session_only() {
        let f = fixture();
        write(&f, "scripted", &scripted(r#"["tool"]"#));
        let config = Config::default();
        let pending = f.host.pending(&config).remove(0);
        f.host.decline(&pending);
        assert!(f.host.pending(&config).is_empty());
        // A changed workflow is asked about again...
        write(&f, "scripted", &scripted(r#"["tool", "--other"]"#));
        assert_eq!(f.host.pending(&config).len(), 1);
        // ...and so is the same one after a restart.
        write(&f, "scripted", &scripted(r#"["tool"]"#));
        assert_eq!(host_at(&f.root).pending(&config).len(), 1);
    }

    #[test]
    fn disabled_workflows_are_neither_loaded_nor_asked_about() {
        let f = fixture();
        write(&f, "harmless", HARMLESS);
        write(&f, "scripted", &scripted(r#"["tool"]"#));
        let mut config = Config::default();
        config.plugins.disabled = vec!["workflow:scripted".into()];
        assert!(f.host.pending(&config).is_empty());
        assert_eq!(
            ids(&f.host.plugins(&config, &platform())),
            ["workflow:harmless:k"]
        );
        config.plugins.disabled = vec!["workflow".into()];
        assert!(f.host.plugins(&config, &platform()).is_empty());
        // The workflow's own switch does the same.
        let off = HARMLESS.replace(
            "name = \"Harmless\"",
            "name = \"Harmless\"\nenabled = false",
        );
        write(&f, "harmless", &off);
        assert!(f.host.plugins(&Config::default(), &platform()).is_empty());
        let rows = f
            .host
            .summaries(&Config::default(), &KeywordOwners::default());
        assert!(
            !rows
                .iter()
                .find(|r| r.folder == "harmless")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn broken_workflows_are_reported_not_loaded() {
        let f = fixture();
        write(&f, "a-good", HARMLESS);
        write(&f, "b-syntax", "this is [ not toml");
        write(
            &f,
            "c-loop",
            &HARMLESS.replace(
                "[[connection]]",
                "[[connection]]\n from = \"o\"\n to = \"o\"\n [[connection]]",
            ),
        );
        write(&f, "d-future", "format = 9\n");
        fs::create_dir_all(f.root.path().join("config/workflows/not-a-workflow")).unwrap();
        write(&f, ".hidden", HARMLESS);
        let scanned = f.host.scan();
        assert_eq!(scanned.len(), 4);
        assert!(matches!(&scanned[0], Scanned::Workflow(c) if c.folder == "a-good"));
        assert!(matches!(&scanned[1], Scanned::Broken { folder, .. } if folder == "b-syntax"));
        assert!(
            matches!(&scanned[2], Scanned::Broken { error, .. } if error.contains("itself")),
            "{:?}",
            scanned[2]
        );
        assert!(matches!(&scanned[3], Scanned::Broken { error, .. } if error.contains("format 9")));
        assert_eq!(
            ids(&f.host.plugins(&Config::default(), &platform())),
            ["workflow:a-good:k"]
        );
        let rows = f
            .host
            .summaries(&Config::default(), &KeywordOwners::default());
        assert_eq!(rows.len(), 4);
        assert!(rows[1].error.is_some() && !rows[1].enabled);
        assert_eq!(rows[0].keywords, ["harm"]);
        assert_eq!(rows[0].name, "Harmless");
    }

    #[test]
    fn hotkey_triggers_become_bindings_for_workflows_that_may_run() {
        let f = fixture();
        let text = r#"
            name = "Keys"
            [[node]]
            id = "hk"
            type = "hotkey"
            key = " Ctrl+Alt+J "
            [[node]]
            id = "n"
            type = "notification"
            body = "hi"
            [[connection]]
            from = "hk"
            to = "n"
        "#;
        write(&f, "keys", text);
        let bindings = f.host.hotkey_bindings(&Config::default());
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].key, "Ctrl+Alt+J");
        assert_eq!(bindings[0].run.as_deref(), Some("workflow:keys:run:hk"));
        assert_eq!(bindings[0].query, None);
        // The engine can resolve what the binding names.
        let plugins = f.host.plugins(&Config::default(), &platform());
        let resolved = plugins
            .iter()
            .find_map(|p| p.resolve("workflow:keys:run:hk"))
            .unwrap();
        assert_eq!(resolved.plugin_id, "workflow:keys");

        // A hotkey of a workflow awaiting approval stays unbound.
        let guarded = text.replace(
            "[[connection]]",
            "[[node]]\nid = \"t\"\ntype = \"terminal_command\"\ncommand = \"ls\"\n[[connection]]",
        );
        write(&f, "keys", &guarded);
        assert!(f.host.hotkey_bindings(&Config::default()).is_empty());
    }

    #[test]
    fn external_triggers_start_only_external_nodes_of_runnable_workflows() {
        let f = fixture();
        let text = r#"
            name = "Ext"
            [[node]]
            id = "go"
            type = "external"
            [[node]]
            id = "kw"
            type = "keyword"
            keyword = "ext"
            [[node]]
            id = "c"
            type = "copy"
            text = "from outside: {query}"
            [[connection]]
            from = "go"
            to = "c"
            [[connection]]
            from = "kw"
            to = "c"
        "#;
        write(&f, "ext", text);
        let mock = MockPlatform::empty();
        let platform: Arc<dyn PlatformProvider> = mock.clone();
        // Before the plugins are built nothing can be triggered.
        assert!(f.host.trigger("ext", "go", "x").is_err());
        let _plugins = f.host.plugins(&Config::default(), &platform);
        assert!(f
            .host
            .trigger("ext", "kw", "x")
            .unwrap_err()
            .contains("not an external"));
        assert!(f
            .host
            .trigger("ext", "nope", "x")
            .unwrap_err()
            .contains("no node"));
        assert!(f
            .host
            .trigger("other", "go", "x")
            .unwrap_err()
            .contains("ready to run"));
        f.host.trigger("ext", "go", "hello").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while mock.clipboard.lock().unwrap().is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "the trigger never ran"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(*mock.clipboard.lock().unwrap(), ["from outside: hello"]);
    }

    #[test]
    fn an_unapproved_workflow_cannot_be_triggered_from_outside() {
        let f = fixture();
        let text = r#"
            name = "Ext"
            [[node]]
            id = "go"
            type = "external"
            [[node]]
            id = "t"
            type = "terminal_command"
            command = "rm -rf ~"
            [[connection]]
            from = "go"
            to = "t"
        "#;
        write(&f, "ext", text);
        let mock = MockPlatform::empty();
        let platform: Arc<dyn PlatformProvider> = mock.clone();
        let _ = f.host.plugins(&Config::default(), &platform);
        assert!(f.host.trigger("ext", "go", "").is_err());
        assert!(mock.terminal_runs.lock().unwrap().is_empty());
        // After the user allows it, it runs.
        f.host
            .approve(&f.host.pending(&Config::default())[0])
            .unwrap();
        let _ = f.host.plugins(&Config::default(), &platform);
        f.host.trigger("ext", "go", "").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while mock.terminal_runs.lock().unwrap().is_empty() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn saving_creates_folders_and_refuses_broken_workflows() {
        let f = fixture();
        let workflow = Workflow::from_toml(HARMLESS).unwrap();
        let saved = f.host.save(None, &workflow).unwrap();
        assert_eq!(saved.folder, "harmless");
        let again = f.host.save(None, &workflow).unwrap();
        assert_eq!(again.folder, "harmless-2");
        // Saving into an existing folder replaces its file.
        let mut edited = workflow.clone();
        edited.description = "edited".into();
        f.host.save(Some("harmless"), &edited).unwrap();
        assert_eq!(
            f.host.load("harmless").unwrap().workflow.description,
            "edited"
        );

        let mut broken = workflow.clone();
        broken.connections.push(crate::workflow::model::Connection {
            from: "o".into(),
            port: "out".into(),
            to: "k".into(),
        });
        let err = f.host.save(Some("harmless"), &broken).unwrap_err();
        assert!(err.contains("problems"), "{err}");
        assert_eq!(
            f.host.load("harmless").unwrap().workflow.description,
            "edited"
        );

        // Folder names cannot climb out.
        for bad in ["../x", "a/b", "..", ""] {
            assert!(f.host.save(Some(bad), &workflow).is_err(), "{bad:?}");
            assert!(f.host.load(bad).is_err(), "{bad:?}");
            assert!(f.host.delete(bad).is_err(), "{bad:?}");
        }
        assert!(f.host.load("missing").is_err());
    }

    #[test]
    fn templates_create_working_folders_and_deleting_removes_them() {
        let f = fixture();
        let saved = f.host.create_from_template("script-filter").unwrap();
        assert_eq!(saved.folder, "script-filter");
        let dir = f.root.path().join("config/workflows/script-filter");
        assert!(dir.join("filter.py").is_file());
        let rows = f
            .host
            .summaries(&Config::default(), &KeywordOwners::default());
        assert!(rows[0].needs_approval && !rows[0].approved);
        assert!(f.host.create_from_template("nope").is_err());

        f.host.delete("script-filter").unwrap();
        assert!(!dir.exists());
        assert!(f.host.delete("script-filter").is_err());
        // Only workflow folders can be deleted this way.
        fs::create_dir_all(f.root.path().join("config/workflows/precious")).unwrap();
        assert!(f.host.delete("precious").is_err());
    }

    #[test]
    fn the_enabled_switch_rewrites_the_file() {
        let f = fixture();
        write(&f, "harmless", HARMLESS);
        f.host.set_enabled("harmless", false).unwrap();
        assert!(!f.host.load("harmless").unwrap().workflow.enabled);
        f.host.set_enabled("harmless", true).unwrap();
        assert!(f.host.load("harmless").unwrap().workflow.enabled);
    }

    #[test]
    fn the_description_says_what_a_workflow_can_do() {
        let text = r#"
            name = "Everything"
            [[node]]
            id = "k"
            type = "keyword"
            keyword = "all"
            [[node]]
            id = "s"
            type = "selection"
            accepts = ["text", "file"]
            [[node]]
            id = "h"
            type = "hotkey"
            key = "Ctrl+Alt+E"
            [[node]]
            id = "x"
            type = "external"
            [[node]]
            id = "run"
            type = "run_script"
            script = "go.py"
            [[node]]
            id = "t"
            type = "terminal_command"
            command = "git status"
            [[node]]
            id = "off"
            type = "system_command"
            command = "lock"
            [[connection]]
            from = "k"
            to = "run"
        "#;
        let workflow = Workflow::from_toml(text).unwrap();
        let shown = describe(&workflow, Path::new("/cfg/workflows/everything"));
        assert!(shown.contains("typing the keyword all"), "{shown}");
        assert!(shown.contains("the shortcut Ctrl+Alt+E"), "{shown}");
        assert!(
            shown.contains("Universal Actions on selected text, files"),
            "{shown}"
        );
        assert!(shown.contains("sevak --trigger x"), "{shown}");
        assert!(
            shown.contains("receive the text, links or files you select"),
            "{shown}"
        );
        assert!(shown.contains("runs the script go.py"), "{shown}");
        assert!(shown.contains("runs in a terminal: git status"), "{shown}");
        assert!(shown.contains("runs the system command lock"), "{shown}");
        assert!(shown.contains("/cfg/workflows/everything"), "{shown}");
    }

    #[test]
    fn script_files_are_found_in_commands_too() {
        let node = |command: &[&str], script: Option<&str>| Node {
            id: "n".into(),
            title: String::new(),
            x: 0.0,
            y: 0.0,
            kind: NodeKind::RunScript {
                command: command.iter().map(|s| (*s).to_owned()).collect(),
                script: script.map(str::to_owned),
                args: Vec::new(),
                stdin: None,
                env: Default::default(),
                timeout_ms: None,
                log_stderr: false,
            },
        };
        assert_eq!(script_files(&node(&[], Some("a.py"))), ["a.py"]);
        // Programs on PATH, flags and absolute paths are not files of ours.
        assert_eq!(
            script_files(&node(
                &["python3", "-u", "lib/main.py", "/abs/x", "../up"],
                None
            )),
            ["python3", "-u", "lib/main.py"]
        );
    }
}
