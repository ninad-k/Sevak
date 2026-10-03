//! Dictionary and spelling, offline: `define <word>` and `spell <word>`.
//!
//! | Keyword | Rows | Enter |
//! |---|---|---|
//! | `define serendipity` | one row per meaning: the definition, with its part of speech | copies the definition (`Shift+Enter`: with the word) |
//! | `spell recieve` | the suggested corrections, best first | pastes the word into the app you came from (`Ctrl+Enter` copies); copies where pasting is not possible |
//!
//! `Ctrl+L` shows a definition in Large Type, and `Tab` on a "Did you mean" row
//! completes the query with that word.
//!
//! # Where the answers come from
//!
//! | | Definitions | Spelling |
//! |---|---|---|
//! | macOS | Dictionary Services (the Dictionary app's data) | the bundled word list |
//! | Windows | the bundled dictionary | Windows' spell checker (`ISpellChecker`) |
//! | Linux | the bundled dictionary | the bundled word list, plus the stems in `/usr/share/hunspell/en_*.dic` when present |
//!
//! `[dictionary] use_system = false` always uses the bundled data. The bundled
//! dictionary is Princeton WordNet 3.0, trimmed (see [`lexicon`]); its licence
//! is in `THIRD_PARTY_NOTICES.md` and in the data file's header.
//!
//! Nothing here uses the network, and neither the words you look up nor the
//! text you check are written anywhere: [`Plugin::tracks_usage`] is false, so
//! they stay out of `usage.json` and the search history.
//!
//! The two keywords are two plugin instances (`dict` and `dict:spell`) sharing
//! one copy of the dictionary, which is decompressed once per process.

pub mod lexicon;

use std::path::Path;
use std::sync::{Arc, OnceLock};

use sevak_core::config::{DictionaryConfig, PasteConfig};
use sevak_core::model::score;
use sevak_core::{Action, IconSource, Modifier, Plugin, PluginResult, ResultItem};
use sevak_platform::{PasteSupport, PlatformProvider};

use self::lexicon::{Entry, Lexicon};
use crate::actions::execute_action;

pub const DEFINE_KEYWORD: &str = "define";
pub const SPELL_KEYWORD: &str = "spell";

const DEFINE_ID: &str = "dict";
const SPELL_ID: &str = "dict:spell";
const PAYLOAD_NOTHING: &str = "nothing";

/// Meanings shown for a word.
const MAX_DEFINITIONS: usize = 10;
/// Corrections shown.
const MAX_SUGGESTIONS: usize = 8;
/// Longest word looked up.
const MAX_WORD_CHARS: usize = 60;
/// Longest definition, per row, taken from the macOS Dictionary.
const MAX_SYSTEM_CHARS: usize = 600;

/// The compressed WordNet data (see `scripts/build-dictionary.py`).
static BUNDLED_DATA: &[u8] = include_bytes!("../../data/wordnet-en.z");
const MAX_UNPACKED_BYTES: usize = 64 * 1024 * 1024;

static BUNDLED: OnceLock<Lexicon> = OnceLock::new();

/// The bundled dictionary, unpacked on first use (about 50 ms) and kept for the
/// life of the process. [`Plugin::refresh`] calls this on a background thread so
/// the first lookup does not pay for it.
pub fn bundled() -> &'static Lexicon {
    BUNDLED.get_or_init(|| {
        match miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(
            BUNDLED_DATA,
            MAX_UNPACKED_BYTES,
        ) {
            Ok(bytes) => Lexicon::from_text(String::from_utf8_lossy(&bytes).into_owned()),
            Err(err) => {
                tracing::error!(?err, "the bundled dictionary is damaged");
                Lexicon::from_text(String::new())
            }
        }
    })
}

/// The system's English hunspell word lists, if installed (Linux). Stems only:
/// the affix rules are not applied, which [`Lexicon::is_correct`] makes up for
/// by trying regular inflections.
const HUNSPELL_FILES: &[&str] = &[
    "/usr/share/hunspell/en_US.dic",
    "/usr/share/hunspell/en_GB.dic",
    "/usr/share/myspell/en_US.dic",
    "/usr/share/myspell/dicts/en_US.dic",
    "/usr/share/myspell/dicts/en_GB.dic",
];

/// The words in a hunspell `.dic` file: the part of each line before `/` (the
/// affix flags). The first line is a count. Names with capitals and anything
/// that is not a plain word are skipped.
pub fn parse_hunspell_dic(text: &str) -> Vec<String> {
    text.lines()
        .skip(usize::from(text.lines().next().is_some_and(|first| {
            first.trim().chars().all(|c| c.is_ascii_digit())
        })))
        .filter_map(|line| {
            let word = line.split(['/', '\t', ' ']).next()?.trim();
            let plain = !word.is_empty()
                && word.len() <= 40
                && word.chars().all(|c| c.is_ascii_lowercase() || c == '\'');
            plain.then(|| word.to_owned())
        })
        .collect()
}

fn load_system_word_lists(files: &[&str]) -> Vec<String> {
    files
        .iter()
        .filter_map(|file| std::fs::read(Path::new(file)).ok())
        .flat_map(|bytes| parse_hunspell_dic(&String::from_utf8_lossy(&bytes)))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Define,
    Spell,
}

/// One of the two instances of the dictionary plugin.
pub struct DictionaryPlugin {
    mode: Mode,
    keyword: String,
    use_system: bool,
    restore_clipboard: bool,
    lexicon: &'static Lexicon,
    platform: Arc<dyn PlatformProvider>,
}

impl DictionaryPlugin {
    /// `define` and `spell` instances for `config`.
    pub fn instances(
        config: &DictionaryConfig,
        paste: &PasteConfig,
        platform: Arc<dyn PlatformProvider>,
    ) -> Vec<Arc<dyn Plugin>> {
        [
            (Mode::Define, config.define_keyword.clone()),
            (Mode::Spell, config.spell_keyword.clone()),
        ]
        .into_iter()
        .map(|(mode, keyword)| {
            Arc::new(Self {
                mode,
                keyword,
                use_system: config.use_system,
                restore_clipboard: paste.restore_clipboard,
                lexicon: bundled(),
                platform: platform.clone(),
            }) as Arc<dyn Plugin>
        })
        .collect()
    }

    fn id_str(&self) -> &'static str {
        match self.mode {
            Mode::Define => DEFINE_ID,
            Mode::Spell => SPELL_ID,
        }
    }

    fn row(&self, key: &str, title: impl Into<String>, action: Action) -> ResultItem {
        ResultItem::new(self.id_str(), key, title, action).with_icon(IconSource::builtin("plugin"))
    }

    fn hint(&self, title: &str, subtitle: &str) -> ResultItem {
        self.row(
            "hint",
            title,
            Action::Custom {
                payload: PAYLOAD_NOTHING.to_owned(),
            },
        )
        .with_subtitle(subtitle)
        .with_score(score::KEYWORD)
    }

    // ----- define ---------------------------------------------------------

    fn define(&self, input: &str) -> Vec<ResultItem> {
        let Some(word) = normalize_word(input) else {
            return vec![self.hint("Type a word to define", "For example: define serendipity")];
        };

        if self.use_system {
            if let Some(text) = self.platform.system_definition(&word) {
                let rows = self.system_rows(&word, &text);
                if !rows.is_empty() {
                    return rows;
                }
            }
        }

        let mut rows: Vec<ResultItem> = Vec::new();
        let mut shown: Vec<String> = Vec::new();
        if let Some(entry) = self.lexicon.entry(&word) {
            if !entry.senses.is_empty() {
                self.sense_rows(&entry, None, &mut rows);
                shown.push(entry.word.to_owned());
            }
        }
        // An inflection (`children`, `running`) is explained through its base form.
        for base in self.lexicon.base_forms(&word) {
            if rows.len() >= MAX_DEFINITIONS || shown.contains(&base) {
                continue;
            }
            if let Some(entry) = self.lexicon.entry(&base) {
                if !entry.senses.is_empty() {
                    self.sense_rows(&entry, Some(&word), &mut rows);
                    shown.push(base);
                }
            }
        }
        rows.truncate(MAX_DEFINITIONS);
        if !rows.is_empty() {
            return rows;
        }
        self.not_found(&word)
    }

    fn sense_rows(&self, entry: &Entry<'_>, inflected: Option<&str>, rows: &mut Vec<ResultItem>) {
        for (index, sense) in entry.senses.iter().enumerate() {
            if rows.len() >= MAX_DEFINITIONS {
                break;
            }
            let origin = inflected
                .map(|form| format!(" · {form} is a form of {}", entry.word))
                .unwrap_or_default();
            let subtitle = format!(
                "{} · {}{origin} · Enter to copy",
                entry.word,
                sense.pos.name()
            );
            let with_word = format!(
                "{} ({}): {}",
                entry.word,
                sense.pos.name(),
                sense.definition
            );
            rows.push(self.definition_row(
                &format!("{}#{}", entry.word, index),
                sense.definition,
                subtitle,
                with_word,
                rows.len(),
            ));
        }
    }

    fn definition_row(
        &self,
        key: &str,
        definition: &str,
        subtitle: String,
        with_word: String,
        position: usize,
    ) -> ResultItem {
        self.row(
            key,
            definition,
            Action::CopyText {
                text: definition.to_owned(),
            },
        )
        .with_subtitle(subtitle)
        .with_secondary(
            "Copy with the word",
            Some(Modifier::Shift),
            Action::CopyText { text: with_word },
        )
        .with_score(score::KEYWORD - position as f64)
    }

    /// Rows from the text of the macOS Dictionary: one per bullet or line.
    fn system_rows(&self, word: &str, text: &str) -> Vec<ResultItem> {
        system_definition_parts(text)
            .into_iter()
            .take(MAX_DEFINITIONS)
            .enumerate()
            .map(|(index, part)| {
                self.definition_row(
                    &format!("{word}#{index}"),
                    &part,
                    format!("{word} · Dictionary · Enter to copy"),
                    format!("{word}: {part}"),
                    index,
                )
            })
            .collect()
    }

    fn not_found(&self, word: &str) -> Vec<ResultItem> {
        let mut rows = vec![self
            .row(
                "none",
                format!("No definition for \u{201c}{word}\u{201d}"),
                Action::Custom {
                    payload: PAYLOAD_NOTHING.to_owned(),
                },
            )
            .with_subtitle("Check the spelling, or try the base form of the word")
            .with_score(score::KEYWORD)];
        for (i, suggestion) in self.lexicon.suggestions(word, 4).into_iter().enumerate() {
            rows.push(
                self.row(
                    &format!("did-you-mean:{suggestion}"),
                    format!("Did you mean \u{201c}{suggestion}\u{201d}?"),
                    Action::CopyText {
                        text: suggestion.clone(),
                    },
                )
                .with_subtitle("Tab to look it up · Enter to copy the word")
                .with_autocomplete(suggestion)
                .with_score(score::KEYWORD - 1.0 - i as f64),
            );
        }
        rows
    }

    // ----- spell ----------------------------------------------------------

    fn spell(&self, input: &str) -> Vec<ResultItem> {
        let words: Vec<&str> = input
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\''))
            .filter(|w| !w.is_empty() && w.chars().count() <= MAX_WORD_CHARS)
            .collect();
        if words.is_empty() {
            return vec![self.hint("Type a word to check", "For example: spell recieve")];
        }

        // The first word that is wrong; the rest is the user's to retype.
        for word in &words {
            let verdict = self.verdict(word);
            if !verdict.correct {
                return self.correction_rows(word, &verdict.suggestions);
            }
        }
        let title = if words.len() == 1 {
            format!("\u{201c}{}\u{201d} is spelled correctly", words[0])
        } else {
            format!("All {} words are spelled correctly", words.len())
        };
        vec![self
            .row(
                "correct",
                title,
                Action::CopyText {
                    text: words.join(" "),
                },
            )
            .with_subtitle("Enter to copy")
            .with_score(score::KEYWORD)]
    }

    fn verdict(&self, word: &str) -> Verdict {
        if self.use_system {
            if let Some(system) = self.platform.system_spelling(word) {
                return Verdict {
                    correct: system.correct,
                    suggestions: system.suggestions,
                };
            }
        }
        if self.lexicon.is_correct(word) {
            return Verdict {
                correct: true,
                suggestions: Vec::new(),
            };
        }
        Verdict {
            correct: false,
            suggestions: self.lexicon.suggestions(word, MAX_SUGGESTIONS),
        }
    }

    fn correction_rows(&self, word: &str, suggestions: &[String]) -> Vec<ResultItem> {
        if suggestions.is_empty() {
            return vec![self
                .row(
                    "no-suggestions",
                    format!("No suggestions for \u{201c}{word}\u{201d}"),
                    Action::CopyText {
                        text: word.to_owned(),
                    },
                )
                .with_subtitle("Enter to copy the word as typed")
                .with_score(score::KEYWORD)];
        }
        let support = self.platform.paste_support();
        suggestions
            .iter()
            .take(MAX_SUGGESTIONS)
            .enumerate()
            .map(|(i, suggestion)| {
                let text = match_case(word, suggestion);
                let copy = Action::CopyText { text: text.clone() };
                let item = match &support {
                    PasteSupport::Available => self
                        .row(
                            &format!("fix:{suggestion}"),
                            &text,
                            Action::PasteText {
                                text: text.clone(),
                                restore_clipboard: self.restore_clipboard,
                            },
                        )
                        .with_subtitle(format!(
                            "Instead of {word} · Enter to paste, Ctrl+Enter to copy"
                        ))
                        .with_secondary("Copy", Some(Modifier::Ctrl), copy),
                    PasteSupport::CopyOnly(reason) => self
                        .row(&format!("fix:{suggestion}"), &text, copy)
                        .with_subtitle(format!(
                            "Instead of {word} · Copies to clipboard · {reason}"
                        )),
                };
                item.with_score(score::KEYWORD - i as f64)
            })
            .collect()
    }
}

struct Verdict {
    correct: bool,
    suggestions: Vec<String>,
}

/// The word to look up: trimmed, lower case, no surrounding quotes or
/// punctuation. `None` for empty input or a very long one.
fn normalize_word(input: &str) -> Option<String> {
    let word = input
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    let word = word.split_whitespace().collect::<Vec<_>>().join(" ");
    (!word.is_empty() && word.chars().count() <= MAX_WORD_CHARS).then_some(word)
}

/// Gives `suggestion` the capitalisation of the word that was typed:
/// `Recieve` -> `Receive`, `RECIEVE` -> `RECEIVE`.
fn match_case(typed: &str, suggestion: &str) -> String {
    let mut letters = typed.chars().filter(|c| c.is_alphabetic());
    let first_upper = letters.next().is_some_and(char::is_uppercase);
    let rest: Vec<char> = letters.collect();
    let all_upper = first_upper && !rest.is_empty() && rest.iter().all(|c| c.is_uppercase());
    if all_upper {
        suggestion.to_uppercase()
    } else if first_upper {
        let mut chars = suggestion.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().chain(chars).collect()
        })
    } else {
        suggestion.to_owned()
    }
}

/// Splits the macOS Dictionary's text into rows: a bullet (`•`) or a line break
/// starts a new one. Each is trimmed and capped. The first part is usually the
/// headword with its pronunciation and part of speech, which makes a fine row.
fn system_definition_parts(text: &str) -> Vec<String> {
    text.split(['\n', '\u{2022}'])
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|part| !part.is_empty())
        .map(|part| {
            if part.chars().count() > MAX_SYSTEM_CHARS {
                let cut: String = part.chars().take(MAX_SYSTEM_CHARS).collect();
                format!("{}…", cut.trim_end())
            } else {
                part
            }
        })
        .collect()
}

impl Plugin for DictionaryPlugin {
    fn id(&self) -> &str {
        self.id_str()
    }

    fn name(&self) -> &str {
        match self.mode {
            Mode::Define => "Dictionary",
            Mode::Spell => "Spelling",
        }
    }

    fn description(&self) -> &str {
        match self.mode {
            Mode::Define => "Type `define <word>` for definitions, offline.",
            Mode::Spell => {
                "Type `spell <word>` for corrections, offline; Enter pastes the right spelling."
            }
        }
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn keyword_row(&self) -> Option<ResultItem> {
        let title = match self.mode {
            Mode::Define => "Press Tab to type a word to define",
            Mode::Spell => "Press Tab to type a word to check",
        };
        Some(
            self.row(
                "keyword",
                title,
                Action::Custom {
                    payload: PAYLOAD_NOTHING.to_owned(),
                },
            )
            .with_icon(IconSource::builtin("plugin"))
            .with_autocomplete(format!("{} ", self.keyword)),
        )
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        match self.mode {
            Mode::Define => self.define(input),
            Mode::Spell => self.spell(input),
        }
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        match &item.action {
            Action::Custom { .. } => Ok(()),
            action => execute_action(self.platform.as_ref(), action),
        }
    }

    /// What was looked up or checked is not recorded.
    fn tracks_usage(&self) -> bool {
        false
    }

    /// Unpacks the dictionary and reads the system word lists (Linux) in the
    /// background, so the first lookup is instant.
    fn refresh(&self) -> PluginResult<()> {
        let lexicon = bundled();
        if cfg!(target_os = "linux") && self.use_system {
            lexicon.set_spelling_words(load_system_word_lists(HUNSPELL_FILES));
        }
        tracing::debug!(words = lexicon.len(), "dictionary ready");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sevak_platform::Spelling;

    use super::*;
    use crate::test_util::MockPlatform;

    fn plugins_on(platform: &Arc<MockPlatform>, use_system: bool) -> Vec<Arc<dyn Plugin>> {
        let config = DictionaryConfig {
            use_system,
            ..DictionaryConfig::default()
        };
        DictionaryPlugin::instances(&config, &PasteConfig::default(), platform.clone())
    }

    fn define(platform: &Arc<MockPlatform>) -> Arc<dyn Plugin> {
        plugins_on(platform, true).remove(0)
    }

    fn spell(platform: &Arc<MockPlatform>) -> Arc<dyn Plugin> {
        plugins_on(platform, true).remove(1)
    }

    fn titles(rows: &[ResultItem]) -> Vec<&str> {
        rows.iter().map(|r| r.title.as_str()).collect()
    }

    #[test]
    fn two_instances_serve_define_and_spell() {
        let plugins = plugins_on(&MockPlatform::empty(), true);
        let summary: Vec<_> = plugins.iter().map(|p| (p.id(), p.keyword())).collect();
        assert_eq!(
            summary,
            [("dict", Some("define")), ("dict:spell", Some("spell"))]
        );
        assert!(plugins.iter().all(|p| !p.global() && !p.tracks_usage()));
        let hint = plugins[0].keyword_row().unwrap();
        assert_eq!(hint.autocomplete.as_deref(), Some("define "));
    }

    #[test]
    fn defines_a_word_with_part_of_speech_and_copies_the_definition() {
        let platform = MockPlatform::empty();
        let plugin = define(&platform);
        let rows = plugin.query("  Dictionary ");
        assert!(rows[0].title.contains("alphabetical"), "{}", rows[0].title);
        assert!(
            rows[0].subtitle.starts_with("dictionary · noun"),
            "{}",
            rows[0].subtitle
        );
        assert_eq!(
            rows[0].action,
            Action::CopyText {
                text: rows[0].title.clone()
            }
        );
        let shift = &rows[0].secondary[0];
        assert_eq!(shift.modifier, Some(Modifier::Shift));
        assert!(
            matches!(&shift.action, Action::CopyText { text } if text.starts_with("dictionary (noun): "))
        );

        plugin.execute(&rows[0]).unwrap();
        assert_eq!(platform.clipboard.lock().unwrap().len(), 1);
        // Rows keep their order and Ctrl+L shows the definition (the title).
        assert!(rows.windows(2).all(|w| w[0].score > w[1].score));
        assert_eq!(rows[0].large_text, None);
    }

    #[test]
    fn several_parts_of_speech_each_get_rows() {
        let rows = define(&MockPlatform::empty()).query("run");
        let kinds: Vec<_> = rows
            .iter()
            .map(|r| r.subtitle.split(" · ").nth(1).unwrap().to_owned())
            .collect();
        assert!(kinds.contains(&"noun".to_owned()));
        assert!(kinds.contains(&"verb".to_owned()));
        assert!(rows.len() <= MAX_DEFINITIONS);
        let ids: std::collections::HashSet<_> = rows.iter().map(|r| &r.id).collect();
        assert_eq!(ids.len(), rows.len());
    }

    #[test]
    fn inflected_forms_are_explained_through_their_base() {
        let plugin = define(&MockPlatform::empty());
        let children = plugin.query("children");
        assert!(
            children[0].subtitle.contains("children is a form of child"),
            "{}",
            children[0].subtitle
        );
        assert!(children[0].subtitle.starts_with("child ·"));
        let running = plugin.query("running");
        assert!(running
            .iter()
            .any(|r| r.subtitle.starts_with("run ·")
                && r.subtitle.contains("running is a form of run")));
    }

    #[test]
    fn an_unknown_word_offers_corrections_that_complete_with_tab() {
        let rows = define(&MockPlatform::empty()).query("recieve");
        assert_eq!(rows[0].title, "No definition for \u{201c}recieve\u{201d}");
        let did_you_mean = rows
            .iter()
            .find(|r| r.title == "Did you mean \u{201c}receive\u{201d}?")
            .expect("receive is suggested");
        assert_eq!(did_you_mean.autocomplete.as_deref(), Some("receive"));
        assert_eq!(
            did_you_mean.action,
            Action::CopyText {
                text: "receive".into()
            }
        );
    }

    #[test]
    fn empty_input_explains_the_keyword() {
        let platform = MockPlatform::empty();
        assert_eq!(
            define(&platform).query("  ")[0].title,
            "Type a word to define"
        );
        assert_eq!(spell(&platform).query("")[0].title, "Type a word to check");
        // A pure-punctuation or absurdly long word is not looked up.
        assert_eq!(
            define(&platform).query("???")[0].title,
            "Type a word to define"
        );
        assert_eq!(
            define(&platform).query(&"x".repeat(100))[0].title,
            "Type a word to define"
        );
    }

    #[test]
    fn the_system_dictionary_wins_when_the_os_has_one() {
        let platform = MockPlatform::empty();
        platform.definitions.lock().unwrap().push((
            "serendipity".into(),
            "serendipity | ˌsɛrənˈdɪpɪti | noun\n• the occurrence of events by chance in a happy way\n• a lucky find".into(),
        ));
        let rows = define(&platform).query("serendipity");
        assert_eq!(
            titles(&rows),
            [
                "serendipity | ˌsɛrənˈdɪpɪti | noun",
                "the occurrence of events by chance in a happy way",
                "a lucky find"
            ]
        );
        assert_eq!(rows[1].subtitle, "serendipity · Dictionary · Enter to copy");

        // A word the OS does not know falls back to the bundled dictionary, and
        // `use_system = false` ignores the OS.
        assert!(define(&platform).query("dictionary")[0]
            .title
            .contains("alphabetical"));
        let off = plugins_on(&platform, false).remove(0);
        assert!(off.query("serendipity")[0].title.contains("good luck"));
    }

    #[test]
    fn spelling_suggests_pastes_and_matches_case() {
        let platform = MockPlatform::empty();
        let plugin = spell(&platform);
        let rows = plugin.query("Recieve");
        assert_eq!(rows[0].title, "Receive");
        assert!(rows[0].subtitle.starts_with("Instead of Recieve"));
        assert!(matches!(&rows[0].action, Action::PasteText { text, .. } if text == "Receive"));
        assert_eq!(rows[0].secondary[0].modifier, Some(Modifier::Ctrl));

        plugin.execute(&rows[0]).unwrap();
        assert_eq!(
            *platform.pasted.lock().unwrap(),
            [("Receive".to_owned(), false)]
        );
        plugin
            .execute(&rows[0].secondary_as_primary(0).unwrap())
            .unwrap();
        assert_eq!(*platform.clipboard.lock().unwrap(), ["Receive"]);

        assert_eq!(plugin.query("RECIEVE")[0].title, "RECEIVE");
        assert_eq!(plugin.query("recieve")[0].title, "receive");
    }

    #[test]
    fn where_pasting_is_not_possible_spelling_copies() {
        let platform = MockPlatform::empty();
        *platform.copy_only.lock().unwrap() = Some("no accessibility permission".into());
        let rows = spell(&platform).query("recieve");
        assert!(matches!(&rows[0].action, Action::CopyText { text } if text == "receive"));
        assert!(rows[0].subtitle.contains("Copies to clipboard"));
        assert!(rows[0].secondary.is_empty());
    }

    #[test]
    fn a_correct_word_and_a_hopeless_one() {
        let platform = MockPlatform::empty();
        let plugin = spell(&platform);
        let ok = plugin.query("receive");
        assert_eq!(ok[0].title, "\u{201c}receive\u{201d} is spelled correctly");
        let many = plugin.query("the quick brown fox");
        assert_eq!(many[0].title, "All 4 words are spelled correctly");
        // The first wrong word is the one corrected.
        let wrong = plugin.query("the quick brwn fox");
        assert!(wrong.iter().any(|r| r.title == "brown"));
        let none = plugin.query("qzxqzxqzxqzx");
        assert_eq!(
            none[0].title,
            "No suggestions for \u{201c}qzxqzxqzxqzx\u{201d}"
        );
    }

    #[test]
    fn the_os_spell_checker_is_preferred_when_it_answers() {
        let platform = MockPlatform::empty();
        platform.spellings.lock().unwrap().push((
            "teh".into(),
            Spelling {
                correct: false,
                suggestions: vec!["the".into(), "tech".into()],
            },
        ));
        platform.spellings.lock().unwrap().push((
            "kubernetes".into(),
            Spelling {
                correct: true,
                suggestions: Vec::new(),
            },
        ));
        let plugin = spell(&platform);
        assert_eq!(titles(&plugin.query("teh")), ["the", "tech"]);
        assert_eq!(
            plugin.query("kubernetes")[0].title,
            "\u{201c}kubernetes\u{201d} is spelled correctly"
        );
        // Words the OS did not answer for use the bundled list.
        assert_eq!(plugin.query("recieve")[0].title, "receive");
        // And `use_system = false` ignores the OS.
        let off = plugins_on(&platform, false).remove(1);
        assert_ne!(off.query("teh")[0].title, "tech");
    }

    #[test]
    fn parses_hunspell_word_lists() {
        let dic = "5\nhello/S\nWorld\nit's/M\nco-op\nnaïve\nzebra\n";
        assert_eq!(parse_hunspell_dic(dic), ["hello", "it's", "zebra"]);
        assert!(parse_hunspell_dic("").is_empty());
        assert_eq!(parse_hunspell_dic("apple\npear/X"), ["apple", "pear"]);
    }

    #[test]
    fn system_text_is_split_into_short_rows() {
        let parts = system_definition_parts("head word\n\n • first   meaning\n•second\n");
        assert_eq!(parts, ["head word", "first meaning", "second"]);
        let long = "x ".repeat(2000);
        let parts = system_definition_parts(&long);
        assert!(parts[0].chars().count() <= MAX_SYSTEM_CHARS + 1);
        assert!(parts[0].ends_with('…'));
    }

    #[test]
    fn case_matching() {
        assert_eq!(match_case("recieve", "receive"), "receive");
        assert_eq!(match_case("Recieve", "receive"), "Receive");
        assert_eq!(match_case("RECIEVE", "receive"), "RECEIVE");
        assert_eq!(match_case("I", "it"), "It");
        assert_eq!(match_case("", "x"), "x");
    }

    #[test]
    fn refresh_unpacks_the_bundled_dictionary() {
        let plugin = define(&MockPlatform::empty());
        plugin.refresh().unwrap();
        assert!(bundled().len() > 50_000);
    }
}
