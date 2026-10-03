//! Helpers shared by the unit tests of the workflow modules.

use std::sync::{Arc, Mutex};

use sevak_core::Config;

use super::exec::{OutputSink, Runtime};
use super::model::{Argument, Connection, Node, NodeKind, Workflow, OUT};
use crate::test_util::MockPlatform;

/// Records everything a workflow shows.
#[derive(Default)]
pub struct RecordingSink {
    pub notes: Mutex<Vec<(String, String)>>,
    pub large: Mutex<Vec<String>>,
    pub views: Mutex<Vec<(String, String)>>,
    /// What `confirm` answers.
    pub answer: bool,
    pub asked: Mutex<Vec<String>>,
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
        self.views
            .lock()
            .unwrap()
            .push((heading.to_owned(), text.to_owned()));
    }

    fn confirm(&self, question: &str) -> bool {
        self.asked.lock().unwrap().push(question.to_owned());
        self.answer
    }
}

pub fn node(id: &str, kind: NodeKind) -> Node {
    Node {
        id: id.into(),
        title: String::new(),
        x: 0.0,
        y: 0.0,
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
