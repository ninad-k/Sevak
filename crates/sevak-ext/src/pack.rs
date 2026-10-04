//! `sevak-ext pack`: a `.sevakext` package from a project and its programs.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sevak_plugins::extensions::{summarize, Builder, EXTENSION};
use sevak_plugins::net::sha256_hex;
use sevak_plugins::script::MANIFEST_FILE;

use crate::{Args, Failure};

/// The largest manifest or data file read for a package.
const MAX_INPUT_BYTES: u64 = 64 * 1024 * 1024;

/// One package written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    pub path: PathBuf,
    pub platforms: Vec<String>,
    pub sha256: String,
    pub bytes: usize,
}

/// What to pack.
pub struct Plan {
    /// The project folder with `plugin.toml`.
    pub dir: PathBuf,
    /// `(platform, program file)`.
    pub binaries: Vec<(String, PathBuf)>,
    /// `(file, path inside the package)`.
    pub includes: Vec<(PathBuf, String)>,
    pub out: PathBuf,
    /// One package per platform.
    pub split: bool,
}

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let meta =
        fs::metadata(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if meta.len() > MAX_INPUT_BYTES {
        return Err(format!("{} is too large to pack", path.display()));
    }
    fs::read(path).map_err(|err| format!("cannot read {}: {err}", path.display()))
}

/// Builds the packages of `plan` and writes them to `plan.out`.
pub fn pack(plan: &Plan) -> Result<Vec<Packed>, String> {
    let manifest = String::from_utf8(read_file(&plan.dir.join(MANIFEST_FILE))?)
        .map_err(|_| "plugin.toml is not text".to_owned())?;
    let summary = summarize(&manifest, "")?;
    let mut builder = Builder::new(&manifest, "")?;
    if plan.binaries.is_empty() {
        return Err(
            "give at least one program: --binary linux-x86_64=target/release/<name>".to_owned(),
        );
    }
    for (platform, file) in &plan.binaries {
        builder.add_binary(platform, read_file(file)?)?;
    }
    for (file, path) in &plan.includes {
        builder.add_file(path, read_file(file)?)?;
    }

    let version = &summary.native.version;
    let mut jobs: Vec<(Option<String>, String)> = Vec::new();
    if plan.split {
        for platform in builder.platforms() {
            let name = format!("{}-{version}-{platform}.{EXTENSION}", summary.id);
            jobs.push((Some(platform), name));
        }
    } else {
        jobs.push((None, format!("{}-{version}.{EXTENSION}", summary.id)));
    }
    fs::create_dir_all(&plan.out)
        .map_err(|err| format!("cannot create {}: {err}", plan.out.display()))?;
    let mut written = Vec::new();
    for (platform, name) in jobs {
        let bytes = builder.build(platform.as_deref())?;
        let path = plan.out.join(name);
        fs::write(&path, &bytes)
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        written.push(Packed {
            path,
            platforms: match platform {
                Some(platform) => vec![platform],
                None => builder.platforms(),
            },
            sha256: sha256_hex(&bytes),
            bytes: bytes.len(),
        });
    }
    Ok(written)
}

pub(crate) fn command(args: &[String], out: &mut dyn Write) -> Result<(), Failure> {
    let args = Args::parse(args, &["binary", "include", "out"], &["split"])?;
    let dir = match args.positional.as_slice() {
        [] => PathBuf::from("."),
        [dir] => PathBuf::from(dir),
        _ => return Err(Failure::Usage("pack takes at most one folder".to_owned())),
    };
    let mut binaries = Vec::new();
    for spec in args.all("binary") {
        let (platform, file) = spec.split_once('=').ok_or_else(|| {
            Failure::Usage(format!("--binary wants PLATFORM=FILE, not \"{spec}\""))
        })?;
        binaries.push((platform.to_owned(), PathBuf::from(file)));
    }
    let mut includes = Vec::new();
    for spec in args.all("include") {
        let (file, path) = match spec.split_once('=') {
            Some((file, path)) => (file, path.to_owned()),
            None => (
                spec,
                Path::new(spec)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
        };
        includes.push((PathBuf::from(file), path));
    }
    let plan = Plan {
        dir,
        binaries,
        includes,
        out: PathBuf::from(args.value("out").unwrap_or(".")),
        split: args.flag("split"),
    };
    for packed in pack(&plan)? {
        writeln!(
            out,
            "{}  {}  ({} bytes; {})",
            packed.sha256,
            packed.path.display(),
            packed.bytes,
            packed.platforms.join(", ")
        )
        .map_err(crate::io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sevak_plugins::extensions::ExtensionPackage;

    use super::*;

    const MANIFEST: &str =
        "protocol = 1\nid = \"script:tool\"\nkeyword = \"t\"\nname = \"Tool\"\ndescription = \"A tool.\"\n\
        [extension]\nversion = \"1.2.3\"\nauthor = \"Ada\"\nlicense = \"MIT\"\n\
        repository = \"https://github.com/example/tool\"\nmin_sevak = \"0.1.0\"\npermissions = []\n\
        [extension.binaries]\nwindows-x86_64 = \"bin/tool-windows-x86_64.exe\"\n\
        linux-x86_64 = \"bin/tool-linux-x86_64\"\n";

    fn project() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tool");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.toml"), MANIFEST).unwrap();
        fs::write(tmp.path().join("win.exe"), b"MZ windows build").unwrap();
        fs::write(tmp.path().join("lin"), b"ELF linux build").unwrap();
        fs::write(tmp.path().join("icon.png"), b"png").unwrap();
        (tmp, dir)
    }

    fn plan(tmp: &tempfile::TempDir, dir: &Path, split: bool) -> Plan {
        Plan {
            dir: dir.to_path_buf(),
            binaries: vec![
                ("windows-x86_64".to_owned(), tmp.path().join("win.exe")),
                ("linux-x86_64".to_owned(), tmp.path().join("lin")),
            ],
            includes: vec![(tmp.path().join("icon.png"), "icon.png".to_owned())],
            out: tmp.path().join("dist"),
            split,
        }
    }

    #[test]
    fn one_package_carries_every_program() {
        let (tmp, dir) = project();
        let written = pack(&plan(&tmp, &dir, false)).unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].path.file_name().unwrap(), "tool-1.2.3.sevakext");
        let bytes = fs::read(&written[0].path).unwrap();
        assert_eq!(written[0].sha256, sha256_hex(&bytes));
        let package = ExtensionPackage::read(&bytes, "tool").unwrap();
        assert_eq!(package.platforms(), ["linux-x86_64", "windows-x86_64"]);
        assert!(package.files.iter().any(|f| f.path == "icon.png"));
    }

    #[test]
    fn split_writes_one_package_per_platform() {
        let (tmp, dir) = project();
        let written = pack(&plan(&tmp, &dir, true)).unwrap();
        let names: Vec<String> = written
            .iter()
            .map(|p| p.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "tool-1.2.3-linux-x86_64.sevakext",
                "tool-1.2.3-windows-x86_64.sevakext"
            ]
        );
        for packed in &written {
            let package = ExtensionPackage::read(&fs::read(&packed.path).unwrap(), "tool").unwrap();
            assert_eq!(package.platforms(), packed.platforms);
        }
    }

    #[test]
    fn packing_twice_gives_the_same_bytes() {
        let (tmp, dir) = project();
        let first = pack(&plan(&tmp, &dir, false)).unwrap();
        let second = pack(&plan(&tmp, &dir, false)).unwrap();
        assert_eq!(first[0].sha256, second[0].sha256);
    }

    #[test]
    fn mistakes_are_explained() {
        let (tmp, dir) = project();
        let mut none = plan(&tmp, &dir, false);
        none.binaries.clear();
        assert!(pack(&none).unwrap_err().contains("at least one program"));

        let mut undeclared = plan(&tmp, &dir, false);
        undeclared.binaries = vec![("macos-aarch64".to_owned(), tmp.path().join("lin"))];
        assert!(pack(&undeclared)
            .unwrap_err()
            .contains("declares no binary for macos-aarch64"));

        let mut missing = plan(&tmp, &dir, false);
        missing.binaries = vec![("linux-x86_64".to_owned(), tmp.path().join("nope"))];
        assert!(pack(&missing).unwrap_err().contains("cannot read"));

        let mut shadow = plan(&tmp, &dir, false);
        shadow.includes = vec![(tmp.path().join("icon.png"), "plugin.toml".to_owned())];
        assert!(pack(&shadow).is_err());

        fs::write(
            dir.join("plugin.toml"),
            "protocol = 1\nid = \"script:tool\"\nkeyword = \"t\"\ncommand = [\"x\"]\n",
        )
        .unwrap();
        assert!(pack(&plan(&tmp, &dir, false))
            .unwrap_err()
            .contains("[extension]"));
    }

    #[test]
    fn the_command_line_form() {
        let (tmp, dir) = project();
        let args: Vec<String> = [
            dir.to_str().unwrap(),
            "--binary",
            &format!("linux-x86_64={}", tmp.path().join("lin").display()),
            "--out",
            tmp.path().join("o").to_str().unwrap(),
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let mut out = Vec::new();
        command(&args, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("tool-1.2.3.sevakext"), "{text}");
        assert!(text.contains("linux-x86_64"), "{text}");
        let bad: Vec<String> = vec![
            dir.to_str().unwrap().to_owned(),
            "--binary".to_owned(),
            "nope".to_owned(),
        ];
        assert!(matches!(
            command(&bad, &mut Vec::new()),
            Err(Failure::Usage(_))
        ));
    }
}
