//! The starting points offered by "New from template" in the workflow builder.

use super::model::{
    Argument, Connection, Node, NodeKind, Test, TransformOp, Workflow, ELSE, OUT, THEN,
};

/// A workflow to start from, with the files it needs next to `workflow.toml`.
#[derive(Debug, Clone)]
pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub workflow: Workflow,
    /// `(path inside the workflow folder, contents)`.
    pub files: Vec<(&'static str, &'static str)>,
}

fn node(id: &str, title: &str, x: f64, y: f64, kind: NodeKind) -> Node {
    Node {
        id: id.to_owned(),
        title: title.to_owned(),
        x,
        y,
        kind,
    }
}

fn wire(from: &str, port: &str, to: &str) -> Connection {
    Connection {
        from: from.to_owned(),
        port: port.to_owned(),
        to: to.to_owned(),
    }
}

fn base(name: &str, description: &str) -> Workflow {
    Workflow {
        name: name.to_owned(),
        description: description.to_owned(),
        version: "1.0".to_owned(),
        ..Workflow::default()
    }
}

/// Every template, in the order the builder lists them.
pub fn all() -> Vec<Template> {
    vec![search_a_site(), open_url_with_query(), script_filter()]
}

pub fn find(id: &str) -> Option<Template> {
    all().into_iter().find(|template| template.id == id)
}

/// `wiki rust` opens a site's search for the text.
fn search_a_site() -> Template {
    let mut workflow = base(
        "Search a site",
        "Type the keyword and some text to search a website.",
    );
    workflow.nodes = vec![
        node(
            "keyword",
            "Search Wikipedia for {query}",
            40.0,
            80.0,
            NodeKind::Keyword {
                keyword: "wiki".to_owned(),
                argument: Argument::Required,
                subtitle: "Opens the search in your browser".to_owned(),
            },
        ),
        node(
            "open",
            "Open the search",
            340.0,
            80.0,
            NodeKind::OpenUrl {
                url: "https://en.wikipedia.org/w/index.php?search={query}".to_owned(),
            },
        ),
    ];
    workflow.connections = vec![wire("keyword", OUT, "open")];
    Template {
        id: "search-a-site",
        name: "Search a site",
        description: "A keyword that opens a website's search for what you type.",
        workflow,
        files: Vec::new(),
    }
}

/// `issue 14` opens issue 14; `issue some words` searches the issues.
fn open_url_with_query() -> Template {
    let mut workflow = base(
        "Open a URL with the query",
        "A number opens that issue; any other text searches the issues.",
    );
    workflow.nodes = vec![
        node(
            "keyword",
            "Open issue {query}",
            40.0,
            140.0,
            NodeKind::Keyword {
                keyword: "issue".to_owned(),
                argument: Argument::Required,
                subtitle: "A number opens it, words search".to_owned(),
            },
        ),
        node(
            "is-number",
            "Is it a number?",
            300.0,
            140.0,
            NodeKind::Conditional {
                left: "{query}".to_owned(),
                test: Test::Matches,
                right: "^#?[0-9]+$".to_owned(),
                ignore_case: false,
            },
        ),
        node(
            "strip",
            "Drop a leading #",
            580.0,
            60.0,
            NodeKind::Transform {
                op: TransformOp::RegexReplace,
                input: "{query}".to_owned(),
                find: "^#".to_owned(),
                replace: String::new(),
                index: 0,
                into: None,
            },
        ),
        node(
            "open-issue",
            "Open the issue",
            860.0,
            60.0,
            NodeKind::OpenUrl {
                url: "https://github.com/ninad-k/Sevak/issues/{query}".to_owned(),
            },
        ),
        node(
            "search",
            "Search the issues",
            580.0,
            240.0,
            NodeKind::OpenUrl {
                url: "https://github.com/ninad-k/Sevak/issues?q={query}".to_owned(),
            },
        ),
    ];
    workflow.connections = vec![
        wire("keyword", OUT, "is-number"),
        wire("is-number", THEN, "strip"),
        wire("strip", OUT, "open-issue"),
        wire("is-number", ELSE, "search"),
    ];
    Template {
        id: "open-url-with-query",
        name: "Open a URL with the query",
        description: "Branches on what you type: a number opens an issue, words search.",
        workflow,
        files: Vec::new(),
    }
}

const FILTER_PY: &str = r#"#!/usr/bin/env python3
"""A starting point for a Sevak workflow script filter.

Sevak runs this script whenever the text after the keyword changes and passes
the text as the first argument. Print Alfred Script Filter JSON:
https://www.alfredapp.com/help/workflows/inputs/script-filter/json/

Picking a row continues the workflow with the row's "arg" (and its "variables").
Holding Alt while picking uses the "alt" entry of "mods" instead.
"""
import json
import sys

query = sys.argv[1].strip() if len(sys.argv) > 1 else ""

FRUIT = ["Apple", "Banana", "Cherry", "Date", "Elderberry", "Fig", "Grape"]
items = [
    {
        "uid": name.lower(),
        "title": name,
        "subtitle": "Enter picks it; Alt+Enter picks it with a note",
        "arg": name,
        "autocomplete": name,
        "mods": {"alt": {"arg": name + " (with a note)", "subtitle": "Pick it with a note"}},
    }
    for name in FRUIT
    if query.lower() in name.lower()
]
if not items:
    items = [{"title": "Nothing matches", "subtitle": "Try another word", "valid": False}]

print(json.dumps({"items": items}))
"#;

/// `fruit ch` lists matches from a script; picking one shows a notification.
fn script_filter() -> Template {
    let mut workflow = base(
        "Script filter",
        "A keyword whose results come from a script (Python 3 here).",
    );
    workflow.nodes = vec![
        node(
            "filter",
            "Pick a fruit",
            40.0,
            100.0,
            NodeKind::ScriptFilter {
                keyword: "fruit".to_owned(),
                command: Vec::new(),
                script: Some("filter.py".to_owned()),
                args: Vec::new(),
                timeout_ms: None,
                hard_timeout_ms: None,
            },
        ),
        node(
            "notify",
            "Say what was picked",
            340.0,
            100.0,
            NodeKind::Notification {
                heading: "You picked".to_owned(),
                body: "{query}".to_owned(),
            },
        ),
    ];
    workflow.connections = vec![wire("filter", OUT, "notify")];
    Template {
        id: "script-filter",
        name: "Script filter",
        description: "Results from your own script; picking one runs the next steps.",
        workflow,
        files: vec![("filter.py", FILTER_PY)],
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn there_are_three_valid_templates() {
        let templates = all();
        assert_eq!(templates.len(), 3);
        let ids: HashSet<_> = templates.iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), 3);
        for template in &templates {
            let problems = template.workflow.validate();
            assert_eq!(problems, [], "{}", template.id);
            assert!(!template.name.is_empty() && !template.description.is_empty());
            // What the builder writes is what loads again.
            let text = template.workflow.to_toml().unwrap();
            assert_eq!(Workflow::from_toml(&text).unwrap(), template.workflow);
        }
    }

    #[test]
    fn the_script_filter_template_ships_its_script() {
        let template = find("script-filter").unwrap();
        assert!(template.workflow.needs_approval());
        assert_eq!(template.files.len(), 1);
        assert_eq!(template.files[0].0, "filter.py");
        assert!(template.files[0].1.contains("json.dumps"));
        // The other two run nothing, so they need no approval.
        assert!(!find("search-a-site").unwrap().workflow.needs_approval());
        assert!(!find("open-url-with-query")
            .unwrap()
            .workflow
            .needs_approval());
        assert!(find("nope").is_none());
    }

    #[test]
    fn the_branching_template_really_branches() {
        use crate::workflow::exec::Ctx;
        use crate::workflow::testing::fixture;

        let template = find("open-url-with-query").unwrap();
        let run = |query: &str| {
            let f = fixture(
                template.workflow.nodes.clone(),
                template.workflow.connections.clone(),
            );
            let report = f.runtime.run_blocking("keyword", Ctx::with_arg(query));
            assert!(report.errors.is_empty(), "{report:?}");
            let urls = f.platform.opened_urls.lock().unwrap().clone();
            urls
        };
        assert_eq!(run("#14"), ["https://github.com/ninad-k/Sevak/issues/14"]);
        assert_eq!(run("14"), ["https://github.com/ninad-k/Sevak/issues/14"]);
        assert_eq!(
            run("dark mode"),
            ["https://github.com/ninad-k/Sevak/issues?q=dark%20mode"]
        );
    }

    #[test]
    fn the_search_template_encodes_the_query() {
        use crate::workflow::exec::Ctx;
        use crate::workflow::testing::fixture;

        let template = find("search-a-site").unwrap();
        let f = fixture(
            template.workflow.nodes.clone(),
            template.workflow.connections.clone(),
        );
        f.runtime
            .run_blocking("keyword", Ctx::with_arg("rust & c++"));
        assert_eq!(
            *f.platform.opened_urls.lock().unwrap(),
            ["https://en.wikipedia.org/w/index.php?search=rust%20%26%20c%2B%2B"]
        );
    }
}
