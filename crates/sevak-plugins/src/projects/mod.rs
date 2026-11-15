//! Recent projects: `code <name>` reopens a folder or workspace you had open in
//! VS Code, VS Code Insiders, VSCodium, Cursor or a JetBrains IDE, in that
//! editor.
//!
//! | Key | Action |
//! |---|---|
//! | Enter | open the project in the editor it came from |
//! | `Ctrl+Enter` | open the folder in the file manager |
//! | `Alt+Enter` | copy the path |
//!
//! An empty query lists the projects most recently opened first; each row
//! names its editor. A project whose editor cannot be found (moved, or a
//! portable install) opens its folder instead, and the row says so.
//!
//! # Privacy
//!
//! The editors' own recent lists are read (see [`sources`]) on a background
//! thread by [`Plugin::refresh`] and kept in memory only; the log has counts,
//! never paths. Nothing is sent anywhere.

pub mod editors;
pub mod sources;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use sevak_core::config::ProjectsConfig;
use sevak_core::model::score;
use sevak_core::{
    Action, FuzzyQuery, IconSource, LaunchTarget, Modifier, Plugin, PluginError, PluginResult,
    ResultItem,
};
use sevak_platform::PlatformProvider;

use crate::actions::execute_action;
use editors::{jetbrains_roots, Env, JetBrainsProduct, Launcher, Os, JETBRAINS_ID, VSCODE_FLAVORS};
use sources::{jetbrains_files, modified_ms, now_ms, read_jetbrains, read_vscode, Kind, Recent};

const ID: &str = "projects";
/// Rows returned for one query.
const MAX_ROWS: usize = 30;
/// Projects kept from every editor together.
const MAX_PROJECTS: usize = 5_000;
/// A name that starts with the input beats a looser match.
const PREFIX_BONUS: f64 = 60.0;
/// Weight of a match on the path (not the name).
const PATH_WEIGHT: f64 = 0.6;
const PAYLOAD_NOTHING: &str = "nothing";

/// One project row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// `<editor key>:<path>`: unique, and stable for hotkeys.
    pub key: String,
    pub name: String,
    pub path: PathBuf,
    pub kind: Kind,
    /// Display name of the editor ("VS Code", "PyCharm").
    pub editor: String,
    pub launcher: Option<Launcher>,
    /// Milliseconds since 1970; higher is more recent.
    pub opened_ms: i64,
}

#[derive(Default)]
struct Snapshot {
    projects: Vec<Project>,
    loaded: bool,
}

/// The `projects` plugin.
pub struct ProjectsPlugin {
    config: ProjectsConfig,
    platform: Arc<dyn PlatformProvider>,
    env: Env,
    snapshot: RwLock<Arc<Snapshot>>,
}

impl ProjectsPlugin {
    pub fn new(config: &ProjectsConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        Self::with_env(config, platform, Env::current())
    }

    /// Looks for the editors in `env` instead of this computer's folders.
    pub fn with_env(
        config: &ProjectsConfig,
        platform: Arc<dyn PlatformProvider>,
        env: Env,
    ) -> Self {
        Self {
            config: config.clone(),
            platform,
            env,
            snapshot: RwLock::new(Arc::new(Snapshot::default())),
        }
    }

    fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot
            .read()
            .map(|s| Arc::clone(&s))
            .unwrap_or_default()
    }

    /// Reads every editor's list. Slow; background thread.
    fn load(&self) -> Vec<Project> {
        let env = &self.env;
        let mut projects = Vec::new();
        for flavor in VSCODE_FLAVORS.iter().filter(|f| self.config.reads(f.id)) {
            let Some(root) = flavor.data_root(env) else {
                continue;
            };
            let (recents, saved_ms) = read_vscode(&root, env.os);
            if recents.is_empty() {
                continue;
            }
            let launcher = flavor.launcher(env);
            // The list is newest first; when it was saved dates its top entry.
            let base = saved_ms.unwrap_or_else(now_ms);
            for (i, recent) in recents.into_iter().enumerate() {
                projects.push(project(
                    flavor.id,
                    flavor.name,
                    &launcher,
                    recent,
                    base - i as i64,
                ));
            }
        }
        if self.config.reads(JETBRAINS_ID) {
            for root in jetbrains_roots(env) {
                for (folder, file) in jetbrains_files(&root) {
                    let Some(product) = JetBrainsProduct::for_folder(&folder) else {
                        continue;
                    };
                    let launcher = product.launcher(env);
                    let base = modified_ms(&file).unwrap_or_else(now_ms);
                    let recents = read_jetbrains(&root, &file, env.home.as_deref(), env.os);
                    for (i, recent) in recents.into_iter().enumerate() {
                        let opened = recent.opened_ms.unwrap_or(base - i as i64);
                        projects.push(project(
                            &product.key(),
                            product.name,
                            &launcher,
                            recent,
                            opened,
                        ));
                    }
                }
            }
        }
        index(projects, env.os)
    }

    fn status(&self, key: &str, title: &str, subtitle: &str) -> ResultItem {
        ResultItem::new(
            ID,
            key,
            title,
            Action::Custom {
                payload: PAYLOAD_NOTHING.to_owned(),
            },
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin("folder"))
        .with_score(score::KEYWORD)
    }

    fn row(&self, project: &Project) -> ResultItem {
        let shown = project.path.display().to_string();
        let what = match project.kind {
            Kind::Folder => project.editor.clone(),
            Kind::Workspace => format!("{} workspace", project.editor),
        };
        let folder = match project.kind {
            Kind::Folder => project.path.clone(),
            Kind::Workspace => project
                .path
                .parent()
                .map_or_else(|| project.path.clone(), Path::to_path_buf),
        };
        let (action, subtitle) = match &project.launcher {
            Some(launcher) => {
                let mut args = launcher.args.clone();
                args.push(project.path.to_string_lossy().into_owned());
                (
                    Action::Launch {
                        target: LaunchTarget::Executable {
                            path: launcher.program.clone(),
                            args,
                            working_dir: None,
                        },
                    },
                    format!("{what} · {shown}"),
                )
            }
            None => (
                Action::OpenPath {
                    path: folder.clone(),
                },
                format!("{what} (not found; Enter opens the folder) · {shown}"),
            ),
        };
        ResultItem::new(ID, &project.key, &project.name, action)
            .with_subtitle(subtitle)
            .with_icon(IconSource::builtin("folder"))
            .with_secondary(
                "Open folder",
                Some(Modifier::Ctrl),
                Action::OpenPath { path: folder },
            )
            .with_secondary(
                "Copy path",
                Some(Modifier::Alt),
                Action::CopyText { text: shown },
            )
    }

    fn matches(&self, projects: &[Project], input: &str) -> Vec<ResultItem> {
        if input.is_empty() {
            return projects
                .iter()
                .take(MAX_ROWS)
                .enumerate()
                .map(|(i, p)| self.row(p).with_score(score::KEYWORD - i as f64))
                .collect();
        }
        let lower = input.to_lowercase();
        let mut names = FuzzyQuery::new(input);
        let mut paths = FuzzyQuery::for_paths(input);
        let mut scored: Vec<(f64, usize, &Project)> = projects
            .iter()
            .enumerate()
            .filter_map(|(i, project)| {
                let name = names.score(&project.name).map(|s| {
                    let bonus = if project.name.to_lowercase().starts_with(&lower) {
                        PREFIX_BONUS
                    } else {
                        0.0
                    };
                    f64::from(s) + bonus
                });
                let path = paths
                    .score(&project.path.to_string_lossy())
                    .map(|s| f64::from(s) * PATH_WEIGHT);
                let best = match (name, path) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                }?;
                Some((best, i, project))
            })
            .collect();
        // Equal scores keep the most recent first.
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        scored
            .into_iter()
            .take(MAX_ROWS)
            .map(|(s, _, p)| self.row(p).with_score(score::KEYWORD + s))
            .collect()
    }
}

fn project(
    editor_key: &str,
    editor: &str,
    launcher: &Option<Launcher>,
    recent: Recent,
    opened_ms: i64,
) -> Project {
    let stem = match recent.kind {
        Kind::Folder => recent.path.file_name(),
        Kind::Workspace => recent.path.file_stem(),
    };
    let name = stem
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .or(recent.title.clone())
        .unwrap_or_else(|| recent.path.display().to_string());
    Project {
        key: format!("{editor_key}:{}", recent.path.display()),
        name,
        path: recent.path,
        kind: recent.kind,
        editor: editor.to_owned(),
        launcher: launcher.clone(),
        opened_ms,
    }
}

/// Newest first, one row per editor and path, only projects that still exist
/// on this computer (paths on other computers are never looked at).
fn index(mut projects: Vec<Project>, os: Os) -> Vec<Project> {
    projects.sort_by(|a, b| b.opened_ms.cmp(&a.opened_ms).then(a.key.cmp(&b.key)));
    let mut seen = HashSet::new();
    projects.retain(|p| {
        let key = match os {
            Os::Windows | Os::MacOs => p.key.to_lowercase(),
            Os::Linux => p.key.clone(),
        };
        let remote = p.path.to_string_lossy().starts_with("\\\\")
            || p.path.to_string_lossy().starts_with("//");
        !remote && seen.insert(key) && p.path.exists()
    });
    projects.truncate(MAX_PROJECTS);
    projects
}

impl Plugin for ProjectsPlugin {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "Recent projects"
    }

    fn description(&self) -> &str {
        "Type `code` to reopen a folder or workspace from VS Code, VS Code Insiders, VSCodium, Cursor or a JetBrains IDE in that editor."
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.config.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let snapshot = self.snapshot();
        if !snapshot.loaded {
            return vec![self.status(
                "loading",
                "Reading recent projects…",
                "Try again in a moment",
            )];
        }
        if snapshot.projects.is_empty() {
            return vec![self.status(
                "empty",
                "No recent projects found",
                "Open a folder in VS Code, Cursor, VSCodium or a JetBrains IDE, then choose Reload index",
            )];
        }
        self.matches(&snapshot.projects, input.trim())
    }

    fn resolve(&self, id: &str) -> Option<ResultItem> {
        let key = id.strip_prefix("projects:")?;
        let snapshot = self.snapshot();
        let project = snapshot.projects.iter().find(|p| p.key == key)?;
        Some(self.row(project).with_score(score::KEYWORD))
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        match &item.action {
            Action::Custom { payload } if payload == PAYLOAD_NOTHING => Ok(()),
            Action::Custom { .. } => Err(PluginError::Unsupported(item.id.clone())),
            action => execute_action(self.platform.as_ref(), action),
        }
    }

    fn refresh(&self) -> PluginResult<()> {
        let projects = self.load();
        tracing::info!(projects = projects.len(), "loaded recent projects");
        if let Ok(mut slot) = self.snapshot.write() {
            *slot = Arc::new(Snapshot {
                projects,
                loaded: true,
            });
        }
        Ok(())
    }

    fn index_size(&self) -> Option<usize> {
        Some(self.snapshot().projects.len())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_util::MockPlatform;
    use editors::tests::env_in;

    fn uri(path: &Path) -> String {
        let text = path.to_string_lossy().replace('\\', "/");
        let text = text.replace(' ', "%20");
        if text.starts_with('/') {
            format!("file://{text}")
        } else {
            format!("file:///{text}")
        }
    }

    fn os() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        env: Env,
    }

    /// A VS Code with three recent entries (one deleted since), Cursor with
    /// one, and GoLand with two, laid out for this OS's conventions.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let env = env_in(&root, os());
        let config = env.config_root().unwrap();
        let projects = root.join("projects");
        for name in ["sevak", "web app", "go-tool", "cursor-play"] {
            fs::create_dir_all(projects.join(name)).unwrap();
        }
        fs::write(projects.join("team.code-workspace"), "{}").unwrap();

        let code = config.join("Code").join("User").join("globalStorage");
        fs::create_dir_all(&code).unwrap();
        fs::write(
            code.join("storage.json"),
            format!(
                r#"{{"openedPathsList":{{"entries":[{{"folderUri":"{}"}},{{"workspace":{{"id":"1","configPath":"{}"}}}},{{"folderUri":"{}"}},{{"folderUri":"{}"}}]}}}}"#,
                uri(&projects.join("sevak")),
                uri(&projects.join("team.code-workspace")),
                uri(&projects.join("deleted")),
                uri(&projects.join("web app")),
            ),
        )
        .unwrap();
        let cursor = config.join("Cursor").join("User").join("globalStorage");
        fs::create_dir_all(&cursor).unwrap();
        fs::write(
            cursor.join("storage.json"),
            format!(
                r#"{{"openedPathsList":{{"entries":[{{"folderUri":"{}"}}]}}}}"#,
                uri(&projects.join("cursor-play"))
            ),
        )
        .unwrap();

        let goland = config
            .join("JetBrains")
            .join("GoLand2024.1")
            .join("options");
        fs::create_dir_all(&goland).unwrap();
        let go_tool = projects
            .join("go-tool")
            .to_string_lossy()
            .replace('\\', "/");
        let sevak = projects.join("sevak").to_string_lossy().replace('\\', "/");
        fs::write(
            goland.join("recentProjects.xml"),
            format!(
                r#"<application><component name="RecentProjectsManager"><option name="additionalInfo"><map>
<entry key="{go_tool}"><value><RecentProjectMetaInfo><option name="activationTimestamp" value="4102444800000"/></RecentProjectMetaInfo></value></entry>
<entry key="{sevak}"><value><RecentProjectMetaInfo><option name="activationTimestamp" value="1000"/></RecentProjectMetaInfo></value></entry>
</map></option></component></application>"#
            ),
        )
        .unwrap();
        Fixture {
            _dir: dir,
            root,
            env,
        }
    }

    fn loaded(fixture: &Fixture, config: ProjectsConfig) -> (ProjectsPlugin, Arc<MockPlatform>) {
        let platform = MockPlatform::empty();
        let plugin = ProjectsPlugin::with_env(&config, platform.clone(), fixture.env.clone());
        plugin.refresh().unwrap();
        (plugin, platform)
    }

    fn titles(rows: &[ResultItem]) -> Vec<&str> {
        rows.iter().map(|r| r.title.as_str()).collect()
    }

    #[test]
    fn metadata() {
        let fixture = fixture();
        let (plugin, _) = loaded(&fixture, ProjectsConfig::default());
        assert_eq!(plugin.id(), "projects");
        assert_eq!(plugin.keyword(), Some("code"));
        assert!(!plugin.global());
    }

    #[test]
    fn lists_every_editors_projects_newest_first_and_skips_missing_ones() {
        let fixture = fixture();
        let (plugin, _) = loaded(&fixture, ProjectsConfig::default());
        let rows = plugin.query("");
        // GoLand's go-tool has a timestamp in 2100; its sevak one in 1970.
        assert_eq!(rows[0].title, "go-tool");
        assert_eq!(rows.last().unwrap().title, "sevak");
        assert!(rows.last().unwrap().subtitle.starts_with("GoLand"));
        let mut names = titles(&rows);
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "cursor-play",
                "go-tool",
                "sevak",
                "sevak",
                "team",
                "web app"
            ]
        );
        assert!(rows.iter().all(|r| !r.title.contains("deleted")));
        let team = rows.iter().find(|r| r.title == "team").unwrap();
        assert!(
            team.subtitle.starts_with("VS Code workspace"),
            "{}",
            team.subtitle
        );
        assert_eq!(plugin.index_size(), Some(6));
    }

    #[test]
    fn finds_projects_by_name_or_path() {
        let fixture = fixture();
        let (plugin, _) = loaded(&fixture, ProjectsConfig::default());
        assert_eq!(plugin.query("web")[0].title, "web app");
        assert_eq!(plugin.query("cursor")[0].title, "cursor-play");
        let sevak = plugin.query("sevak");
        assert_eq!(titles(&sevak)[..2], ["sevak", "sevak"]);
        assert!(plugin.query("zzzzqq").is_empty());
    }

    #[test]
    fn enter_opens_the_editor_or_else_the_folder() {
        let fixture = fixture();
        let (plugin, platform) = loaded(&fixture, ProjectsConfig::default());
        // No editor is installed in the fixture: Enter opens the folder.
        let web = plugin.query("web app").remove(0);
        assert!(web.subtitle.contains("not found"));
        plugin.execute(&web).unwrap();
        assert_eq!(
            *platform.opened_paths.lock().unwrap(),
            [fixture.root.join("projects").join("web app")]
        );

        // Install VS Code where this OS keeps it, and reload.
        let program = match fixture.env.os {
            Os::Windows => fixture
                .root
                .join("local/Programs/Microsoft VS Code/Code.exe"),
            _ => fixture.root.join("bin/code"),
        };
        fs::create_dir_all(program.parent().unwrap()).unwrap();
        fs::write(&program, "").unwrap();
        plugin.refresh().unwrap();
        let web = plugin.query("web app").remove(0);
        plugin.execute(&web).unwrap();
        let path = fixture.root.join("projects").join("web app");
        assert_eq!(
            *platform.launched.lock().unwrap(),
            [LaunchTarget::Executable {
                path: program,
                args: vec![path.to_string_lossy().into_owned()],
                working_dir: None,
            }]
        );

        // Ctrl opens the folder, Alt copies the path.
        for modifier in [Modifier::Ctrl, Modifier::Alt] {
            let index = web
                .secondary
                .iter()
                .position(|s| s.modifier == Some(modifier))
                .unwrap();
            plugin
                .execute(&web.secondary_as_primary(index).unwrap())
                .unwrap();
        }
        assert_eq!(platform.opened_paths.lock().unwrap().len(), 2);
        assert_eq!(
            *platform.clipboard.lock().unwrap(),
            [path.display().to_string()]
        );
    }

    #[test]
    fn the_editor_list_limits_what_is_read() {
        let fixture = fixture();
        let config = ProjectsConfig {
            editors: vec!["jetbrains".into()],
            ..ProjectsConfig::default()
        };
        let (plugin, _) = loaded(&fixture, config);
        assert_eq!(titles(&plugin.query("")), ["go-tool", "sevak"]);
        let config = ProjectsConfig {
            editors: vec!["cursor".into()],
            ..ProjectsConfig::default()
        };
        let (plugin, _) = loaded(&fixture, config);
        assert_eq!(titles(&plugin.query("")), ["cursor-play"]);
    }

    #[test]
    fn status_rows_and_resolve() {
        let fixture = fixture();
        let platform = MockPlatform::empty();
        let empty = ProjectsPlugin::with_env(
            &ProjectsConfig::default(),
            platform.clone(),
            env_in(&fixture.root.join("nowhere"), os()),
        );
        assert_eq!(empty.query("")[0].title, "Reading recent projects…");
        empty.refresh().unwrap();
        let row = empty.query("x").remove(0);
        assert_eq!(row.title, "No recent projects found");
        empty.execute(&row).unwrap();

        let (plugin, _) = loaded(&fixture, ProjectsConfig::default());
        let first = plugin.query("").remove(0);
        let resolved = plugin.resolve(&first.id).unwrap();
        assert_eq!(resolved.title, first.title);
        assert!(plugin.resolve("projects:nope").is_none());
    }
}
