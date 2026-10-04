//! `sevak-ext entry`: the gallery index entry for packages.
//!
//! The gallery lists a native extension with one package per platform. A
//! maintainer (or the author, in a pull request) runs `pack --split`, copies the
//! packages to `gallery/extensions/<id>/` and pastes this output into
//! `gallery/index.json`.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use sevak_plugins::extensions::{summarize, ExtensionPackage, Summary};
use sevak_plugins::net::sha256_hex;

use crate::{Args, Failure};

/// One package given on the command line.
struct Given {
    file_name: String,
    sha256: String,
    platforms: Vec<String>,
    manifest: String,
}

fn read(path: &Path) -> Result<Given, String> {
    let bytes = fs::read(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    let package = ExtensionPackage::read(&bytes, "")?;
    Ok(Given {
        file_name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{} has no file name", path.display()))?,
        sha256: sha256_hex(&bytes),
        platforms: package.platforms(),
        manifest: package.manifest_text,
    })
}

fn json(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

/// The entry for `paths`, as pretty JSON in the index's own field order.
pub fn entry(paths: &[&Path]) -> Result<String, String> {
    if paths.is_empty() {
        return Err("give at least one package".to_owned());
    }
    let given: Vec<Given> = paths.iter().map(|p| read(p)).collect::<Result<_, _>>()?;
    let first = &given[0];
    if let Some(other) = given.iter().find(|g| g.manifest != first.manifest) {
        return Err(format!(
            "{} and {} were built from different manifests; pack them together with --split",
            first.file_name, other.file_name
        ));
    }
    let Summary {
        id,
        name,
        description,
        native,
        ..
    } = summarize(&first.manifest, "")?;

    // Each platform maps to the package that carries it.
    let mut platforms: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for package in &given {
        for platform in &package.platforms {
            if platforms
                .insert(platform, (&package.file_name, &package.sha256))
                .is_some()
            {
                return Err(format!("two packages carry a program for {platform}"));
            }
        }
    }

    let mut lines = vec![
        format!("  \"id\": {},", json(&id)),
        "  \"kind\": \"native\",".to_owned(),
        format!("  \"name\": {},", json(&name)),
        format!("  \"description\": {},", json(&description)),
        format!("  \"author\": {},", json(&native.author)),
        format!("  \"version\": {},", json(&native.version)),
        format!("  \"license\": {},", json(&native.license)),
        "  \"tags\": [\"native\"],".to_owned(),
    ];
    if let Some(min) = &native.min_sevak {
        lines.push(format!("  \"min_sevak\": {},", json(min)));
    }
    let permissions: Vec<String> = native.permissions.iter().map(|p| json(p)).collect();
    lines.push(format!("  \"permissions\": [{}],", permissions.join(", ")));
    if let Some(repository) = &native.repository {
        lines.push(format!("  \"repository\": {},", json(repository)));
    }
    if let Some(homepage) = native.homepage.as_ref().or(native.repository.as_ref()) {
        lines.push(format!("  \"homepage\": {},", json(homepage)));
    }
    lines.push("  \"platforms\": {".to_owned());
    let count = platforms.len();
    for (index, (platform, (file, hash))) in platforms.iter().enumerate() {
        lines.push(format!(
            "    {}: {{\"source\": {}, \"sha256\": {}}}{}",
            json(platform),
            json(&format!("gallery/extensions/{id}/{file}")),
            json(hash),
            if index + 1 < count { "," } else { "" }
        ));
    }
    lines.push("  }".to_owned());
    Ok(format!("{{\n{}\n}}", lines.join("\n")))
}

pub(crate) fn command(args: &[String], out: &mut dyn Write) -> Result<(), Failure> {
    let args = Args::parse(args, &[], &[])?;
    if args.positional.is_empty() {
        return Err(Failure::Usage(
            "entry needs at least one package".to_owned(),
        ));
    }
    let paths: Vec<&Path> = args.positional.iter().map(Path::new).collect();
    let text = entry(&paths)?;
    writeln!(out, "{text}").map_err(crate::io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sevak_plugins::extensions::Builder;

    use super::*;

    const MANIFEST: &str = "protocol = 1\nid = \"script:tool\"\nkeyword = \"t\"\nname = \"Tool\"\ndescription = \"A \\\"quoted\\\" tool.\"\n\
        [extension]\nversion = \"1.2.3\"\nauthor = \"Ada\"\nlicense = \"MIT\"\n\
        repository = \"https://github.com/example/tool\"\nmin_sevak = \"0.1.0\"\npermissions = [\"network\"]\n\
        [extension.binaries]\nwindows-x86_64 = \"bin/tool.exe\"\nlinux-x86_64 = \"bin/tool\"\n";

    fn write_split(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut builder = Builder::new(MANIFEST, "tool").unwrap();
        builder.add_binary("linux-x86_64", b"elf".to_vec()).unwrap();
        builder
            .add_binary("windows-x86_64", b"mz".to_vec())
            .unwrap();
        ["linux-x86_64", "windows-x86_64"]
            .iter()
            .map(|platform| {
                let path = dir.join(format!("tool-1.2.3-{platform}.sevakext"));
                fs::write(&path, builder.build(Some(platform)).unwrap()).unwrap();
                path
            })
            .collect()
    }

    #[test]
    fn the_entry_is_valid_json_the_index_parser_accepts() {
        let tmp = tempfile::tempdir().unwrap();
        let files = write_split(tmp.path());
        let paths: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();
        let text = entry(&paths).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["id"], "tool");
        assert_eq!(value["kind"], "native");
        assert_eq!(value["description"], "A \"quoted\" tool.");
        assert_eq!(value["permissions"][0], "network");
        assert_eq!(
            value["platforms"]["linux-x86_64"]["source"],
            "gallery/extensions/tool/tool-1.2.3-linux-x86_64.sevakext"
        );
        let hash = sha256_hex(&fs::read(&files[1]).unwrap());
        assert_eq!(
            value["platforms"]["windows-x86_64"]["sha256"],
            hash.as_str()
        );

        // Sevak's own index parser takes it.
        let index = serde_json::json!({"format": 2, "entries": [value]}).to_string();
        let pin = sevak_core::gallery_source::Pin::new("v1.2.3").unwrap();
        let parsed = sevak_plugins::workflow::gallery::parse_index(&index, &pin).unwrap();
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        assert_eq!(parsed.entries[0].platforms.len(), 2);
    }

    #[test]
    fn packages_from_different_manifests_are_not_one_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let files = write_split(tmp.path());
        let other = MANIFEST.replace("1.2.3", "1.2.4");
        let mut builder = Builder::new(&other, "tool").unwrap();
        builder.add_binary("linux-x86_64", b"elf".to_vec()).unwrap();
        let odd = tmp.path().join("odd.sevakext");
        fs::write(&odd, builder.build(None).unwrap()).unwrap();
        let err = entry(&[files[0].as_path(), odd.as_path()]).unwrap_err();
        assert!(err.contains("different manifests"), "{err}");
        // Two packages for one platform are ambiguous.
        let err = entry(&[files[0].as_path(), files[0].as_path()]).unwrap_err();
        assert!(err.contains("two packages"), "{err}");
        assert!(entry(&[]).is_err());
    }
}
