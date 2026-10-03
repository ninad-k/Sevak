//! Which keyword belongs to whom.
//!
//! Settings refuses two built-in searches (or a built-in one and a web engine)
//! that share a keyword. Workflows and script plugins come from folders the
//! user manages, so a clash there is only a warning: both plugins answer the
//! keyword and both show results. [`KeywordOwners`] is the one list of every
//! keyword in use that those checks share. Keywords are compared
//! case-insensitively; an empty keyword (a search turned off) is not in use.

use sevak_core::Config;

use crate::script::ScriptPluginHost;
use crate::workflow::WorkflowHost;

/// Keywords of built-in plugins that cannot be changed, and what they open.
pub const FIXED_KEYWORDS: &[(&str, &str)] = &[
    (">", "terminal commands"),
    ("cb", "clipboard history"),
    ("s", "snippets"),
    ("emoji", "the emoji picker"),
    (":", "the emoji picker"),
    ("@", "contacts"),
    ("uuid", "the UUID generator"),
];

/// A keyword the user can change in `[files]`, `[bookmarks]`, `[tasks]`,
/// `[media]`, `[contacts]`, `[onepassword]` or `[dictionary]`.
#[derive(Debug, Clone, Copy)]
pub struct ConfigurableKeyword<'a> {
    /// How an error message names the setting ("files", "dictionary").
    pub label: &'static str,
    pub keyword: &'a str,
    /// How a message names whatever answers it ("the files search").
    pub owner: &'static str,
}

/// Every configurable built-in keyword, as the config has it (untrimmed, and
/// empty when the search is off).
pub fn configurable_keywords(config: &Config) -> [ConfigurableKeyword<'_>; 10] {
    let entry = |label, keyword, owner| ConfigurableKeyword {
        label,
        keyword,
        owner,
    };
    [
        entry("files", &config.files.keyword, "the files search"),
        entry(
            "whole-disk file search",
            &config.files.index_keyword,
            "the whole-disk file search",
        ),
        entry(
            "file contents search",
            &config.files.content_keyword,
            "the file contents search",
        ),
        entry("bookmarks", &config.bookmarks.keyword, "bookmarks"),
        entry(
            "automation tasks",
            &config.tasks.keyword,
            "automation tasks",
        ),
        entry("media controls", &config.media.keyword, "media controls"),
        entry("contacts", &config.contacts.keyword, "contacts"),
        entry("1Password", &config.onepassword.keyword, "1Password"),
        entry(
            "dictionary",
            &config.dictionary.define_keyword,
            "the dictionary",
        ),
        entry(
            "spelling",
            &config.dictionary.spell_keyword,
            "the spelling checker",
        ),
    ]
}

/// What kind of thing answers a keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerKind {
    Builtin,
    WebSearch,
    Workflow,
    ScriptPlugin,
}

/// One keyword and the thing that answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordUse {
    /// Trimmed and lowercased.
    pub keyword: String,
    pub kind: OwnerKind,
    /// Identifies the owner, so a workflow being edited is not "another"
    /// owner of its own keywords.
    pub key: String,
    /// How a message names the owner ("web search Google", "workflow Docs").
    pub owner: String,
}

impl KeywordUse {
    pub fn new(
        keyword: &str,
        kind: OwnerKind,
        key: impl Into<String>,
        owner: impl Into<String>,
    ) -> Self {
        Self {
            keyword: keyword.trim().to_lowercase(),
            kind,
            key: key.into(),
            owner: owner.into(),
        }
    }
}

/// The key of the workflow in `folder`.
pub fn workflow_key(folder: &str) -> String {
    format!("workflow:{folder}")
}

/// Every keyword in use, with its owner or owners.
#[derive(Debug, Clone, Default)]
pub struct KeywordOwners {
    uses: Vec<KeywordUse>,
}

impl KeywordOwners {
    /// The built-in keywords (fixed and configurable ones) and the web search
    /// engines of `config`.
    pub fn builtin(config: &Config) -> Self {
        let mut owners = Self::default();
        for (keyword, owner) in FIXED_KEYWORDS {
            owners.push(KeywordUse::new(
                keyword,
                OwnerKind::Builtin,
                format!("builtin:{keyword}"),
                *owner,
            ));
        }
        for entry in configurable_keywords(config) {
            owners.push(KeywordUse::new(
                entry.keyword,
                OwnerKind::Builtin,
                format!("builtin:{}", entry.label),
                entry.owner,
            ));
        }
        for engine in &config.web_search {
            owners.push(KeywordUse::new(
                &engine.keyword,
                OwnerKind::WebSearch,
                format!("web:{}", engine.keyword.trim().to_lowercase()),
                format!("web search {}", engine.name.trim()),
            ));
        }
        owners
    }

    /// Everything in use now: the built-ins, the enabled workflows and the
    /// enabled script plugins.
    pub fn collect(config: &Config, scripts: &ScriptPluginHost, workflows: &WorkflowHost) -> Self {
        let mut owners = Self::builtin(config);
        owners.extend(scripts.keyword_uses(config));
        owners.extend(workflows.keyword_uses(config));
        owners
    }

    /// Adds one keyword. An empty one (a search that is off) is ignored.
    pub fn push(&mut self, usage: KeywordUse) {
        if !usage.keyword.is_empty() {
            self.uses.push(usage);
        }
    }

    pub fn extend(&mut self, uses: impl IntoIterator<Item = KeywordUse>) {
        for usage in uses {
            self.push(usage);
        }
    }

    /// Who answers `keyword`, except the owner `own_key`; each owner once.
    pub fn owners_of(&self, keyword: &str, own_key: Option<&str>) -> Vec<&str> {
        let keyword = keyword.trim().to_lowercase();
        let mut found: Vec<&str> = Vec::new();
        for usage in &self.uses {
            if usage.keyword == keyword
                && Some(usage.key.as_str()) != own_key
                && !found.contains(&usage.owner.as_str())
            {
                found.push(&usage.owner);
            }
        }
        found
    }

    /// The warning for each of `keywords` (used by the owner `own_key`) that
    /// someone else answers too: `(keyword, message)`, in the order given.
    pub fn warnings_for<'a>(
        &self,
        own_key: Option<&str>,
        keywords: impl IntoIterator<Item = &'a str>,
    ) -> Vec<(String, String)> {
        let mut seen: Vec<String> = Vec::new();
        let mut warnings = Vec::new();
        for keyword in keywords {
            let keyword = keyword.trim();
            let lower = keyword.to_lowercase();
            if lower.is_empty() || seen.contains(&lower) {
                continue;
            }
            seen.push(lower);
            let others = self.owners_of(keyword, own_key);
            if !others.is_empty() {
                warnings.push((keyword.to_owned(), clash_message(keyword, &others)));
            }
        }
        warnings
    }

    /// Keywords with more than one owner where a workflow or a script plugin
    /// is one of them (clashes between built-ins are errors in Settings):
    /// `(keyword, owners)`.
    pub fn shared(&self) -> Vec<(String, Vec<String>)> {
        let mut seen: Vec<&str> = Vec::new();
        let mut shared = Vec::new();
        for usage in &self.uses {
            if seen.contains(&usage.keyword.as_str()) {
                continue;
            }
            seen.push(&usage.keyword);
            let same: Vec<&KeywordUse> = self
                .uses
                .iter()
                .filter(|other| other.keyword == usage.keyword)
                .collect();
            let user_made = same
                .iter()
                .any(|u| matches!(u.kind, OwnerKind::Workflow | OwnerKind::ScriptPlugin));
            let mut keys: Vec<&str> = same.iter().map(|u| u.key.as_str()).collect();
            keys.sort_unstable();
            keys.dedup();
            if user_made && keys.len() > 1 {
                let mut owners: Vec<String> = Vec::new();
                for u in same {
                    if !owners.contains(&u.owner) {
                        owners.push(u.owner.clone());
                    }
                }
                shared.push((usage.keyword.clone(), owners));
            }
        }
        shared
    }

    /// Logs each shared keyword once. Called when the plugins load.
    pub fn log_shared(&self) {
        for (keyword, owners) in self.shared() {
            tracing::warn!(
                keyword,
                "keyword \"{keyword}\" is used by {}; all of them show results",
                join_names(&owners)
            );
        }
    }
}

/// The warning shown for a keyword that `others` answer too.
pub fn clash_message(keyword: &str, others: &[&str]) -> String {
    format!(
        "Keyword \"{keyword}\" is also used by {}; both will show results.",
        join_names(others)
    )
}

/// "a", "a and b", "a, b and c".
fn join_names<S: AsRef<str>>(names: &[S]) -> String {
    match names {
        [] => String::new(),
        [only] => only.as_ref().to_owned(),
        [rest @ .., last] => format!(
            "{} and {}",
            rest.iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>()
                .join(", "),
            last.as_ref()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow(keyword: &str, folder: &str, name: &str) -> KeywordUse {
        KeywordUse::new(
            keyword,
            OwnerKind::Workflow,
            workflow_key(folder),
            format!("workflow {name}"),
        )
    }

    fn script(keyword: &str, name: &str) -> KeywordUse {
        KeywordUse::new(
            keyword,
            OwnerKind::ScriptPlugin,
            format!("script:{name}"),
            format!("script plugin {name}"),
        )
    }

    #[test]
    fn a_keyword_of_a_built_in_plugin_is_found() {
        let owners = KeywordOwners::builtin(&Config::default());
        assert_eq!(owners.owners_of("f", None), ["the files search"]);
        assert_eq!(owners.owners_of(">", None), ["terminal commands"]);
        assert_eq!(owners.owners_of("define", None), ["the dictionary"]);
    }

    #[test]
    fn a_web_engine_is_named_with_its_name() {
        let owners = KeywordOwners::builtin(&Config::default());
        assert_eq!(owners.owners_of("g", None), ["web search Google"]);
    }

    #[test]
    fn matching_ignores_case_and_spaces_around_the_keyword() {
        let owners = KeywordOwners::builtin(&Config::default());
        assert_eq!(owners.owners_of("  G ", None), ["web search Google"]);
        assert_eq!(owners.owners_of("EMOJI", None), ["the emoji picker"]);
    }

    #[test]
    fn a_keyword_nobody_uses_has_no_owner() {
        let owners = KeywordOwners::builtin(&Config::default());
        assert!(owners.owners_of("zzqx", None).is_empty());
        assert!(owners.warnings_for(None, ["zzqx", ""]).is_empty());
    }

    #[test]
    fn an_empty_built_in_keyword_is_not_in_use() {
        let mut config = Config::default();
        config.files.keyword = String::new();
        let owners = KeywordOwners::builtin(&config);
        assert!(owners.owners_of("", None).is_empty());
        assert!(owners.owners_of("f", None).is_empty());
    }

    #[test]
    fn workflows_and_script_plugins_are_owners_too() {
        let mut owners = KeywordOwners::builtin(&Config::default());
        owners.extend([
            workflow("DDG", "ddg", "DuckDuckGo"),
            script("hello", "hello"),
        ]);
        assert_eq!(owners.owners_of("ddg", None), ["workflow DuckDuckGo"]);
        assert_eq!(owners.owners_of("Hello", None), ["script plugin hello"]);
    }

    #[test]
    fn the_owner_itself_is_not_another_owner() {
        let mut owners = KeywordOwners::builtin(&Config::default());
        owners.extend([workflow("docs", "docs", "Docs")]);
        assert!(owners
            .owners_of("docs", Some(&workflow_key("docs")))
            .is_empty());
        assert_eq!(owners.owners_of("docs", Some("workflow:other")).len(), 1);
    }

    #[test]
    fn the_warning_names_the_other_owner() {
        let owners = KeywordOwners::builtin(&Config::default());
        let warnings = owners.warnings_for(None, ["g"]);
        assert_eq!(
            warnings,
            [(
                "g".to_owned(),
                "Keyword \"g\" is also used by web search Google; both will show results."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn several_other_owners_are_all_named() {
        let mut owners = KeywordOwners::builtin(&Config::default());
        owners.extend([workflow("x", "a", "A"), script("x", "b")]);
        let warnings = owners.warnings_for(None, ["x"]);
        assert_eq!(
            warnings[0].1,
            "Keyword \"x\" is also used by workflow A and script plugin b; both will show \
             results."
        );
    }

    #[test]
    fn shared_lists_only_clashes_that_involve_user_made_plugins() {
        let mut owners = KeywordOwners::builtin(&Config::default());
        assert!(owners.shared().is_empty());
        owners.extend([
            workflow("g", "mine", "Mine"),
            workflow("solo", "solo", "Solo"),
            script("solo", "solo"),
        ]);
        let shared = owners.shared();
        assert_eq!(shared.len(), 2);
        assert_eq!(shared[0].0, "g");
        assert_eq!(shared[0].1, ["web search Google", "workflow Mine"]);
        assert_eq!(shared[1].1, ["workflow Solo", "script plugin solo"]);
    }

    // ---- with real folders ------------------------------------------------

    use std::fs;
    use std::sync::Arc;

    use crate::workflow::{NoSink, Workflow};

    fn write_workflow(root: &std::path::Path, folder: &str, name: &str, keyword: &str) {
        let dir = root.join("config").join("workflows").join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("workflow.toml"),
            format!(
                "name = \"{name}\"
[[node]]
id = \"k\"
type = \"keyword\"
                 keyword = \"{keyword}\"
[[node]]
id = \"o\"
type = \"open_url\"
                 url = \"https://example.com/{{query}}\"
[[connection]]
from = \"k\"
to = \"o\"
"
            ),
        )
        .unwrap();
    }

    fn hosts(root: &std::path::Path) -> (ScriptPluginHost, WorkflowHost) {
        (
            ScriptPluginHost::new(
                root.join("config").join("plugins"),
                root.join("data").join("plugins"),
                root.join("data").join("approvals.json"),
            ),
            WorkflowHost::new(
                root.join("config").join("workflows"),
                root.join("data").join("workflows"),
                root.join("data").join("approvals.json"),
                Arc::new(NoSink),
            ),
        )
    }

    #[test]
    fn clashes_between_folders_are_found_and_stay_warnings() {
        let root = tempfile::tempdir().unwrap();
        let plugin = root.path().join("config").join("plugins").join("hello");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(
            plugin.join("plugin.toml"),
            "protocol = 1
name = \"Hello\"
keyword = \"hello\"
command = [\"prog\"]
",
        )
        .unwrap();
        write_workflow(root.path(), "duck", "DuckDuckGo", "G");
        write_workflow(root.path(), "greet", "Greeter", "HELLO");
        write_workflow(root.path(), "twin", "Twin", "greeter-two");
        write_workflow(root.path(), "twin2", "Twin Too", "greeter-two");
        write_workflow(root.path(), "free", "Free", "nothing-uses-this");
        let (scripts, workflows) = hosts(root.path());
        let config = Config::default();
        let owners = KeywordOwners::collect(&config, &scripts, &workflows);

        let load = |folder: &str| workflows.load(folder).unwrap().workflow;
        let messages = |folder: &str| -> Vec<String> {
            load(folder)
                .keyword_problems(&owners, Some(&workflow_key(folder)))
                .into_iter()
                .map(|problem| problem.message)
                .collect()
        };
        assert_eq!(
            messages("duck"),
            ["Keyword \"G\" is also used by web search Google; both will show results."]
        );
        assert_eq!(
            messages("greet"),
            ["Keyword \"HELLO\" is also used by script plugin Hello; both will show results."]
        );
        assert_eq!(
            messages("twin"),
            ["Keyword \"greeter-two\" is also used by workflow Twin Too; both will show results."]
        );
        assert_eq!(
            messages("twin2"),
            ["Keyword \"greeter-two\" is also used by workflow Twin; both will show results."]
        );
        assert!(messages("free").is_empty());

        // The clash is a warning: the workflow is still valid and loads.
        let duck = load("duck");
        assert!(duck.is_valid());
        let problems = duck.keyword_problems(&owners, Some(&workflow_key("duck")));
        assert!(problems.iter().all(|p| !p.is_error()));
        assert_eq!(problems[0].node.as_deref(), Some("k"));
        assert!(workflows
            .scan()
            .iter()
            .all(|scanned| matches!(scanned, crate::workflow::Scanned::Workflow(_))));

        // The settings list counts the warnings and says what they are.
        let rows = workflows.summaries(&config, &owners);
        let duck_row = rows.iter().find(|row| row.folder == "duck").unwrap();
        assert_eq!(duck_row.warnings, 1);
        assert_eq!(duck_row.keyword_warnings.len(), 1);
        let free_row = rows.iter().find(|row| row.folder == "free").unwrap();
        assert_eq!(free_row.warnings, 0);
        assert!(free_row.keyword_warnings.is_empty());

        // The script plugin's Settings description names the workflow.
        let catalog = scripts.catalog(&config, &owners);
        assert!(
            catalog[0].description.contains(
                "Keyword \"hello\" is also used by workflow Greeter; both will show results."
            ),
            "{}",
            catalog[0].description
        );

        // The log line is for the clashes that involve a workflow or a plugin.
        let shared: Vec<String> = owners.shared().into_iter().map(|(k, _)| k).collect();
        assert_eq!(shared, ["g", "hello", "greeter-two"]);
    }

    #[test]
    fn a_disabled_workflow_clashes_with_nothing() {
        let root = tempfile::tempdir().unwrap();
        write_workflow(root.path(), "duck", "DuckDuckGo", "g");
        let (scripts, workflows) = hosts(root.path());
        let mut config = Config::default();
        config.plugins.disabled = vec!["workflow:duck".into()];
        let owners = KeywordOwners::collect(&config, &scripts, &workflows);
        assert_eq!(owners.owners_of("g", None), ["web search Google"]);
        assert!(owners.shared().is_empty());
        // An unsaved workflow with the same keyword is not warned about itself.
        let unsaved = Workflow::from_toml(
            "name = \"X\"
[[node]]
id = \"k\"
type = \"keyword\"
keyword = \"zq\"
",
        )
        .unwrap();
        assert!(unsaved.keyword_problems(&owners, None).is_empty());
    }
}
