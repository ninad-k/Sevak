//! The `ai <question>` plugin.
//!
//! | You type | Rows | Enter |
//! |---|---|---|
//! | `ai` | (nothing, or a Tab hint) | |
//! | `ai why is the sky blue` | "Ask ..." naming the model and the host the question goes to | sends the question; the row becomes "Asking ..." and then the answer |
//! | (after the answer) | the answer, and "Open full answer" for a long one | copies the answer (`Shift+Enter` pastes it, `Ctrl+T` reads it all) |
//!
//! # Nothing is sent until you say so
//!
//! Typing the keyword and a question only builds a row: [`Plugin::query`] never
//! starts a request. The request starts when the user presses Enter on that row
//! ([`Plugin::execute`]), on a worker thread. While `[ai] enabled` is off the
//! plugin has no way to ask at all.
//!
//! # What is never done
//!
//! * The answer is untrusted text. It becomes plain strings in `CopyText` and
//!   `PasteText` actions and a text view; nothing in it is run, opened or
//!   interpreted, and it is cleaned first ([`super::provider::clean_answer`]).
//! * Neither the question nor the answer is logged or written to disk: they
//!   live in memory until another question replaces them, the engine is
//!   rebuilt or Sevak quits. [`Plugin::tracks_usage`] is false, so they stay
//!   out of `usage.json` and the search history.
//!
//! # Ask AI about selection
//!
//! The Universal Actions list gains one action for a text selection. It only
//! puts `ai <selection>` in the search box, so the user can edit it; sending is
//! still the Enter above.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, MutexGuard};

use sevak_core::config::PasteConfig;
use sevak_core::model::score;
use sevak_core::plugin::ResultsNotifier;
use sevak_core::selection::preview;
use sevak_core::{
    Action, AiConfig, IconSource, Modifier, Plugin, PluginError, PluginResult, ResultItem,
    Selection, SelectionKind, ViewHint,
};
use sevak_platform::PlatformProvider;

use super::assistant::{Asker, Assistant, Target, MAX_PROMPT_CHARS};
use super::error::AiError;
use super::provider::Answer;
use crate::actions::execute_action;

pub const ID: &str = "ai";

const PAYLOAD_ASK: &str = "ask:";
const PAYLOAD_CANCEL: &str = "cancel";
const PAYLOAD_NOTHING: &str = "nothing";
/// The window request `sevak_plugins::selection::ui_request` understands: put
/// this text in the search box.
const SEARCH_PREFIX: &str = "search:";
const ENABLE_SNIPPET: &str = "[ai]\nenabled = true";
/// What the selection action puts in front of the selected text.
const SELECTION_LEAD: &str = "Explain this: ";

/// An answer's row shows this many characters of it.
const TITLE_CHARS: usize = 140;
const QUESTION_CHARS: usize = 80;
/// The most of a selection put in the search box.
const SELECTION_CHARS: usize = 2_000;

#[derive(Debug)]
enum Outcome {
    Pending,
    Answer(Answer),
    Failed(AiError),
}

#[derive(Debug)]
struct Exchange {
    prompt: String,
    outcome: Outcome,
}

#[derive(Default)]
struct State {
    /// Bumped by every question and every cancel, so a late reply to an
    /// abandoned question is dropped.
    generation: u64,
    exchange: Option<Exchange>,
}

struct Shared {
    asker: Arc<dyn Asker>,
    target: Target,
    state: Mutex<State>,
    notifier: Mutex<Option<ResultsNotifier>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    fn notify(&self) {
        let notifier = lock(&self.notifier).clone();
        if let Some(notify) = notifier {
            notify(ID);
        }
    }

    /// Starts asking `prompt` on a worker thread. A question that is already
    /// on its way is not sent twice.
    fn ask(self: &Arc<Self>, prompt: &str) {
        let prompt = prompt.trim().to_owned();
        let generation = {
            let mut state = lock(&self.state);
            if let Some(Exchange {
                prompt: current,
                outcome: Outcome::Pending,
            }) = &state.exchange
            {
                if *current == prompt {
                    return;
                }
            }
            state.generation += 1;
            state.exchange = Some(Exchange {
                prompt: prompt.clone(),
                outcome: Outcome::Pending,
            });
            state.generation
        };
        // The launcher shows "Asking..." now, not when the answer is in.
        self.notify();

        let shared = Arc::clone(self);
        let worker = prompt.clone();
        let spawned = std::thread::Builder::new()
            .name("sevak-ai".into())
            .spawn(move || {
                let outcome = match catch_unwind(AssertUnwindSafe(|| shared.asker.ask(&worker))) {
                    Ok(Ok(answer)) => Outcome::Answer(answer),
                    Ok(Err(error)) => Outcome::Failed(error),
                    Err(_) => Outcome::Failed(AiError::BadSettings(
                        "The assistant stopped unexpectedly.".to_owned(),
                    )),
                };
                shared.finish(generation, outcome);
            });
        if spawned.is_err() {
            self.finish(
                generation,
                Outcome::Failed(AiError::BadSettings(
                    "Could not start a background thread.".to_owned(),
                )),
            );
        }
    }

    fn finish(&self, generation: u64, outcome: Outcome) {
        {
            let mut state = lock(&self.state);
            if state.generation != generation {
                return;
            }
            if let Some(exchange) = state.exchange.as_mut() {
                exchange.outcome = outcome;
            }
        }
        self.notify();
    }

    /// Abandons the question on its way. The request itself runs until its
    /// timeout; its reply is dropped.
    fn cancel(&self) {
        {
            let mut state = lock(&self.state);
            state.generation += 1;
            state.exchange = None;
        }
        self.notify();
    }

    fn clear(&self) {
        let mut state = lock(&self.state);
        state.generation += 1;
        state.exchange = None;
    }
}

/// The `ai` plugin.
pub struct AiPlugin {
    keyword: String,
    /// `None` while `[ai] enabled` is off: there is nothing that could ask.
    shared: Option<Arc<Shared>>,
    restore_clipboard: bool,
    platform: Arc<dyn PlatformProvider>,
}

impl AiPlugin {
    pub fn new(
        config: &AiConfig,
        paste: &PasteConfig,
        platform: Arc<dyn PlatformProvider>,
    ) -> Self {
        let asker: Option<Arc<dyn Asker>> = config
            .enabled
            .then(|| Arc::new(Assistant::new(config)) as Arc<dyn Asker>);
        Self::with_asker(config, paste, platform, asker)
    }

    /// Like [`AiPlugin::new`] with a different way of asking (`None`: off).
    pub fn with_asker(
        config: &AiConfig,
        paste: &PasteConfig,
        platform: Arc<dyn PlatformProvider>,
        asker: Option<Arc<dyn Asker>>,
    ) -> Self {
        let shared = asker.map(|asker| {
            Arc::new(Shared {
                target: asker.target(),
                asker,
                state: Mutex::new(State::default()),
                notifier: Mutex::new(None),
            })
        });
        Self {
            keyword: config.keyword.clone(),
            shared,
            restore_clipboard: paste.restore_clipboard,
            platform,
        }
    }

    fn status(&self, key: &str, title: &str, subtitle: &str, action: Action) -> ResultItem {
        ResultItem::new(ID, key, title, action)
            .with_subtitle(subtitle)
            .with_icon(IconSource::builtin("sparkle"))
            .with_score(score::KEYWORD)
    }

    /// Where the question goes, in words: what the user is told before Enter.
    fn destination(target: &Target) -> String {
        if target.local {
            format!("{} on this computer ({})", target.model, target.host)
        } else {
            format!("{} at {}", target.model, target.host)
        }
    }

    fn rows(&self, input: &str) -> Vec<ResultItem> {
        let Some(shared) = &self.shared else {
            return vec![self
                .status(
                    "off",
                    "The AI assistant is off",
                    "Turn it on in Settings > AI assistant. Nothing is sent until you do.",
                    nothing(),
                )
                .with_secondary(
                    "Copy the config.toml setting",
                    None,
                    Action::CopyText {
                        text: ENABLE_SNIPPET.to_owned(),
                    },
                )];
        };
        let prompt = input.trim();
        if prompt.is_empty() {
            return vec![self.status(
                "hint",
                "Type a question, then press Enter",
                &format!(
                    "Enter sends it to {}. Nothing is sent before that.",
                    Self::destination(&shared.target)
                ),
                nothing(),
            )];
        }
        let length = prompt.chars().count();
        if length > MAX_PROMPT_CHARS {
            return vec![self.status(
                "toolong",
                "That question is too long to send",
                &format!("{length} characters; the limit is {MAX_PROMPT_CHARS}. Shorten it."),
                nothing(),
            )];
        }

        let state = lock(&shared.state);
        match state
            .exchange
            .as_ref()
            .filter(|exchange| exchange.prompt == prompt)
        {
            Some(exchange) => self.exchange_rows(&shared.target, exchange),
            None => vec![self.status(
                "ask",
                &format!("Ask \u{201c}{}\u{201d}", preview(prompt, QUESTION_CHARS)),
                &format!(
                    "Enter sends your question to {}",
                    Self::destination(&shared.target)
                ),
                ask(prompt),
            )],
        }
    }

    fn exchange_rows(&self, target: &Target, exchange: &Exchange) -> Vec<ResultItem> {
        match &exchange.outcome {
            // Enter does nothing here, so a double-tap on the "Ask" row cannot
            // cancel what it just sent; the action panel has Cancel.
            Outcome::Pending => vec![self
                .status(
                    "pending",
                    &format!("Asking {}\u{2026}", target.model),
                    &format!("Waiting for {}", target.host),
                    nothing(),
                )
                .with_secondary(
                    "Cancel the question",
                    None,
                    Action::Custom {
                        payload: PAYLOAD_CANCEL.to_owned(),
                    },
                )],
            Outcome::Failed(error) => {
                let retry = !matches!(
                    error,
                    AiError::NoKey { .. }
                        | AiError::BadSettings(_)
                        | AiError::BadPrompt(_)
                        | AiError::KeyUnreadable
                );
                let detail = error.to_string();
                let subtitle = if retry {
                    format!("{detail} \u{b7} Enter to try again")
                } else {
                    detail.clone()
                };
                vec![self
                    .status(
                        "error",
                        &error.title(),
                        &subtitle,
                        if retry {
                            ask(&exchange.prompt)
                        } else {
                            nothing()
                        },
                    )
                    .with_view(ViewHint::Text {
                        text: detail,
                        on_enter: false,
                    })]
            }
            Outcome::Answer(answer) => self.answer_rows(target, exchange, answer),
        }
    }

    fn answer_rows(
        &self,
        target: &Target,
        exchange: &Exchange,
        answer: &Answer,
    ) -> Vec<ResultItem> {
        let text = &answer.text;
        let shown = preview(text, TITLE_CHARS);
        let can_paste = self.platform.paste_support().is_available();

        let mut hints = vec![target.model.clone(), "Enter copies".to_owned()];
        if can_paste {
            hints.push("Shift+Enter pastes".to_owned());
        }
        hints.push("Ctrl+T reads it all".to_owned());
        let mut subtitle = hints.join(" \u{b7} ");
        if answer.cut_off {
            subtitle.push_str(" \u{b7} cut off at the length limit");
        }

        let mut row = self
            .status(
                "answer",
                &shown,
                &subtitle,
                Action::CopyText { text: text.clone() },
            )
            .with_view(ViewHint::Text {
                text: text.clone(),
                on_enter: false,
            });
        if can_paste {
            row = row.with_secondary(
                "Paste the answer",
                Some(Modifier::Shift),
                Action::PasteText {
                    text: text.clone(),
                    restore_clipboard: self.restore_clipboard,
                },
            );
        }
        row = row.with_secondary("Ask again", None, ask(&exchange.prompt));
        let mut rows = vec![row];

        if text.chars().count() > TITLE_CHARS || text.contains('\n') {
            rows.push(
                self.status(
                    "full",
                    "Open full answer",
                    &format!(
                        "{} characters \u{b7} Enter to read it",
                        text.chars().count()
                    ),
                    Action::CopyText { text: text.clone() },
                )
                .with_view(ViewHint::Text {
                    text: text.clone(),
                    on_enter: true,
                })
                .with_score(score::KEYWORD - 1.0),
            );
        }
        rows
    }
}

fn ask(prompt: &str) -> Action {
    Action::Custom {
        payload: format!("{PAYLOAD_ASK}{prompt}"),
    }
}

fn nothing() -> Action {
    Action::Custom {
        payload: PAYLOAD_NOTHING.to_owned(),
    }
}

/// What "Ask AI about selection" puts in the search box.
fn selection_query(keyword: &str, selection: &str) -> String {
    let one_line: String = selection.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = one_line.chars().take(SELECTION_CHARS).collect();
    format!("{keyword} {SELECTION_LEAD}{cut}")
}

impl Plugin for AiPlugin {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "AI assistant"
    }

    fn description(&self) -> &str {
        "Type `ai ` and a question; Enter sends it to the AI service you chose (OpenAI-compatible, Anthropic or Ollama). Off until [ai] enabled = true; nothing is sent before you press Enter."
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn keyword_row(&self) -> Option<ResultItem> {
        self.shared.as_ref()?;
        Some(
            self.status(
                "keyword",
                "Ask the AI assistant",
                "Press Tab, then type your question",
                nothing(),
            )
            .with_autocomplete(format!("{} ", self.keyword)),
        )
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        self.rows(input)
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        let Action::Custom { payload } = &item.action else {
            return execute_action(self.platform.as_ref(), &item.action);
        };
        if payload == PAYLOAD_NOTHING {
            return Ok(());
        }
        if payload == PAYLOAD_CANCEL {
            if let Some(shared) = &self.shared {
                shared.cancel();
            }
            return Ok(());
        }
        if let Some(prompt) = payload.strip_prefix(PAYLOAD_ASK) {
            let shared = self.shared.as_ref().ok_or_else(|| {
                PluginError::Message("the AI assistant is off in Settings".to_owned())
            })?;
            shared.ask(prompt);
            return Ok(());
        }
        if payload.starts_with(SEARCH_PREFIX) {
            return Err(PluginError::Message(
                "that action is carried out by the launcher window".to_owned(),
            ));
        }
        Err(PluginError::Unsupported(item.id.clone()))
    }

    /// Asking and cancelling happen in the launcher: the row turns into
    /// "Asking..." and then the answer.
    fn keeps_open(&self, item: &ResultItem) -> bool {
        matches!(
            &item.action,
            Action::Custom { payload }
                if payload == PAYLOAD_NOTHING
                    || payload == PAYLOAD_CANCEL
                    || payload.starts_with(PAYLOAD_ASK)
        )
    }

    /// Questions and answers stay out of `usage.json` and the search history.
    fn tracks_usage(&self) -> bool {
        false
    }

    fn selection_actions(&self, selection: &Selection) -> Vec<ResultItem> {
        if self.shared.is_none()
            || selection.is_too_large()
            || selection.kind() != SelectionKind::Text
        {
            return Vec::new();
        }
        let Some(text) = selection.text() else {
            return Vec::new();
        };
        vec![ResultItem::new(
            ID,
            "selection",
            "Ask AI about selection",
            Action::Custom {
                payload: format!("{SEARCH_PREFIX}{}", selection_query(&self.keyword, text)),
            },
        )
        .with_subtitle("Puts it in the search box; nothing is sent until you press Enter")
        .with_icon(IconSource::builtin("sparkle"))]
    }

    fn attach_notifier(&self, notifier: ResultsNotifier) {
        if let Some(shared) = &self.shared {
            *lock(&shared.notifier) = Some(notifier);
        }
    }

    fn shutdown(&self) {
        if let Some(shared) = &self.shared {
            shared.clear();
        }
    }
}

impl Drop for AiPlugin {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
#[path = "plugin_tests.rs"]
mod tests;
