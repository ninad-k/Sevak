//! `sevak-ext init`: a project from `templates/rust-extension`.
//!
//! The template is also usable with `cargo generate` and by plain copying; the
//! markers below are `cargo generate`'s names, so one set of files serves all
//! three.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::{Args, Failure};

/// The template's files, embedded so the tool works from a plain
/// `cargo install`. `cargo-generate.toml` is `cargo generate`'s own and is not
/// part of a project.
const TEMPLATE: [(&str, &str); 5] = [
    (
        "Cargo.toml",
        include_str!("../../../templates/rust-extension/Cargo.toml"),
    ),
    (
        "plugin.toml",
        include_str!("../../../templates/rust-extension/plugin.toml"),
    ),
    (
        "README.md",
        include_str!("../../../templates/rust-extension/README.md"),
    ),
    (
        ".gitignore",
        include_str!("../../../templates/rust-extension/.gitignore"),
    ),
    (
        "src/main.rs",
        include_str!("../../../templates/rust-extension/src/main.rs"),
    ),
];

/// What a new project is called and says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Values {
    /// Lower case letters, digits and `-`: the crate name, the gallery id and
    /// the folder name.
    pub name: String,
    /// What users type to reach the extension.
    pub keyword: String,
    pub description: String,
    /// The publisher's name as users should see it.
    pub author: String,
}

impl Values {
    /// Checks every value is something the template can hold: plain text with
    /// no quotes, backslashes or line breaks (it ends up in TOML strings).
    pub fn check(&self) -> Result<(), String> {
        let name = &self.name;
        let valid_name = !name.is_empty()
            && name.len() <= 48
            && name.starts_with(|c: char| c.is_ascii_lowercase())
            && !name.ends_with('-')
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !valid_name {
            return Err(format!(
                "\"{name}\" is not a usable name: use lower case letters, digits and -, \
                 starting with a letter (it becomes the crate name and the gallery id)"
            ));
        }
        if self.keyword.is_empty() || self.keyword.chars().any(char::is_whitespace) {
            return Err("the keyword must be one word without spaces".to_owned());
        }
        for (label, text) in [
            ("keyword", &self.keyword),
            ("description", &self.description),
            ("author", &self.author),
        ] {
            if text
                .chars()
                .any(|c| c.is_control() || c == '"' || c == '\\')
            {
                return Err(format!(
                    "the {label} cannot contain quotes, backslashes or line breaks"
                ));
            }
        }
        if self.author.trim().is_empty() {
            return Err("the author (the publisher users see) cannot be empty".to_owned());
        }
        Ok(())
    }
}

/// `text` with the template's markers replaced.
pub fn render_template(text: &str, values: &Values) -> String {
    text.replace("{{project-name}}", &values.name)
        .replace("{{crate_name}}", &values.name.replace('-', "_"))
        .replace("{{keyword}}", &values.keyword)
        .replace("{{description}}", &values.description)
        .replace("{{authors}}", &values.author)
}

/// The template's files with the markers replaced.
fn rendered(values: &Values) -> Result<Vec<(&'static str, String)>, String> {
    TEMPLATE
        .iter()
        .map(|(path, text)| {
            let text = render_template(text, values);
            if text.contains("{{") {
                Err(format!("the template file {path} has an unknown marker"))
            } else {
                Ok((*path, text))
            }
        })
        .collect()
}

/// Writes the project into `dir` (which must not exist or be empty).
pub fn create(dir: &Path, values: &Values) -> Result<Vec<String>, String> {
    values.check()?;
    if dir.exists() {
        let empty = fs::read_dir(dir)
            .map_err(|err| format!("cannot read {}: {err}", dir.display()))?
            .next()
            .is_none();
        if !empty {
            return Err(format!("{} is not empty", dir.display()));
        }
    }
    let files = rendered(values)?;
    let mut written = Vec::new();
    for (path, text) in files {
        let target = dir.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
        }
        fs::write(&target, text)
            .map_err(|err| format!("cannot write {}: {err}", target.display()))?;
        written.push(path.to_owned());
    }
    Ok(written)
}

pub(crate) fn command(args: &[String], out: &mut dyn Write) -> Result<(), Failure> {
    let args = Args::parse(args, &["name", "keyword", "description", "author"], &[])?;
    let [dir] = args.positional.as_slice() else {
        return Err(Failure::Usage("init needs exactly one folder".to_owned()));
    };
    let dir = Path::new(dir);
    let name = match args.value("name") {
        Some(name) => name.to_owned(),
        None => dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_lowercase().replace(['_', ' '], "-"))
            .ok_or_else(|| Failure::Usage("give --name: the folder has no name".to_owned()))?,
    };
    let values = Values {
        keyword: args.value("keyword").unwrap_or("hello").to_owned(),
        description: args
            .value("description")
            .unwrap_or("Says hello.")
            .to_owned(),
        author: args
            .value("author")
            .map(str::to_owned)
            .or_else(|| std::env::var("USERNAME").ok())
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_else(|| "Your Name".to_owned()),
        name,
    };
    let written = create(dir, &values)?;
    writeln!(out, "Created {} ({}):", dir.display(), values.name).map_err(crate::io)?;
    for path in &written {
        writeln!(out, "  {path}").map_err(crate::io)?;
    }
    writeln!(
        out,
        "\nNext: edit src/main.rs, then `cargo build --release`, and read README.md \
         for trying it in Sevak and packaging it. Check `author`, `repository` and \
         `permissions` in plugin.toml before you publish."
    )
    .map_err(crate::io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(name: &str) -> Values {
        Values {
            name: name.to_owned(),
            keyword: "hi".to_owned(),
            description: "Says hi.".to_owned(),
            author: "Ada Lovelace".to_owned(),
        }
    }

    #[test]
    fn the_rendered_template_has_no_markers_left_and_a_valid_manifest() {
        let files = rendered(&values("my-ext")).unwrap();
        for (path, text) in &files {
            assert!(!text.contains("{{"), "{path}");
        }
        let manifest = &files.iter().find(|(p, _)| *p == "plugin.toml").unwrap().1;
        let native = sevak_plugins::extensions::check_manifest(manifest, "my-ext").unwrap();
        assert_eq!(native.author, "Ada Lovelace");
        assert!(native.binaries["linux-x86_64"].contains("my-ext-linux-x86_64"));
        let cargo = &files.iter().find(|(p, _)| *p == "Cargo.toml").unwrap().1;
        assert!(cargo.contains("name = \"my-ext\""));
    }

    #[test]
    fn the_template_main_is_the_examples_main() {
        let example = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rust-hello/src/main.rs"),
        )
        .unwrap();
        let template = TEMPLATE
            .iter()
            .find(|(p, _)| *p == "src/main.rs")
            .unwrap()
            .1;
        assert_eq!(
            template.replace("\r\n", "\n"),
            example.replace("\r\n", "\n"),
            "templates/rust-extension/src/main.rs must be examples/rust-hello/src/main.rs"
        );
    }

    #[test]
    fn creating_a_project_writes_the_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hello-ext");
        let written = create(&dir, &values("hello-ext")).unwrap();
        assert_eq!(written.len(), TEMPLATE.len());
        assert!(dir.join("src/main.rs").is_file());
        assert!(dir.join(".gitignore").is_file());
        // The result validates as a project.
        let text = fs::read_to_string(dir.join("plugin.toml")).unwrap();
        assert!(sevak_plugins::extensions::check_manifest(&text, "hello-ext").is_ok());
    }

    #[test]
    fn an_existing_folder_with_files_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("keep.txt"), "x").unwrap();
        let err = create(tmp.path(), &values("ext")).unwrap_err();
        assert!(err.contains("not empty"), "{err}");
        assert!(tmp.path().join("keep.txt").is_file());
        // An empty existing folder is fine.
        let empty = tmp.path().join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(create(&empty, &values("ext")).is_ok());
    }

    #[test]
    fn values_that_would_break_the_toml_are_refused() {
        for bad in ["", "My Ext", "9ext", "ext-", "Ext", &"x".repeat(49)] {
            assert!(values(bad).check().is_err(), "{bad:?}");
        }
        for (field, mut v) in [
            ("keyword", values("ext")),
            ("description", values("ext")),
            ("author", values("ext")),
        ] {
            let target = match field {
                "keyword" => &mut v.keyword,
                "description" => &mut v.description,
                _ => &mut v.author,
            };
            target.push_str("\" \nx = \"y");
            assert!(v.check().is_err(), "{field}");
        }
        let mut spaced = values("ext");
        spaced.keyword = "two words".to_owned();
        assert!(spaced.check().is_err());
        let mut nobody = values("ext");
        nobody.author = "  ".to_owned();
        assert!(nobody.check().is_err());
        assert!(values("ok-name-2").check().is_ok());
    }
}
