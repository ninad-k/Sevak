//! The protocol loop and the entry points.

use std::collections::VecDeque;
use std::io::{self, BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc;
use std::thread;

use serde_json::{json, Value};

use crate::error::Error;
use crate::item::{Item, MAX_ITEMS};
use crate::protocol::{
    error_line, parse_line, ready_line, results_line, Execute, Incoming, Initialize,
};
use crate::query::Query;

type QueryFn = Box<dyn FnMut(&Query) -> Result<Vec<Item>, Error>>;
type ExecuteFn = Box<dyn FnMut(&Execute) -> Result<(), Error>>;
type InitializeFn = Box<dyn FnMut(&Initialize)>;

/// Writes a line to stderr. Sevak captures an extension's stderr into its log,
/// tagged with the extension's id; stdout is reserved for the protocol, so never
/// print diagnostics there.
pub fn log(message: impl AsRef<str>) {
    eprintln!("{}", message.as_ref());
}

/// Runs a persistent extension that answers queries with `query`, until Sevak
/// sends `shutdown` or closes stdin.
///
/// ```no_run
/// use sevak_extension_sdk::{run, Item};
///
/// fn main() {
///     run(|query| Ok(vec![Item::new(query.text().to_uppercase()).copy_on_enter()]));
/// }
/// ```
pub fn run<F>(query: F)
where
    F: FnMut(&Query) -> Result<Vec<Item>, Error> + 'static,
{
    Extension::new(query).run();
}

/// Runs a one-shot extension (`mode = "oneshot"` in the manifest): Sevak starts
/// the program for every query with the text as the last argument and reads one
/// `{"items": [...]}` document from stdout. Prefer [`run`] unless you have a
/// reason; a persistent process answers without paying the start-up cost for
/// each key press.
///
/// On an error the message goes to stderr and the exit code is 1.
pub fn run_oneshot<F>(mut query: F)
where
    F: FnMut(&Query) -> Result<Vec<Item>, Error>,
{
    // The query is the last argument; no argument is an empty query.
    let args: Vec<String> = std::env::args().collect();
    let input = if args.len() > 1 {
        args.last().cloned().unwrap_or_default()
    } else {
        String::new()
    };
    match oneshot_document(&mut query, &input) {
        Ok(document) => {
            let mut stdout = io::stdout().lock();
            let _ = writeln!(stdout, "{document}");
            let _ = stdout.flush();
        }
        Err(err) => {
            log(err.message());
            std::process::exit(1);
        }
    }
}

/// The one-shot document for `input`.
fn oneshot_document<F>(query: &mut F, input: &str) -> Result<String, Error>
where
    F: FnMut(&Query) -> Result<Vec<Item>, Error>,
{
    let items = query(&Query::new(input))?;
    Ok(json!({"items": items_to_values(&items)}).to_string())
}

/// A persistent extension: the query handler, and optionally handlers for
/// `execute` (the user picked a row with [`Action::Custom`](crate::Action::Custom))
/// and `initialize`.
///
/// ```no_run
/// use sevak_extension_sdk::{Action, Extension, Item};
///
/// fn main() {
///     Extension::new(|query| {
///         Ok(vec![Item::new(format!("Remember: {}", query.text()))
///             .action(Action::custom(query.text()))])
///     })
///     .on_execute(|execute| {
///         // Save `execute.payload` somewhere; there is no reply to send.
///         eprintln!("remembering {}", execute.payload);
///         Ok(())
///     })
///     .run();
/// }
/// ```
pub struct Extension {
    query: QueryFn,
    execute: Option<ExecuteFn>,
    initialize: Option<InitializeFn>,
}

impl Extension {
    /// An extension that answers every query with `query`.
    pub fn new<F>(query: F) -> Self
    where
        F: FnMut(&Query) -> Result<Vec<Item>, Error> + 'static,
    {
        Self {
            query: Box::new(query),
            execute: None,
            initialize: None,
        }
    }

    /// Handles rows whose action is [`Action::Custom`](crate::Action::Custom).
    /// Sevak does not wait for a reply; an `Err` is logged by Sevak.
    #[must_use]
    pub fn on_execute<F>(mut self, handler: F) -> Self
    where
        F: FnMut(&Execute) -> Result<(), Error> + 'static,
    {
        self.execute = Some(Box::new(handler));
        self
    }

    /// Called once with Sevak's `initialize` message, before the first query.
    #[must_use]
    pub fn on_initialize<F>(mut self, handler: F) -> Self
    where
        F: FnMut(&Initialize) + 'static,
    {
        self.initialize = Some(Box::new(handler));
        self
    }

    /// Serves Sevak on stdin and stdout until it says `shutdown` or closes the
    /// pipe.
    pub fn run(self) {
        let stdin = io::stdin();
        // The reader thread owns a lock for the life of the process.
        let reader = io::BufReader::new(stdin);
        if let Err(err) = self.serve(reader, io::stdout()) {
            // A closed pipe means Sevak is gone; nothing to tell anyone.
            if err.kind() != io::ErrorKind::BrokenPipe {
                log(format!("sevak-extension-sdk: {err}"));
            }
        }
    }

    /// The protocol loop over any reader and writer. Returns when `shutdown`
    /// arrives or the reader ends. This is what [`Extension::run`] does with
    /// stdin and stdout, and how an extension is tested without Sevak.
    ///
    /// Lines are read on a helper thread so that, when several queries are
    /// waiting, the stale ones can be skipped: only the newest is answered.
    /// Other messages are handled in the order they arrived.
    ///
    /// ```
    /// use std::io::Cursor;
    /// use sevak_extension_sdk::{Extension, Item};
    ///
    /// let mut out = Vec::new();
    /// let input = "{\"type\":\"query\",\"request_id\":1,\"input\":\"hi\"}\n".to_owned();
    /// Extension::new(|query| Ok(vec![Item::new(query.text())]))
    ///     .serve(Cursor::new(input), &mut out)
    ///     .unwrap();
    /// assert!(String::from_utf8(out).unwrap().contains("\"title\":\"hi\""));
    /// ```
    pub fn serve<R, W>(mut self, input: R, mut output: W) -> io::Result<()>
    where
        R: BufRead + Send + 'static,
        W: Write,
    {
        let (tx, rx) = mpsc::channel::<String>();
        thread::spawn(move || {
            for line in input.lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        let mut waiting: VecDeque<Incoming> = VecDeque::new();
        loop {
            let message = match waiting.pop_front() {
                Some(message) => message,
                None => match rx.recv() {
                    Ok(line) => match parse_line(&line) {
                        Some(message) => message,
                        None => continue,
                    },
                    Err(_) => return Ok(()),
                },
            };
            if matches!(message, Incoming::Query { .. }) {
                // Whatever has already arrived: is there a newer question?
                while let Ok(line) = rx.try_recv() {
                    if let Some(next) = parse_line(&line) {
                        waiting.push_back(next);
                    }
                }
                if waiting
                    .iter()
                    .any(|next| matches!(next, Incoming::Query { .. }))
                {
                    continue;
                }
            }
            match message {
                Incoming::Initialize(init) => {
                    if let Some(handler) = &mut self.initialize {
                        let _ = catch_unwind(AssertUnwindSafe(|| handler(&init)));
                    }
                    send(&mut output, &ready_line())?;
                }
                Incoming::Query { request_id, input } => {
                    let line = self.answer(request_id, &input);
                    send(&mut output, &line)?;
                }
                Incoming::Execute(execute) => {
                    if let Some(handler) = &mut self.execute {
                        let outcome = catch_unwind(AssertUnwindSafe(|| handler(&execute)));
                        let failure = match outcome {
                            Ok(Ok(())) => None,
                            Ok(Err(err)) => Some(err.to_string()),
                            Err(panic) => Some(panic_message(&panic)),
                        };
                        if let Some(message) = failure {
                            send(&mut output, &error_line(None, &message))?;
                        }
                    }
                }
                Incoming::Shutdown {} => return Ok(()),
                Incoming::Unknown => {}
            }
        }
    }

    /// The `results` or `error` line for one query.
    fn answer(&mut self, request_id: u64, input: &str) -> String {
        let query = Query::new(input);
        match catch_unwind(AssertUnwindSafe(|| (self.query)(&query))) {
            Ok(Ok(items)) => {
                #[cfg(debug_assertions)]
                for item in &items {
                    for problem in item.problems() {
                        log(format!("sevak-extension-sdk: {problem}"));
                    }
                }
                results_line(request_id, items_to_values(&items))
            }
            Ok(Err(err)) => error_line(Some(request_id), err.message()),
            Err(panic) => error_line(Some(request_id), &panic_message(&panic)),
        }
    }
}

fn items_to_values(items: &[Item]) -> Vec<Value> {
    items.iter().take(MAX_ITEMS).map(Item::to_value).collect()
}

fn send<W: Write>(output: &mut W, line: &str) -> io::Result<()> {
    writeln!(output, "{line}")?;
    output.flush()
}

fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    let detail = panic
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned());
    match detail {
        Some(detail) => format!("the extension panicked: {detail}"),
        None => "the extension panicked".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::item::Action;

    /// A writer that can be read after `serve` consumed it.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Sink {
        fn lines(&self) -> Vec<Value> {
            String::from_utf8(self.0.lock().unwrap().clone())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    fn serve(extension: Extension, input: &str) -> Vec<Value> {
        let sink = Sink::default();
        extension
            .serve(Cursor::new(input.to_owned()), sink.clone())
            .unwrap();
        sink.lines()
    }

    fn echo() -> Extension {
        Extension::new(|query| {
            Ok(vec![
                Item::new(format!("echo {}", query.text())).copy_on_enter()
            ])
        })
    }

    #[test]
    fn answers_a_query_with_results_for_the_same_request() {
        let out = serve(
            echo(),
            "{\"type\":\"initialize\",\"protocol\":1,\"sevak_version\":\"0.1.0\",\"plugin_id\":\"script:t\"}\n\
             {\"type\":\"query\",\"request_id\":7,\"input\":\"hi\"}\n\
             {\"type\":\"shutdown\"}\n",
        );
        assert_eq!(out[0], json!({"type": "ready"}));
        assert_eq!(out[1]["type"], "results");
        assert_eq!(out[1]["request_id"], 7);
        assert_eq!(out[1]["items"][0]["title"], "echo hi");
        assert_eq!(
            out[1]["items"][0]["action"],
            json!({"type": "copy_text", "text": "echo hi"})
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn stops_at_shutdown_and_at_end_of_input() {
        let out = serve(
            echo(),
            "{\"type\":\"shutdown\"}\n{\"type\":\"query\",\"request_id\":1,\"input\":\"x\"}\n",
        );
        assert!(
            out.is_empty(),
            "nothing is answered after shutdown: {out:?}"
        );
        let out = serve(
            echo(),
            "{\"type\":\"query\",\"request_id\":1,\"input\":\"x\"}\n",
        );
        assert_eq!(out.len(), 1, "end of input ends the loop");
    }

    #[test]
    fn garbage_and_unknown_messages_are_ignored() {
        let out = serve(
            echo(),
            "not json\n\n[1]\n{\"type\":\"from_the_future\"}\n\
             {\"type\":\"query\",\"request_id\":2,\"input\":\"ok\"}\n",
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["request_id"], 2);
    }

    #[test]
    fn errors_are_reported_against_the_request() {
        let out = serve(
            Extension::new(|_| Err(Error::new("the service is down"))),
            "{\"type\":\"query\",\"request_id\":9,\"input\":\"x\"}\n",
        );
        assert_eq!(
            out[0],
            json!({"type": "error", "request_id": 9, "message": "the service is down"})
        );
    }

    #[test]
    fn a_panic_is_an_error_and_the_loop_survives() {
        let calls = Rc::new(RefCell::new(0));
        let seen = calls.clone();
        let extension = Extension::new(move |query| {
            *seen.borrow_mut() += 1;
            if query.text() == "boom" {
                panic!("kaboom");
            }
            Ok(vec![Item::new("fine")])
        });
        // `serve` needs a Send reader only; the handlers stay on this thread.
        let out = serve(
            extension,
            "{\"type\":\"query\",\"request_id\":1,\"input\":\"boom\"}\n\
             {\"type\":\"query\",\"request_id\":2,\"input\":\"ok\"}\n",
        );
        // Queued queries are coalesced: depending on timing the first may be
        // skipped, but the last always gets its answer and a panic never ends the loop.
        let last = out.last().unwrap();
        assert_eq!(last["type"], "results");
        assert_eq!(last["request_id"], 2);
        if let Some(first) = out.first().filter(|first| first["request_id"] == 1) {
            assert_eq!(first["type"], "error");
            assert!(
                first["message"].as_str().unwrap().contains("kaboom"),
                "{first}"
            );
        }
        assert!(*calls.borrow() >= 1);
    }

    #[test]
    fn a_panic_alone_is_reported() {
        let out = serve(
            Extension::new(|_| -> Result<Vec<Item>, Error> { panic!("kaboom") }),
            "{\"type\":\"query\",\"request_id\":1,\"input\":\"x\"}\n",
        );
        assert_eq!(out[0]["type"], "error");
        assert_eq!(out[0]["request_id"], 1);
        assert!(out[0]["message"].as_str().unwrap().contains("kaboom"));
    }

    #[test]
    fn only_the_newest_of_several_waiting_queries_is_answered() {
        let asked = Rc::new(RefCell::new(Vec::<String>::new()));
        let log = asked.clone();
        let extension = Extension::new(move |query| {
            log.borrow_mut().push(query.text().to_owned());
            Ok(vec![Item::new(query.text())])
        });
        // The reader thread queues the lines as fast as it can, so the first
        // queries are usually stale by the time the loop looks at them. How many
        // are skipped depends on timing; the newest is always answered.
        let input = (1..=3)
            .map(|n| format!("{{\"type\":\"query\",\"request_id\":{n},\"input\":\"q{n}\"}}\n"))
            .collect::<String>();
        let sink = Sink::default();
        extension.serve(Cursor::new(input), sink.clone()).unwrap();
        let out = sink.lines();
        assert_eq!(out.last().unwrap()["request_id"], 3);
        assert!(!out.is_empty() && out.len() <= 3);
        // Whatever was answered, the newest is the last and none is out of order.
        let ids: Vec<u64> = out
            .iter()
            .map(|v| v["request_id"].as_u64().unwrap())
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
        assert_eq!(asked.borrow().last().unwrap(), "q3");
    }

    #[test]
    fn execute_reaches_its_handler_and_failures_are_reported() {
        let got = Rc::new(RefCell::new(Vec::<(String, String)>::new()));
        let seen = got.clone();
        let extension = echo().on_execute(move |execute| {
            seen.borrow_mut()
                .push((execute.key.clone(), execute.payload.clone()));
            if execute.payload == "fail" {
                Err(Error::new("could not save"))
            } else {
                Ok(())
            }
        });
        let out = serve(
            extension,
            "{\"type\":\"execute\",\"key\":\"a\",\"payload\":\"one\"}\n\
             {\"type\":\"execute\",\"key\":\"b\",\"payload\":\"fail\"}\n",
        );
        assert_eq!(
            *got.borrow(),
            [
                ("a".to_owned(), "one".to_owned()),
                ("b".to_owned(), "fail".to_owned())
            ]
        );
        assert_eq!(out.len(), 1, "only the failure is reported: {out:?}");
        assert_eq!(out[0]["type"], "error");
        assert_eq!(out[0]["message"], "could not save");
    }

    #[test]
    fn execute_without_a_handler_is_ignored() {
        assert!(serve(
            echo(),
            "{\"type\":\"execute\",\"key\":\"a\",\"payload\":\"p\"}\n"
        )
        .is_empty());
    }

    #[test]
    fn initialize_reaches_its_handler_before_ready() {
        let got = Rc::new(RefCell::new(None));
        let seen = got.clone();
        let extension = echo().on_initialize(move |init| {
            *seen.borrow_mut() = Some(init.clone());
        });
        let out = serve(
            extension,
            "{\"type\":\"initialize\",\"protocol\":1,\"sevak_version\":\"9.9.9\",\"plugin_id\":\"script:z\"}\n",
        );
        assert_eq!(out, [json!({"type": "ready"})]);
        let init = got.borrow().clone().unwrap();
        assert_eq!(init.protocol, 1);
        assert_eq!(init.sevak_version, "9.9.9");
        assert_eq!(init.plugin_id, "script:z");
    }

    #[test]
    fn an_answer_holds_at_most_fifty_items() {
        let out = serve(
            Extension::new(|_| Ok((0..80).map(|n| Item::new(format!("row {n}"))).collect())),
            "{\"type\":\"query\",\"request_id\":1,\"input\":\"\"}\n",
        );
        assert_eq!(out[0]["items"].as_array().unwrap().len(), MAX_ITEMS);
    }

    #[test]
    fn custom_actions_round_trip_through_the_protocol() {
        let out = serve(
            Extension::new(|query| {
                Ok(vec![Item::new("Save")
                    .key("save")
                    .action(Action::custom(query.text()))])
            }),
            "{\"type\":\"query\",\"request_id\":1,\"input\":\"note\"}\n",
        );
        assert_eq!(
            out[0]["items"][0],
            json!({"title": "Save", "key": "save", "action": {"type": "custom", "payload": "note"}})
        );
    }

    #[test]
    fn non_ascii_text_survives_in_both_directions() {
        let out = serve(
            echo(),
            "{\"type\":\"query\",\"request_id\":1,\"input\":\"caf\\u00e9 \\ud83d\\ude00\"}\n",
        );
        assert_eq!(out[0]["items"][0]["title"], "echo café 😀");
    }

    #[test]
    fn the_oneshot_document_is_the_items_object() {
        let mut handler = |query: &Query| Ok(vec![Item::new(query.text())]);
        let document = oneshot_document(&mut handler, "abc").unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&document).unwrap(),
            json!({"items": [{"title": "abc"}]})
        );
        let mut failing = |_: &Query| -> Result<Vec<Item>, Error> { Err(Error::new("no")) };
        assert_eq!(
            oneshot_document(&mut failing, "").unwrap_err().message(),
            "no"
        );
    }
}
