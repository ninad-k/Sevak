//! The editors whose recent projects are read, where they keep them on each
//! OS, and how to start them.
//!
//! | Editor | Recent list | Started as |
//! |---|---|---|
//! | VS Code, Insiders, VSCodium, Cursor | `<config>/<name>/User/globalStorage/state.vscdb` (SQLite), older `storage.json` | `Code.exe <path>`, `open -a <app> <path>`, `code <path>` |
//! | JetBrains IDEs, Android Studio | `<config>/JetBrains/<Product><version>/options/recentProjects.xml` (`Google/AndroidStudio…` for Android Studio) | `<product>64.exe`, `open -a`, the launcher script |
//!
//! `<config>` is `%APPDATA%` on Windows, `~/Library/Application Support` on
//! macOS and `$XDG_CONFIG_HOME` (or `~/.config`) on Linux.
//!
//! Everything here takes an [`Env`] so tests can point it at a temporary
//! folder that looks like any of the three systems.

use std::fs;
use std::path::{Path, PathBuf};

use crate::files::home_dir;

/// Which system's layout to expect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

/// The folders the editors are found in.
#[derive(Debug, Clone)]
pub struct Env {
    pub os: Os,
    pub home: Option<PathBuf>,
    /// `%APPDATA%` (Windows).
    pub appdata: Option<PathBuf>,
    /// `%LOCALAPPDATA%` (Windows).
    pub local_appdata: Option<PathBuf>,
    /// `%ProgramFiles%` and `%ProgramFiles(x86)%` (Windows).
    pub program_files: Vec<PathBuf>,
    /// `$XDG_CONFIG_HOME` (Linux).
    pub xdg_config: Option<PathBuf>,
    /// The folders on `PATH`.
    pub path: Vec<PathBuf>,
    /// Where macOS apps are installed: `/Applications` and `~/Applications`.
    pub mac_apps: Vec<PathBuf>,
}

impl Env {
    /// This computer.
    pub fn current() -> Self {
        let os = if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        };
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        let home = home_dir();
        Self {
            os,
            mac_apps: [
                Some(PathBuf::from("/Applications")),
                home.as_ref().map(|h| h.join("Applications")),
            ]
            .into_iter()
            .flatten()
            .collect(),
            home,
            appdata: var("APPDATA"),
            local_appdata: var("LOCALAPPDATA"),
            program_files: ["ProgramFiles", "ProgramFiles(x86)"]
                .into_iter()
                .filter_map(var)
                .collect(),
            xdg_config: var("XDG_CONFIG_HOME"),
            path: std::env::var_os("PATH")
                .map(|p| {
                    std::env::split_paths(&p)
                        .filter(|d| d.is_absolute())
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// Where per-user application settings live.
    pub fn config_root(&self) -> Option<PathBuf> {
        match self.os {
            Os::Windows => self.appdata.clone(),
            Os::MacOs => self
                .home
                .as_ref()
                .map(|h| h.join("Library").join("Application Support")),
            Os::Linux => self
                .xdg_config
                .clone()
                .or_else(|| self.home.as_ref().map(|h| h.join(".config"))),
        }
    }

    /// `command` in a `PATH` folder (`command.exe` on Windows).
    fn find_in_path(&self, command: &str) -> Option<PathBuf> {
        let names: Vec<String> = match self.os {
            Os::Windows => vec![format!("{command}.exe")],
            _ => vec![command.to_owned()],
        };
        self.path
            .iter()
            .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
            .find(|candidate| candidate.is_file())
    }

    /// `<name>.app` in `/Applications` or `~/Applications`.
    fn mac_app(&self, name: &str) -> Option<PathBuf> {
        self.mac_apps
            .iter()
            .map(|dir| dir.join(format!("{name}.app")))
            .find(|app| app.is_dir())
    }
}

/// How an editor is started with a project: `program args... <path>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launcher {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl Launcher {
    fn direct(program: PathBuf) -> Self {
        Self {
            program,
            args: Vec::new(),
        }
    }

    /// `open -a <app>` on macOS.
    fn mac_open(app: &Path) -> Self {
        Self {
            program: PathBuf::from("/usr/bin/open"),
            args: vec!["-a".to_owned(), app.to_string_lossy().into_owned()],
        }
    }
}

/// A VS Code build: the editor and its forks share one storage format.
#[derive(Debug, Clone, Copy)]
pub struct VsCodeFlavor {
    /// The id in `[projects] editors`.
    pub id: &'static str,
    pub name: &'static str,
    /// Its folder under the config root.
    pub data_dir: &'static str,
    /// Windows: install folder (under `%LOCALAPPDATA%\Programs` or
    /// `%ProgramFiles%`) and program.
    windows: (&'static str, &'static str),
    mac_app: &'static str,
    linux_command: &'static str,
}

pub const VSCODE_FLAVORS: [VsCodeFlavor; 4] = [
    VsCodeFlavor {
        id: "vscode",
        name: "VS Code",
        data_dir: "Code",
        windows: ("Microsoft VS Code", "Code.exe"),
        mac_app: "Visual Studio Code",
        linux_command: "code",
    },
    VsCodeFlavor {
        id: "vscode-insiders",
        name: "VS Code Insiders",
        data_dir: "Code - Insiders",
        windows: ("Microsoft VS Code Insiders", "Code - Insiders.exe"),
        mac_app: "Visual Studio Code - Insiders",
        linux_command: "code-insiders",
    },
    VsCodeFlavor {
        id: "vscodium",
        name: "VSCodium",
        data_dir: "VSCodium",
        windows: ("VSCodium", "VSCodium.exe"),
        mac_app: "VSCodium",
        linux_command: "codium",
    },
    VsCodeFlavor {
        id: "cursor",
        name: "Cursor",
        data_dir: "Cursor",
        windows: ("cursor", "Cursor.exe"),
        mac_app: "Cursor",
        linux_command: "cursor",
    },
];

impl VsCodeFlavor {
    pub fn data_root(&self, env: &Env) -> Option<PathBuf> {
        env.config_root().map(|root| root.join(self.data_dir))
    }

    pub fn launcher(&self, env: &Env) -> Option<Launcher> {
        match env.os {
            Os::Windows => {
                let (folder, exe) = self.windows;
                env.local_appdata
                    .iter()
                    .map(|dir| dir.join("Programs"))
                    .chain(env.program_files.iter().cloned())
                    .map(|base| base.join(folder).join(exe))
                    .find(|program| program.is_file())
                    .map(Launcher::direct)
            }
            Os::MacOs => env
                .mac_app(self.mac_app)
                .map(|app| Launcher::mac_open(&app)),
            Os::Linux => env
                .find_in_path(self.linux_command)
                .or_else(|| {
                    Some(PathBuf::from("/snap/bin").join(self.linux_command))
                        .filter(|p| p.is_file())
                })
                .map(Launcher::direct),
        }
    }
}

/// The id of the JetBrains family in `[projects] editors`.
pub const JETBRAINS_ID: &str = "jetbrains";

/// A JetBrains IDE (or Android Studio, built on the same platform).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JetBrainsProduct {
    /// Its settings folder is this followed by the version (`PyCharm2024.1`).
    prefix: &'static str,
    pub name: &'static str,
    mac_app: &'static str,
    /// The launcher script (Linux) and, with `64.exe`, the program (Windows).
    command: &'static str,
}

const fn product(
    prefix: &'static str,
    name: &'static str,
    mac_app: &'static str,
    command: &'static str,
) -> JetBrainsProduct {
    JetBrainsProduct {
        prefix,
        name,
        mac_app,
        command,
    }
}

const JETBRAINS_PRODUCTS: [JetBrainsProduct; 15] = [
    product("IntelliJIdea", "IntelliJ IDEA", "IntelliJ IDEA", "idea"),
    product("IdeaIC", "IntelliJ IDEA CE", "IntelliJ IDEA CE", "idea"),
    product("PyCharm", "PyCharm", "PyCharm", "pycharm"),
    product("PyCharmCE", "PyCharm CE", "PyCharm CE", "pycharm"),
    product("WebStorm", "WebStorm", "WebStorm", "webstorm"),
    product("GoLand", "GoLand", "GoLand", "goland"),
    product("CLion", "CLion", "CLion", "clion"),
    product("Rider", "Rider", "Rider", "rider"),
    product("RustRover", "RustRover", "RustRover", "rustrover"),
    product("PhpStorm", "PhpStorm", "PhpStorm", "phpstorm"),
    product("RubyMine", "RubyMine", "RubyMine", "rubymine"),
    product("DataGrip", "DataGrip", "DataGrip", "datagrip"),
    product("DataSpell", "DataSpell", "DataSpell", "dataspell"),
    product("Aqua", "Aqua", "Aqua", "aqua"),
    product(
        "AndroidStudio",
        "Android Studio",
        "Android Studio",
        "studio",
    ),
];

impl JetBrainsProduct {
    /// The product whose settings folder is `folder` (`GoLand2024.2`).
    pub fn for_folder(folder: &str) -> Option<Self> {
        JETBRAINS_PRODUCTS.iter().copied().find(|p| {
            folder
                .strip_prefix(p.prefix)
                .and_then(|rest| rest.chars().next())
                .is_some_and(|c| c.is_ascii_digit())
        })
    }

    /// A short id for result keys (`goland`, `pycharm-ce`).
    pub fn key(&self) -> String {
        self.name.to_lowercase().replace(' ', "-")
    }

    pub fn launcher(&self, env: &Env) -> Option<Launcher> {
        match env.os {
            Os::Windows => {
                let exe = format!("{}64.exe", self.command);
                let mut bases: Vec<PathBuf> = env
                    .local_appdata
                    .iter()
                    .map(|d| d.join("Programs"))
                    .collect();
                for files in &env.program_files {
                    bases.push(files.join("JetBrains"));
                    bases.push(files.join("Android"));
                }
                bases.iter().find_map(|base| newest_with(base, &exe))
            }
            Os::MacOs => env
                .mac_app(self.mac_app)
                .map(|app| Launcher::mac_open(&app)),
            Os::Linux => env
                .find_in_path(self.command)
                .or_else(|| {
                    env.home
                        .as_ref()
                        .map(|h| {
                            h.join(".local/share/JetBrains/Toolbox/scripts")
                                .join(self.command)
                        })
                        .filter(|p| p.is_file())
                })
                .or_else(|| {
                    Some(PathBuf::from("/snap/bin").join(self.command)).filter(|p| p.is_file())
                })
                .map(Launcher::direct),
        }
    }
}

/// The JetBrains settings roots: `<config>/JetBrains` and `<config>/Google`
/// (Android Studio).
pub fn jetbrains_roots(env: &Env) -> Vec<PathBuf> {
    env.config_root()
        .map(|root| vec![root.join("JetBrains"), root.join("Google")])
        .unwrap_or_default()
}

/// `<base>/<install>/bin/<exe>` in the install folder that sorts last
/// (the newest version, as JetBrains name them `PyCharm 2024.2`).
fn newest_with(base: &Path, exe: &str) -> Option<Launcher> {
    let mut installs: Vec<PathBuf> = fs::read_dir(base)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("bin").join(exe))
        .filter(|program| program.is_file())
        .collect();
    installs.sort();
    installs.pop().map(Launcher::direct)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An `Env` for `os` with every folder inside `root`.
    pub(crate) fn env_in(root: &Path, os: Os) -> Env {
        Env {
            os,
            home: Some(root.join("home")),
            appdata: Some(root.join("appdata")),
            local_appdata: Some(root.join("local")),
            program_files: vec![root.join("pf")],
            xdg_config: None,
            path: vec![root.join("bin")],
            mac_apps: vec![root.join("Applications")],
        }
    }

    #[test]
    fn config_roots_per_os() {
        let root = Path::new("/r");
        assert_eq!(
            env_in(root, Os::Windows).config_root(),
            Some(root.join("appdata"))
        );
        assert_eq!(
            env_in(root, Os::MacOs).config_root(),
            Some(root.join("home/Library/Application Support"))
        );
        assert_eq!(
            env_in(root, Os::Linux).config_root(),
            Some(root.join("home/.config"))
        );
        let mut xdg = env_in(root, Os::Linux);
        xdg.xdg_config = Some(root.join("xdg"));
        assert_eq!(xdg.config_root(), Some(root.join("xdg")));
    }

    #[test]
    fn jetbrains_folders_name_their_product() {
        let name = |folder: &str| JetBrainsProduct::for_folder(folder).map(|p| p.name);
        assert_eq!(name("PyCharm2024.1"), Some("PyCharm"));
        assert_eq!(name("PyCharmCE2023.3"), Some("PyCharm CE"));
        assert_eq!(name("IdeaIC2024.2"), Some("IntelliJ IDEA CE"));
        assert_eq!(name("AndroidStudio2023.1"), Some("Android Studio"));
        assert_eq!(name("Toolbox"), None);
        assert_eq!(name("PyCharm"), None);
        assert_eq!(
            JetBrainsProduct::for_folder("PyCharmCE2023.3")
                .unwrap()
                .key(),
            "pycharm-ce"
        );
    }

    #[test]
    fn launchers_are_found_per_os() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let touch = |path: PathBuf| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "").unwrap();
            path
        };

        let windows = env_in(root, Os::Windows);
        assert_eq!(VSCODE_FLAVORS[0].launcher(&windows), None);
        let code = touch(root.join("local/Programs/Microsoft VS Code/Code.exe"));
        assert_eq!(
            VSCODE_FLAVORS[0].launcher(&windows),
            Some(Launcher::direct(code))
        );
        touch(root.join("pf/JetBrains/PyCharm 2023.3/bin/pycharm64.exe"));
        let newer = touch(root.join("pf/JetBrains/PyCharm 2024.2/bin/pycharm64.exe"));
        let pycharm = JetBrainsProduct::for_folder("PyCharm2024.2").unwrap();
        assert_eq!(pycharm.launcher(&windows), Some(Launcher::direct(newer)));

        let mac = env_in(root, Os::MacOs);
        fs::create_dir_all(root.join("Applications/Cursor.app")).unwrap();
        let cursor = VSCODE_FLAVORS[3].launcher(&mac).unwrap();
        assert_eq!(cursor.program, PathBuf::from("/usr/bin/open"));
        assert_eq!(cursor.args[0], "-a");
        assert!(cursor.args[1].ends_with("Cursor.app"));

        let linux = env_in(root, Os::Linux);
        let codium = touch(root.join("bin/codium"));
        assert_eq!(
            VSCODE_FLAVORS[2].launcher(&linux),
            Some(Launcher::direct(codium))
        );
        let script = touch(root.join("home/.local/share/JetBrains/Toolbox/scripts/goland"));
        let goland = JetBrainsProduct::for_folder("GoLand2024.1").unwrap();
        assert_eq!(goland.launcher(&linux), Some(Launcher::direct(script)));
    }
}
