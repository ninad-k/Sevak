use super::*;
use crate::config::{HotkeyBinding, Snippet, WebSearchEngine};

fn redactor() -> Redactor {
    Redactor::new(&Identity {
        home_dirs: vec![r"C:\Users\Ninad".to_owned()],
        user_names: vec!["Ninad".to_owned()],
        host_names: vec!["DESKTOP-7QK2LM".to_owned()],
    })
}

fn sample_config() -> Config {
    let mut config = Config::default();
    config.files.directories = vec![r"C:\Users\Ninad\SECRET-CLIENT-DIR".into()];
    config.snippet = vec![Snippet {
        name: "SECRET-SNIPPET-NAME".into(),
        keyword: Some("SECRET-SNIPPET-KW".into()),
        text: "SECRET-SNIPPET-TEXT".into(),
    }];
    config.web_search = vec![WebSearchEngine {
        keyword: "g".into(),
        name: "SECRET-ENGINE-NAME".into(),
        url: "https://secret.example/?key=SECRET-API-KEY&q={query}".into(),
    }];
    config.hotkeys = vec![HotkeyBinding {
        key: "Ctrl+Alt+T".into(),
        query: Some("SECRET-HOTKEY-QUERY".into()),
        run: None,
    }];
    config.appearance.custom_css = "/* SECRET-CSS */".into();
    config
}

fn tail() -> Vec<String> {
    let line = |level: &str, text: &str| {
        format!("2026-10-04T12:00:00.000000Z {level:>5} sevak::search: {text}")
    };
    vec![
        line("INFO", r#"starting sevak version="0.1.0" display=Windows"#),
        line(
            "INFO",
            r#"loaded config path="C:\\Users\\Ninad\\AppData\\Roaming\\sevak\\config.toml""#,
        ),
        line(
            "WARN",
            r#"could not paste text="SECRET-CLIPBOARD my bank pin is 1234" into app"#,
        ),
        line(
            "ERROR",
            "thread panicked at src/x.rs:10:5: index out of bounds",
        ),
        line(
            "INFO",
            "mail SECRET@example.com from 10.20.30.40 ```fence```",
        ),
        line(
            "INFO",
            "token ghp_16C7e42F292c6912E7710c838347Ae178B4a leaked",
        ),
        line(
            "INFO",
            r"opened D:\Clients\SECRET-CLIENT\plan.pdf for Ninad",
        ),
    ]
}

fn input(config: ConfigState) -> DiagnosticsInput {
    DiagnosticsInput {
        generated_at_unix: 1_791_115_200,
        source: ReportSource::SettingsWindow,
        build: BuildInfo {
            version: "0.1.0".into(),
            commit: Some("9f3e2d1".into()),
            profile: "release".into(),
            target: "x86_64-pc-windows-msvc".into(),
        },
        system: SystemInfo {
            os: OsInfo {
                name: "Windows 11 Pro".into(),
                version: "24H2".into(),
                build: Some("26100.2314".into()),
                kernel: None,
                arch: "x86_64".into(),
            },
            display_server: "windows".into(),
            desktop: None,
            webview: Ok("130.0.2849.80".into()),
        },
        install: InstallInfo {
            exe: Some(r"C:\Users\Ninad\AppData\Local\Sevak\sevak.exe".into()),
            kind: InstallKind::WindowsPerUser,
            managed_by: None,
        },
        paths: PathsInfo {
            config_dir: r"C:\Users\Ninad\AppData\Roaming\sevak".into(),
            config_file: r"C:\Users\Ninad\AppData\Roaming\sevak\config.toml".into(),
            data_dir: r"C:\Users\Ninad\AppData\Roaming\sevak".into(),
            log_dir: r"C:\Users\Ninad\AppData\Roaming\sevak\logs".into(),
            config_overridden: false,
            data_overridden: false,
        },
        config,
        keywords: vec![
            KeywordEntry {
                keyword: "yt".into(),
                owner: "web search".into(),
            },
            KeywordEntry {
                keyword: ">".into(),
                owner: "built in".into(),
            },
        ],
        health: vec![
            HealthCheck::new("The config file parses", Health::Pass, ""),
            HealthCheck::new(
                "The global shortcut is registered",
                Health::Fail,
                r#"taken by "Some Other App" (mail me@example.com)"#,
            ),
            HealthCheck::new(
                "The tray icon exists",
                Health::Unknown,
                "needs the running app",
            ),
        ],
        live: vec![(
            "Shortcut".into(),
            "Super+Space via the keyboard hook".into(),
        )],
        plugins: vec![
            PluginStatus {
                id: "apps".into(),
                kind: PluginKind::Builtin,
                state: PluginState::Loaded,
            },
            PluginStatus {
                id: "files".into(),
                kind: PluginKind::Builtin,
                state: PluginState::Failed(r"cannot read D:\Clients\SECRET-CLIENT: denied".into()),
            },
            PluginStatus {
                id: "clipboard".into(),
                kind: PluginKind::Builtin,
                state: PluginState::Disabled,
            },
            PluginStatus {
                id: "script:my-plugin".into(),
                kind: PluginKind::Script,
                state: PluginState::WaitingForApproval,
            },
            PluginStatus {
                id: "workflow:my-flow".into(),
                kind: PluginKind::Workflow,
                state: PluginState::Loaded,
            },
        ],
        indexes: vec![
            IndexSize {
                plugin: "apps".into(),
                count: 312,
            },
            IndexSize {
                plugin: "files".into(),
                count: 20_000,
            },
        ],
        indexes_note: None,
        config_dir: Inventory {
            exists: true,
            entries: vec![
                FileEntry {
                    name: "config.toml".into(),
                    bytes: 2_048,
                    files: None,
                },
                FileEntry {
                    name: "themes".into(),
                    bytes: 1_500,
                    files: Some(3),
                },
            ],
            truncated: false,
            error: None,
        },
        data_dir: None,
        logs: LogScan {
            files: vec![FileEntry {
                name: "sevak.2026-10-04.log".into(),
                bytes: 4_000,
                files: None,
            }],
            tail: tail(),
            errors: 1,
            warnings: 1,
            panics: 1,
            error: None,
        },
    }
}

fn report() -> String {
    render(
        &input(ConfigState::Loaded(Box::new(sample_config()))),
        &redactor(),
    )
}

#[test]
fn the_report_says_what_it_contains_and_what_it_does_not() {
    let report = report();
    assert!(report.starts_with("# Sevak diagnostics\n"));
    assert!(report.contains("Made 2026-10-04 12:00:00 UTC by Settings → Help"));
    assert!(report.contains("Nothing was sent anywhere"));
    for included in [
        "**Included:**",
        "the Sevak version and build",
        "the last 100 log lines",
    ] {
        assert!(report.contains(included), "{included}");
    }
    for excluded in [
        "**Not included:**",
        "clipboard history or snippet text",
        "what is in any script or workflow",
        "what you searched for",
        "usage statistics",
        "contacts or 1Password data",
        "file lists or bookmark titles",
    ] {
        assert!(report.contains(excluded), "{excluded}");
    }
}

#[test]
fn the_report_has_every_section() {
    let report = report();
    for heading in [
        "## Overview",
        "## Health checks",
        "## Configuration",
        "## Keywords in use",
        "## Plugins",
        "## Index sizes",
        "## Files in Sevak's folders",
        "## Logs",
    ] {
        assert!(report.contains(heading), "{heading}");
    }
    assert!(report
        .contains("- **Sevak:** 0.1.0, commit 9f3e2d1 (release build for x86_64-pc-windows-msvc)"));
    assert!(
        report.contains("- **Operating system:** Windows 11 Pro 24H2 (build 26100.2314), x86_64")
    );
    assert!(report.contains("- **Web view:** 130.0.2849.80"));
    assert!(report.contains("- **Install type:** installed for this user only"));
    assert!(report.contains(r"- **Program:** ~\AppData\Local\Sevak\sevak.exe"));
    assert!(report.contains(r"- **Config folder:** ~\AppData\Roaming\sevak"));
    assert!(report.contains("- **Shortcut:** Super+Space via the keyboard hook"));
    assert!(report.contains("- **OK** The config file parses."));
    assert!(report.contains("- **FAIL** The global shortcut is registered: taken by"));
    assert!(report.contains("- **?** The tray icon exists: needs the running app"));
    assert!(report.contains("- `general.hotkey` = Super+Space"));
    assert!(report.contains("- `files.directories` = 1"));
    assert!(report.contains("- `>`: built in"));
    assert!(report.contains("- `apps`: loaded"));
    assert!(report.contains("- `clipboard`: switched off"));
    assert!(report.contains("- `script:my-plugin`: waiting for your approval"));
    assert!(report.contains("- `workflow:my-flow`: loaded"));
    assert!(report.contains("- `files`: 20000 entries"));
    assert!(report.contains("- `config.toml`: 2.0 KB"));
    assert!(report.contains("- `themes/`: 3 files, 1.5 KB"));
    assert!(report.contains("The data folder is the config folder."));
    assert!(report.contains("1 error, 1 warning, 1 panic"));
}

#[test]
fn nothing_private_gets_into_the_report() {
    let report = report();
    for secret in [
        "SECRET",
        "Ninad",
        "DESKTOP-7QK2LM",
        "ghp_16C7",
        "example.com",
        "10.20.30.40",
        "1234",
        r"C:\Users",
        "Clients",
    ] {
        assert!(
            !report.to_lowercase().contains(&secret.to_lowercase()),
            "{secret:?} is in the report:\n{report}"
        );
    }
    // The paths that remain are Sevak's own, below `~`.
    assert!(report.contains(r"~\AppData\Roaming\sevak"));
}

#[test]
fn log_lines_cannot_break_out_of_their_block() {
    let report = report();
    let block: Vec<&str> = report
        .lines()
        .skip_while(|line| *line != "```text")
        .skip(1)
        .take_while(|line| *line != "```")
        .collect();
    assert_eq!(block.len(), 7, "{block:?}");
    assert!(report.trim_end().ends_with("```"));
    assert_eq!(report.matches("```").count(), 2);
}

#[test]
fn a_broken_config_is_reported_without_its_text() {
    let report = render(
        &input(ConfigState::Invalid(
            r#"line 12, column 9: invalid type: string "SECRET-VALUE here", expected u32"#.into(),
        )),
        &redactor(),
    );
    assert!(report.contains("The config file is not valid"));
    assert!(report.contains("line 12, column 9: invalid type: string \"<redacted>\", expected u32"));
    assert!(!report.contains("SECRET"));
    assert!(!report.contains("## Configuration\n\nOnly the settings"));
    let missing = render(&input(ConfigState::Missing), &redactor());
    assert!(missing.contains("There is no config file yet"));
}

#[test]
fn the_command_line_report_says_what_it_cannot_see() {
    let mut command_line = input(ConfigState::Missing);
    command_line.source = ReportSource::CommandLine;
    command_line.live.clear();
    command_line.indexes.clear();
    command_line.indexes_note = Some("only the running app knows".into());
    let report = render(&command_line, &redactor());
    assert!(report.contains("by `sevak --diagnostics`"));
    assert!(report.contains("the command line: files only"));
    assert!(report.contains("Not available: only the running app knows."));
}

#[test]
fn empty_and_missing_pieces_have_a_line_of_their_own() {
    let mut empty = input(ConfigState::Missing);
    empty.plugins.clear();
    empty.keywords.clear();
    empty.config_dir = Inventory::default();
    empty.data_dir = Some(Inventory {
        exists: true,
        ..Inventory::default()
    });
    empty.logs = LogScan::default();
    empty.system.webview = Err("WebView2 is not installed".into());
    empty.system.desktop = Some("ubuntu:GNOME".into());
    let report = render(&empty, &redactor());
    assert!(report.contains("- (none)"));
    assert!(report.contains("- (does not exist yet)"));
    assert!(report.contains("- (empty)"));
    assert!(report.contains("There are no log files yet."));
    assert!(report.contains("- **Web view:** not found (WebView2 is not installed)"));
    assert!(report.contains("- **Desktop:** ubuntu:GNOME"));
    assert!(!report.contains("## Keywords in use"));
}

#[test]
fn the_time_is_utc() {
    assert_eq!(format_utc(0), "1970-01-01 00:00:00 UTC");
    assert_eq!(format_utc(951_782_400), "2000-02-29 00:00:00 UTC");
    assert_eq!(format_utc(1_791_115_200), "2026-10-04 12:00:00 UTC");
    assert_eq!(format_utc(4_102_444_799), "2099-12-31 23:59:59 UTC");
}

#[test]
fn sizes_read_naturally() {
    assert_eq!(format_bytes(0), "0 bytes");
    assert_eq!(format_bytes(1), "1 byte");
    assert_eq!(format_bytes(1023), "1023 bytes");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
}
