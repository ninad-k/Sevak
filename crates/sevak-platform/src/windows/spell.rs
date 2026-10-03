//! Windows' own spell checker (`ISpellChecker`, Windows 8 and later).
//!
//! The checker is created once, on a worker thread that lives for the rest of
//! the process, so a keystroke never pays for COM start-up: [`check`] sends the
//! word to that thread and waits a short, bounded time for the answer. If the
//! checker is not ready yet (the first call creates it) or no English
//! dictionary is installed, `None` is returned and the dictionary plugin uses
//! its own word list for that keystroke.

use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::OnceLock;
use std::time::Duration;

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Globalization::{
    ISpellChecker, ISpellCheckerFactory, ISpellingError, SpellCheckerFactory,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED,
};

use crate::dictionary::Spelling;

/// How long a keystroke waits for the checker before giving up on it.
const ANSWER_WITHIN: Duration = Duration::from_millis(120);
/// Suggestions kept per word.
const MAX_SUGGESTIONS: usize = 8;
/// English variants tried in this order; the plugin's word list is English.
const LANGUAGES: [&str; 4] = ["en-US", "en-GB", "en-AU", "en-CA"];

struct Request {
    word: String,
    reply: SyncSender<Option<Spelling>>,
}

static WORKER: OnceLock<Option<Sender<Request>>> = OnceLock::new();

/// Spell-checks `word`; `None` if Windows cannot answer in time.
pub(super) fn check(word: &str) -> Option<Spelling> {
    let sender = WORKER.get_or_init(start_worker).as_ref()?;
    let (reply, answer) = mpsc::sync_channel(1);
    sender
        .send(Request {
            word: word.to_owned(),
            reply,
        })
        .ok()?;
    answer.recv_timeout(ANSWER_WITHIN).ok().flatten()
}

fn start_worker() -> Option<Sender<Request>> {
    let (sender, requests) = mpsc::channel::<Request>();
    std::thread::Builder::new()
        .name("sevak-spell".into())
        .spawn(move || serve(&requests))
        .ok()?;
    Some(sender)
}

fn serve(requests: &Receiver<Request>) {
    // SAFETY: no pointers; this thread never ends before the process does, so
    // the matching `CoUninitialize` below only runs if the channel closes.
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let checker = create_checker();
    if checker.is_none() {
        tracing::debug!("no Windows spell checker for English; using the built-in word list");
    }
    for request in requests {
        let answer = checker.as_ref().and_then(|c| spell(c, &request.word));
        let _ = request.reply.send(answer);
    }
    drop(checker);
    if initialized {
        // SAFETY: matches the successful `CoInitializeEx` above.
        unsafe { CoUninitialize() };
    }
}

fn create_checker() -> Option<ISpellChecker> {
    // SAFETY: COM is initialized on this thread; the CLSID and interface are the
    // ones Windows documents for the spell-checking API.
    let factory: ISpellCheckerFactory =
        unsafe { CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER) }.ok()?;
    LANGUAGES.iter().find_map(|tag| {
        let tag = HSTRING::from(*tag);
        // SAFETY: `tag` is a NUL-terminated UTF-16 string that outlives the calls.
        unsafe {
            let supported = factory.IsSupported(PCWSTR(tag.as_ptr())).ok()?.as_bool();
            if supported {
                factory.CreateSpellChecker(PCWSTR(tag.as_ptr())).ok()
            } else {
                None
            }
        }
    })
}

fn spell(checker: &ISpellChecker, word: &str) -> Option<Spelling> {
    let text = HSTRING::from(word);
    // SAFETY: `text` is a NUL-terminated UTF-16 string that outlives the calls;
    // every string `IEnumString::Next` hands out is freed with `CoTaskMemFree`.
    unsafe {
        let errors = checker.Check(PCWSTR(text.as_ptr())).ok()?;
        // `Next` returns S_FALSE and no error object when the text is fine.
        let mut first: Option<ISpellingError> = None;
        if errors.Next(&mut first).is_err() || first.is_none() {
            return Some(Spelling {
                correct: true,
                suggestions: Vec::new(),
            });
        }
        let suggestions = checker.Suggest(PCWSTR(text.as_ptr())).ok()?;
        let mut found = Vec::new();
        while found.len() < MAX_SUGGESTIONS {
            let mut item = [PWSTR::null()];
            let mut fetched = 0u32;
            if !suggestions.Next(&mut item, Some(&mut fetched)).is_ok() || fetched == 0 {
                break;
            }
            if let Ok(text) = item[0].to_string() {
                found.push(text);
            }
            CoTaskMemFree(Some(item[0].0.cast_const().cast()));
        }
        Some(Spelling {
            correct: false,
            suggestions: found,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asks the real spell checker: `cargo test -p sevak-platform -- --ignored
    /// --nocapture windows_spell_checker`.
    #[test]
    #[ignore = "needs the Windows spell checker and an English language pack"]
    fn windows_spell_checker() {
        // The first call may only start the worker.
        let mut right = None;
        for _ in 0..20 {
            right = check("receive");
            if right.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let right = right.expect("no answer from the spell checker");
        assert!(right.correct);
        let wrong = check("recieve").unwrap();
        assert!(!wrong.correct);
        println!("suggestions: {:?}", wrong.suggestions);
        assert!(wrong.suggestions.iter().any(|s| s == "receive"));
    }
}
