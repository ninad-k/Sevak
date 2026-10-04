use std::fs;

use sevak_core::diagnostics::{Identity, OsInfo};
use sevak_core::{AppEntry, IconData, IconSource, LaunchTarget};
use sevak_platform::{PlatformError, Result as PlatformResult};

use super::*;

/// A platform with two apps and a fixed OS, so nothing depends on the
/// computer the tests run on.
struct FakePlatform;

impl PlatformProvider for FakePlatform {
    fn os_info(&self) -> OsInfo {
        OsInfo {
            name: "FakeOS".into(),
            version: "1.2".into(),
            build: Some("77".into()),
            kernel: None,
            arch: "x86_64".into(),
        }
    }
    fn list_applications(&self) -> PlatformResult<Vec<AppEntry>> {
        let app = |id: &str| AppEntry {
            id: id.into(),
            name: id.into(),
            description: None,
            keywords: Vec::new(),
            icon: None,
            target: LaunchTarget::PackagedApp {
                app_user_model_id: id.into(),
            },
        };
        Ok(vec![app("one"), app("two")])
    }
    fn launch(&self, _target: &LaunchTarget) -> PlatformResult<()> {
        Ok(())
    }
    fn load_icon(&self, _source: &IconSource, _size: u32) -> PlatformResult<IconData> {
        Err(PlatformError::Unsupported("icons in tests"))
    }
}

struct Fixture {
    root: tempfile::TempDir,
    env: Environment,
}

impl Fixture {
    fn new(config_toml: Option<&str>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_roots(root.path().join("cfg"), root.path().join("data"));
        fs::create_dir_all(&paths.config_dir).unwrap();
        fs::create_dir_all(&paths.log_dir).unwrap();
        if let Some(text) = config_toml {
            fs::write(&paths.config_file, text).unwrap();
        }
        let env = Environment {
            paths,
            config_overridden: false,
            data_overridden: true,
            platform: Arc::new(FakePlatform),
            display: DisplayServer::Windows,
            now_unix: 1_791_115_200,
            exe: Some(root.path().join("Sevak").join("sevak.exe")),
            install_env: InstallEnv::default(),
            desktop: None,
            webview: Ok("130.0.1".into()),
            managed_by: None,
        };
        Self { root, env }
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.root.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn redactor(&self) -> Redactor {
        Redactor::new(&Identity {
            home_dirs: vec![self.root.path().display().to_string()],
            user_names: vec!["tester".into()],
            host_names: vec!["testbox".into()],
        })
    }

    fn report(&self, scan_indexes: bool) -> (DiagnosticsInput, String) {
        let input = gather_offline(&self.env, scan_indexes);
        let text = diagnostics::render(&input, &self.redactor());
        (input, text)
    }
}

const CONFIG: &str = r#"
[general]
hotkey = "Ctrl+Alt+K"

[files]
directories = []

[[snippet]]
name = "SECRET-SNIPPET-NAME"
text = "SECRET-SNIPPET-TEXT"

[[web_search]]
keyword = "zz"
name = "SECRET-ENGINE"
url = "https://secret.example/?token=SECRET-TOKEN&q={query}"

[[hotkey]]
key = "Ctrl+Alt+T"
query = "SECRET-HOTKEY-QUERY"
"#;

fn status<'a>(input: &'a DiagnosticsInput, id: &str) -> &'a PluginState {
    &input
        .plugins
        .iter()
        .find(|plugin| plugin.id == id)
        .unwrap_or_else(|| panic!("no plugin {id}"))
        .state
}

fn check<'a>(input: &'a DiagnosticsInput, name: &str) -> &'a HealthCheck {
    input
        .health
        .iter()
        .find(|check| check.name == name)
        .unwrap_or_else(|| panic!("no check {name}"))
}

#[test]
fn the_offline_report_is_made_from_files_and_leaks_nothing() {
    let f = Fixture::new(Some(CONFIG));
    f.write("data/sevak/usage.json", r#"{"SECRET-USAGE": 1}"#);
    f.write("data/sevak/clipboard-history.json", "SECRET-CLIPBOARD");
    f.write("data/sevak/clipboard/SECRET-IMAGE.png", "x");
    f.write(
        "cfg/sevak/plugins/mine/plugin.toml",
        "protocol = 1\nkeyword = \"mm\"\ncommand = [\"SECRET-PROGRAM\"]\n",
    );
    f.write("cfg/sevak/plugins/mine/SECRET-script.py", "SECRET-SCRIPT");
    f.write("cfg/sevak/plugins/bad/plugin.toml", "this is not toml [");
    f.write("cfg/sevak/workflows/broken/workflow.toml", "nor is this [");
    let workflow = sevak_plugins::workflow::templates::all().remove(0).workflow;
    f.write(
        "cfg/sevak/workflows/flow/workflow.toml",
        &workflow.to_toml().unwrap(),
    );
    f.write(
        "data/sevak/logs/sevak.2026-10-04.log",
        "2026-10-04T12:00:00.000000Z  INFO sevak: clip=\"SECRET-CLIP text\" me@example.com\n\
         2026-10-04T12:00:01.000000Z ERROR sevak: thread panicked at src/x.rs:1:1: boom\n",
    );

    let (input, text) = f.report(true);

    // The facts.
    assert_eq!(input.source, ReportSource::CommandLine);
    assert_eq!(input.system.os.name, "FakeOS");
    assert!(matches!(input.config, ConfigState::Loaded(_)));
    assert_eq!(*status(&input, "apps"), PluginState::Loaded);
    assert_eq!(*status(&input, "files"), PluginState::Loaded);
    assert_eq!(*status(&input, "calculator"), PluginState::Enabled);
    assert_eq!(*status(&input, "clipboard"), PluginState::Disabled);
    assert_eq!(*status(&input, "web:zz"), PluginState::Enabled);
    assert_eq!(
        *status(&input, "script:mine"),
        PluginState::WaitingForApproval
    );
    assert!(matches!(
        status(&input, "script:bad"),
        PluginState::Failed(_)
    ));
    assert!(matches!(
        status(&input, "workflow:broken"),
        PluginState::Failed(_)
    ));
    assert!(input.plugins.iter().any(|p| p.id == "workflow:flow"));
    assert_eq!(
        input.indexes,
        [
            IndexSize {
                plugin: "apps".into(),
                count: 2
            },
            IndexSize {
                plugin: "files".into(),
                count: 0
            },
        ]
    );
    assert_eq!(input.logs.errors, 1);
    assert_eq!(input.logs.panics, 1);
    assert!(input
        .keywords
        .iter()
        .any(|k| k.keyword == "zz" && k.owner == "web search"));
    assert!(input
        .keywords
        .iter()
        .any(|k| k.keyword == "mm" && k.owner == "script plugin"));

    // The text.
    assert!(text.contains("- **Operating system:** FakeOS 1.2 (build 77), x86_64"));
    assert!(text.contains("- `usage.json`: 19 bytes"));
    assert!(text.contains("- `script:mine`: waiting for your approval"));
    assert!(text.contains("by `sevak --diagnostics`"));
    assert!(text.contains("1 error, 0 warnings, 1 panic"));

    // Nothing private: no file contents, names inside sub-folders, scripts
    // or commands, snippet or search text, tokens, addresses, the temp path.
    for secret in [
        "SECRET",
        "me@example.com",
        "tester",
        "testbox",
        &f.root.path().display().to_string(),
    ] {
        assert!(!text.contains(secret), "{secret} leaked:\n{text}");
    }
}

#[test]
fn the_probe_file_is_removed() {
    let f = Fixture::new(Some(CONFIG));
    let (input, _) = f.report(false);
    assert_eq!(
        check(&input, "The data folder is writable").status,
        Health::Pass
    );
    assert!(!f.env.paths.data_dir.join(PROBE_FILE).exists());
    assert!(!f.env.paths.config_dir.join(PROBE_FILE).exists());
}

#[test]
fn a_missing_config_and_data_folder_are_warnings_not_failures() {
    let f = Fixture::new(None);
    fs::remove_dir_all(f.env.paths.config_dir.parent().unwrap()).unwrap();
    fs::remove_dir_all(f.env.paths.data_dir.parent().unwrap()).unwrap();
    let (input, text) = f.report(false);
    assert_eq!(input.config, ConfigState::Missing);
    assert_eq!(check(&input, "The config file parses").status, Health::Warn);
    assert_eq!(
        check(&input, "The data folder is writable").status,
        Health::Warn
    );
    assert!(text.contains("There is no config file yet"));
    assert!(text.contains("(does not exist yet)"));
    assert_eq!(*status(&input, "apps"), PluginState::Enabled);
    assert!(input.indexes.is_empty());
    assert!(text.contains("Not available: only the running app keeps these"));
}

#[test]
fn a_broken_config_is_a_failure_that_names_the_line_only() {
    let f = Fixture::new(Some(
        "[general]\nhotkey = 5\n[window]\nwidth = \"SECRET-WIDTH\"\n",
    ));
    let (input, text) = f.report(false);
    let ConfigState::Invalid(message) = &input.config else {
        panic!("{:?}", input.config);
    };
    assert!(message.starts_with("line 2"), "{message}");
    assert_eq!(check(&input, "The config file parses").status, Health::Fail);
    // Sevak runs on the defaults, so the shortcut check uses those.
    assert_eq!(
        check(&input, "The configured shortcut is valid").status,
        Health::Pass
    );
    assert!(!text.contains("SECRET"));
    assert!(text.contains("The config file is not valid"));
}

#[test]
fn a_bad_shortcut_fails_its_check() {
    let f = Fixture::new(Some("[general]\nhotkey = \"Banana+K\"\n"));
    let (input, _) = f.report(false);
    assert_eq!(
        check(&input, "The configured shortcut is valid").status,
        Health::Fail
    );
}

#[test]
fn wayland_does_not_check_the_shortcut() {
    let mut f = Fixture::new(Some("[general]\nhotkey = \"Banana+K\"\n"));
    f.env.display = DisplayServer::Wayland;
    let (input, _) = f.report(false);
    assert_eq!(
        check(&input, "The configured shortcut is valid").status,
        Health::Pass
    );
}

#[test]
fn a_missing_web_view_is_a_failure() {
    let mut f = Fixture::new(Some(CONFIG));
    f.env.webview = Err("WebView2 is not installed".into());
    let (input, text) = f.report(false);
    assert_eq!(
        check(&input, "The web view runtime is installed").status,
        Health::Fail
    );
    assert!(text.contains("not found (WebView2 is not installed)"));
}

#[test]
fn an_unknown_display_server_is_a_warning() {
    let mut f = Fixture::new(Some(CONFIG));
    f.env.display = DisplayServer::Unknown;
    let (input, _) = f.report(false);
    assert_eq!(
        check(&input, "A graphical session was detected").status,
        Health::Warn
    );
}

#[test]
fn plugins_switched_off_in_the_config_are_disabled() {
    let f = Fixture::new(Some(
        "[plugins]\ndisabled = [\"apps\", \"web:zz\", \"script\"]\n[contacts]\nenabled = true\n[[web_search]]\nkeyword = \"zz\"\nname = \"Z\"\nurl = \"https://z.test/{query}\"\n",
    ));
    f.write(
        "cfg/sevak/plugins/mine/plugin.toml",
        "protocol = 1\nkeyword = \"mm\"\ncommand = [\"prog\"]\n",
    );
    let (input, _) = f.report(false);
    assert_eq!(*status(&input, "apps"), PluginState::Disabled);
    assert_eq!(*status(&input, "web:zz"), PluginState::Disabled);
    assert_eq!(*status(&input, "script:mine"), PluginState::Disabled);
    // Opt-in plugins follow their own switch.
    assert_eq!(*status(&input, "contacts"), PluginState::Enabled);
    assert_eq!(*status(&input, "1password"), PluginState::Disabled);
}

fn live(config: Config) -> Live {
    Live {
        config,
        hotkey: HotkeyStatus {
            accelerator: "Super+Space".into(),
            mode: crate::state::HotkeyMode::Global,
            error: None,
            mechanism: Mechanism::WindowsHook,
            note: None,
            can_take_over: false,
            can_restore: false,
        },
        custom_hotkeys: vec![CustomHotkeyStatus {
            key: "Ctrl+Alt+T".into(),
            description: "Run apps:firefox.desktop".into(),
            error: Some("taken by another app".into()),
            mechanism: Mechanism::Inactive,
        }],
        actions_hotkey: None,
        snippet_expansion: ExpansionStatus {
            active: false,
            problem: Some("needs the Input Monitoring permission".into()),
        },
        indexing: false,
        tray_created: true,
        loaded: vec![
            ("apps".into(), Some(312)),
            ("calculator".into(), None),
            ("files".into(), Some(20)),
            ("web:zz".into(), None),
        ],
        refresh_failures: vec![("files".into(), "disk on fire".into())],
    }
}

#[test]
fn the_running_app_adds_its_view() {
    let f = Fixture::new(Some(CONFIG));
    let mut input = gather_offline(&f.env, false);
    let config = match &input.config {
        ConfigState::Loaded(config) => (**config).clone(),
        other => panic!("{other:?}"),
    };
    apply_live(&mut input, live(config));
    let text = diagnostics::render(&input, &f.redactor());

    assert_eq!(input.source, ReportSource::SettingsWindow);
    assert_eq!(*status(&input, "apps"), PluginState::Loaded);
    assert_eq!(*status(&input, "calculator"), PluginState::Loaded);
    assert_eq!(
        *status(&input, "files"),
        PluginState::Failed("disk on fire".into())
    );
    // Enabled but not in the engine.
    assert!(matches!(status(&input, "system"), PluginState::Failed(_)));
    // Off stays off.
    assert_eq!(*status(&input, "clipboard"), PluginState::Disabled);
    assert_eq!(
        check(&input, "The global shortcut is registered").status,
        Health::Pass
    );
    assert_eq!(
        check(&input, "The tray icon was created").status,
        Health::Pass
    );
    assert!(
        text.contains("- **Shortcut:** Super+Space (taken over with the Windows keyboard hook)")
    );
    assert!(text.contains("Ctrl+Alt+T (Run): taken by another app"));
    assert!(text
        .contains("- **Snippet expansion:** not running: needs the Input Monitoring permission"));
    assert!(text.contains("- `apps`: 312 entries"));
    assert!(!text.contains("apps:firefox.desktop"));
    assert!(text.contains("the running app (Settings → Help)"));
}

#[test]
fn a_failing_shortcut_or_missing_tray_shows_in_the_checks() {
    let f = Fixture::new(Some(CONFIG));
    let mut input = gather_offline(&f.env, false);
    let mut state = live(Config::default());
    state.hotkey.error = Some("Win+Space is taken by Windows".into());
    state.hotkey.mechanism = Mechanism::Inactive;
    state.tray_created = false;
    apply_live(&mut input, state);
    let hotkey = check(&input, "The global shortcut is registered");
    assert_eq!(hotkey.status, Health::Fail);
    assert!(hotkey.detail.contains("taken by Windows"));
    assert_eq!(
        check(&input, "The tray icon was created").status,
        Health::Warn
    );
    assert!(input
        .live
        .iter()
        .any(|(name, value)| name == "Shortcut problem" && value.contains("taken by Windows")));
}

#[test]
fn an_inactive_shortcut_without_an_error_still_fails() {
    let status = HotkeyStatus {
        accelerator: "Super+Space".into(),
        mode: crate::state::HotkeyMode::Global,
        error: None,
        mechanism: Mechanism::Inactive,
        note: None,
        can_take_over: false,
        can_restore: false,
    };
    assert_eq!(hotkey_check(&status).status, Health::Fail);
}

#[test]
fn the_config_is_read_without_being_created() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.toml");
    assert_eq!(read_config(&file), ConfigState::Missing);
    assert!(!file.exists());
    fs::write(&file, "[general]\nlaunch_at_login = true\n").unwrap();
    assert!(matches!(read_config(&file), ConfigState::Loaded(_)));
    fs::write(&file, "[general\n").unwrap();
    assert!(matches!(read_config(&file), ConfigState::Invalid(_)));
    // A folder where the file should be cannot be read.
    let folder = dir.path().join("folder.toml");
    fs::create_dir(&folder).unwrap();
    assert!(matches!(read_config(&folder), ConfigState::Unreadable(_)));
}

#[test]
fn the_install_is_classified_from_the_executable() {
    let mut f = Fixture::new(None);
    f.env.exe = Some(PathBuf::from("/home/me/Sevak/target/debug/sevak"));
    f.env.managed_by = Some("Scoop".into());
    let input = gather_offline(&f.env, false);
    assert_eq!(
        input.install.kind,
        diagnostics::InstallKind::DevelopmentBuild
    );
    assert_eq!(input.install.managed_by.as_deref(), Some("Scoop"));
    f.env.exe = None;
    assert_eq!(
        gather_offline(&f.env, false).install.kind,
        diagnostics::InstallKind::Other
    );
}

#[test]
fn the_build_is_described() {
    let build = build_info();
    assert_eq!(build.version, env!("CARGO_PKG_VERSION"));
    assert!(build.profile == "debug" || build.profile == "release");
    assert!(!build.target.is_empty());
}

#[test]
fn config_and_data_in_one_folder_are_listed_once() {
    let mut f = Fixture::new(Some(CONFIG));
    f.env.paths.data_dir = f.env.paths.config_dir.clone();
    f.env.paths.log_dir = f.env.paths.data_dir.join("logs");
    let input = gather_offline(&f.env, false);
    assert!(input.data_dir.is_none());
}
