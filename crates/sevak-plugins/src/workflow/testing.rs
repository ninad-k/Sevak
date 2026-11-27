//! Helpers shared by the unit tests of the workflow modules.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use sevak_core::Config;

use super::exec::{OutputSink, Runtime, UiHost, UiOutcome, UiSession, UiUpdate};
use super::model::{Argument, Connection, Node, NodeKind, Workflow, OUT};
use crate::test_util::MockPlatform;

/// Records everything a workflow shows.
#[derive(Default)]
pub struct RecordingSink {
    pub notes: Mutex<Vec<(String, String)>>,
    pub large: Mutex<Vec<String>>,
    pub views: Mutex<Vec<(String, String)>>,
    /// Whether each text view was flagged as Markdown, in order.
    pub view_markdown: Mutex<Vec<bool>>,
    /// What `confirm` answers.
    pub answer: bool,
    pub asked: Mutex<Vec<String>>,
    /// The button `ask` answers with; `None` is a dismissed dialog.
    pub button: Option<usize>,
    /// `(heading, message, buttons)` of every dialog shown.
    pub dialogs: Mutex<Vec<(String, String, Vec<String>)>>,
    /// What each `show_ui` does, in order: scripts to run through the host,
    /// then how the UI ends. With none left, the UI is "unsupported".
    pub ui_script: Mutex<VecDeque<UiScripted>>,
    pub ui_sessions: Mutex<Vec<UiSession>>,
    /// What each script the UI ran answered.
    pub ui_updates: Mutex<Vec<Result<UiUpdate, String>>>,
}

/// One scripted `show_ui`: the user's clicks, as far as the workflow sees them.
pub struct UiScripted {
    /// `(script id, stdin JSON)` the user's buttons run while the UI is open.
    pub runs: Vec<(String, serde_json::Value)>,
    pub outcome: UiOutcome,
}

impl OutputSink for RecordingSink {
    fn notify(&self, heading: &str, body: &str) {
        self.notes
            .lock()
            .unwrap()
            .push((heading.to_owned(), body.to_owned()));
    }

    fn large_type(&self, text: &str) {
        self.large.lock().unwrap().push(text.to_owned());
    }

    fn text_view(&self, heading: &str, text: &str) {
        self.text_view_markdown(heading, text, false);
    }

    fn text_view_markdown(&self, heading: &str, text: &str, markdown: bool) {
        self.views
            .lock()
            .unwrap()
            .push((heading.to_owned(), text.to_owned()));
        self.view_markdown.lock().unwrap().push(markdown);
    }

    fn confirm(&self, question: &str) -> bool {
        self.asked.lock().unwrap().push(question.to_owned());
        self.answer
    }

    fn ask(&self, heading: &str, message: &str, buttons: &[String]) -> Option<usize> {
        self.dialogs.lock().unwrap().push((
            heading.to_owned(),
            message.to_owned(),
            buttons.to_vec(),
        ));
        self.button
    }

    fn show_ui(&self, session: &UiSession, host: &dyn UiHost) -> Option<UiOutcome> {
        self.ui_sessions.lock().unwrap().push(session.clone());
        let scripted = self.ui_script.lock().unwrap().pop_front()?;
        for (id, input) in &scripted.runs {
            let update = host.run_script(id, input);
            self.ui_updates.lock().unwrap().push(update);
        }
        Some(scripted.outcome)
    }
}

pub fn node(id: &str, kind: NodeKind) -> Node {
    Node {
        id: id.into(),
        title: String::new(),
        x: 0.0,
        y: 0.0,
        note: String::new(),
        disabled: false,
        kind,
    }
}

pub fn wire(from: &str, to: &str) -> Connection {
    wire_port(from, OUT, to)
}

pub fn wire_port(from: &str, port: &str, to: &str) -> Connection {
    Connection {
        from: from.into(),
        port: port.into(),
        to: to.into(),
        disabled: false,
    }
}

pub fn keyword(id: &str) -> Node {
    keyword_with(id, "go", Argument::Optional)
}

pub fn keyword_with(id: &str, keyword: &str, argument: Argument) -> Node {
    node(
        id,
        NodeKind::Keyword {
            keyword: keyword.into(),
            argument,
            subtitle: String::new(),
        },
    )
}

pub fn copy(id: &str, text: &str) -> Node {
    node(id, NodeKind::Copy { text: text.into() })
}

pub fn set_var(id: &str, name: &str, value: &str) -> Node {
    node(
        id,
        NodeKind::SetVariable {
            name: name.into(),
            value: value.into(),
        },
    )
}

/// A workflow that is ready to run against a recording platform and sink.
pub struct Fixture {
    pub runtime: Arc<Runtime>,
    pub platform: Arc<MockPlatform>,
    pub sink: Arc<RecordingSink>,
    pub _dir: tempfile::TempDir,
}

pub fn fixture(nodes: Vec<Node>, connections: Vec<Connection>) -> Fixture {
    fixture_with(nodes, connections, RecordingSink::default())
}

pub fn fixture_with(
    nodes: Vec<Node>,
    connections: Vec<Connection>,
    sink: RecordingSink,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let platform = MockPlatform::empty();
    let sink = Arc::new(sink);
    let workflow = Workflow {
        name: "Test flow".into(),
        nodes,
        connections,
        ..Workflow::default()
    };
    assert!(workflow.is_valid(), "{:?}", workflow.validate());
    let runtime = Runtime::new(
        "test-flow",
        dir.path().join("wf"),
        dir.path().join("data"),
        workflow,
        platform.clone(),
        sink.clone(),
        &Config::default(),
    );
    Fixture {
        runtime,
        platform,
        sink,
        _dir: dir,
    }
}

pub fn clipboard(fixture: &Fixture) -> Vec<String> {
    fixture.platform.clipboard.lock().unwrap().clone()
}
