//! Emoji picker: `emoji smile` or `:smile` shows matching emoji as a grid of
//! tiles, and Enter pastes the selected one into the app you were in.
//!
//! The list (about 1,900 emoji with their Unicode names and CLDR keywords) is
//! bundled in `data/emoji.tsv`, one `glyph<TAB>name<TAB>keyword|keyword` line
//! each, and read once on first use. Nothing is downloaded: regenerate the file
//! with `node scripts/generate-emoji.mjs` when a new Unicode version is wanted.
//! Skin-tone variants are left out; the base emoji is pasted and apps apply
//! their own tone setting.
//!
//! The plugin comes as two instances of one family, because a plugin has one
//! keyword: `emoji:word` (keyword `emoji`) and `emoji:colon` (keyword `:`, so
//! `:smile` works without a space). Both answer the same way; disabling the
//! family `emoji` turns off both.
//!
//! # Matching
//!
//! The query is split into words (surrounding `:` are ignored, so `:thumbs up:`
//! works) and every word must match the emoji's name or keywords. A word scores
//! highest as a whole name, then as a whole word of the name, a prefix of a
//! name word, a keyword, a prefix of a keyword, and last as a substring. The
//! name without spaces also counts (`thumbsup`). Ties keep Unicode's order, so
//! faces come first. An empty query lists the first emoji of the list.

use std::sync::{Arc, OnceLock};

use sevak_core::config::PasteConfig;
use sevak_core::model::score;
use sevak_core::{Action, Modifier, Plugin, PluginResult, PreviewHint, ResultItem};
use sevak_platform::{PasteSupport, PlatformProvider};

use crate::actions::execute_action;

const DATA: &str = include_str!("../data/emoji.tsv");

/// Most tiles one answer holds (the engine's grid limit).
const MAX_TILES: usize = sevak_core::engine::GRID_MAX_RESULTS;

/// One emoji with the lowercase forms it is matched against.
#[derive(Debug)]
struct Emoji {
    glyph: String,
    name: String,
    keywords: Vec<String>,
    /// The name's words, lowercase.
    words: Vec<String>,
    /// The name lowercase, letters and digits only (`thumbsup`).
    compact: String,
}

fn catalog() -> &'static [Emoji] {
    static CATALOG: OnceLock<Vec<Emoji>> = OnceLock::new();
    CATALOG.get_or_init(|| parse(DATA))
}

fn parse(data: &str) -> Vec<Emoji> {
    data.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let glyph = fields.next()?.trim();
            let name = fields.next()?.trim();
            if glyph.is_empty() || name.is_empty() {
                return None;
            }
            let keywords = fields
                .next()
                .unwrap_or("")
                .split('|')
                .map(|word| word.trim().to_lowercase())
                .filter(|word| !word.is_empty())
                .collect();
            let lower = name.to_lowercase();
            Some(Emoji {
                glyph: glyph.to_owned(),
                name: name.to_owned(),
                keywords,
                words: lower
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|word| !word.is_empty())
                    .map(str::to_owned)
                    .collect(),
                compact: lower.chars().filter(|c| c.is_alphanumeric()).collect(),
            })
        })
        .collect()
}

/// How well `token` (lowercase) matches `emoji`; `None` when it does not.
fn token_score(emoji: &Emoji, token: &str) -> Option<f64> {
    let compact_token: String = token.chars().filter(|c| c.is_alphanumeric()).collect();
    let best = if token == emoji.glyph {
        1000.0
    } else if (emoji.words.len() == 1 && emoji.words[0] == token) || emoji.compact == compact_token
    {
        900.0
    } else if emoji.words.iter().any(|word| word == token) {
        800.0
    } else if emoji.words.iter().any(|word| word.starts_with(token)) {
        600.0
    } else if emoji.keywords.iter().any(|word| word == token) {
        500.0
    } else if emoji.keywords.iter().any(|word| word.starts_with(token)) {
        400.0
    } else if emoji.words.iter().any(|word| word.contains(token)) {
        200.0
    } else if !compact_token.is_empty() && emoji.compact.contains(&compact_token) {
        150.0
    } else if emoji.keywords.iter().any(|word| word.contains(token)) {
        100.0
    } else {
        return None;
    };
    Some(best)
}

/// Splits a query into lowercase words without surrounding colons.
fn tokens(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|word| word.trim_matches(':').to_lowercase())
        .filter(|word| !word.is_empty())
        .collect()
}

/// The emoji matching `query`, best first (at most `limit`), with scores.
fn search(query: &str, limit: usize) -> Vec<(&'static Emoji, f64)> {
    let words = tokens(query);
    let all = catalog();
    if words.is_empty() {
        return all
            .iter()
            .take(limit)
            .enumerate()
            .map(|(i, emoji)| (emoji, score::KEYWORD - 1.0 - i as f64))
            .collect();
    }
    let mut hits: Vec<(usize, &Emoji, f64)> = all
        .iter()
        .enumerate()
        .filter_map(|(index, emoji)| {
            let mut total = 0.0;
            for word in &words {
                total += token_score(emoji, word)?;
            }
            Some((index, emoji, total))
        })
        .collect();
    // Best first; equal scores keep Unicode's order.
    hits.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
    hits.into_iter()
        .take(limit)
        .map(|(_, emoji, total)| (emoji, total.min(score::KEYWORD - 2.0)))
        .collect()
}

/// `1F600` or `2764 FE0F`-style code points of `glyph`, as `U+1F600 U+FE0F`.
fn code_points(glyph: &str) -> String {
    glyph
        .chars()
        .map(|c| format!("U+{:04X}", c as u32))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Which keyword an instance answers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// `emoji smile`
    Word,
    /// `:smile`
    Colon,
}

/// The emoji picker.
pub struct EmojiPlugin {
    trigger: Trigger,
    restore_clipboard: bool,
    platform: Arc<dyn PlatformProvider>,
}

impl EmojiPlugin {
    pub fn new(trigger: Trigger, paste: &PasteConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        Self {
            trigger,
            restore_clipboard: paste.restore_clipboard,
            platform,
        }
    }

    fn row(&self, emoji: &Emoji, support: &PasteSupport, score: f64) -> ResultItem {
        let (action, hint) = match support {
            PasteSupport::Available => (
                Action::PasteText {
                    text: emoji.glyph.clone(),
                    restore_clipboard: self.restore_clipboard,
                },
                "Enter to paste",
            ),
            PasteSupport::CopyOnly(_) => (
                Action::CopyText {
                    text: emoji.glyph.clone(),
                },
                "Enter to copy",
            ),
        };
        let mut item = ResultItem::new(self.id(), &emoji.glyph, &emoji.name, action)
            .with_subtitle(format!("{} · {hint}", emoji.glyph))
            .with_score(score)
            .as_tile(Some(&emoji.glyph));
        if matches!(support, PasteSupport::Available) {
            item = item.with_secondary(
                "Copy emoji",
                Some(Modifier::Shift),
                Action::CopyText {
                    text: emoji.glyph.clone(),
                },
            );
        }
        item
    }

    fn lookup(&self, item: &ResultItem) -> Option<&'static Emoji> {
        let glyph = item.id.strip_prefix(self.id())?.strip_prefix(':')?;
        catalog().iter().find(|emoji| emoji.glyph == glyph)
    }
}

impl Plugin for EmojiPlugin {
    fn id(&self) -> &str {
        match self.trigger {
            Trigger::Word => "emoji:word",
            Trigger::Colon => "emoji:colon",
        }
    }

    fn name(&self) -> &str {
        match self.trigger {
            Trigger::Word => "Emoji picker",
            Trigger::Colon => "Emoji picker (:)",
        }
    }

    fn description(&self) -> &str {
        match self.trigger {
            Trigger::Word => {
                "Type `emoji ` and a name to pick an emoji from a grid; Enter pastes it (offline)."
            }
            Trigger::Colon => "Type `:` and a name (`:heart`) to pick an emoji; same as `emoji `.",
        }
    }

    fn keyword(&self) -> Option<&str> {
        Some(match self.trigger {
            Trigger::Word => "emoji",
            Trigger::Colon => ":",
        })
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let support = self.platform.paste_support();
        search(input, MAX_TILES)
            .into_iter()
            .map(|(emoji, score)| self.row(emoji, &support, score))
            .collect()
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        execute_action(self.platform.as_ref(), &item.action)
    }

    fn preview(&self, item: &ResultItem) -> Option<PreviewHint> {
        let emoji = self.lookup(item)?;
        let mut rows = vec![("Name".to_owned(), emoji.name.clone())];
        if !emoji.keywords.is_empty() {
            rows.push(("Keywords".to_owned(), emoji.keywords.join(", ")));
        }
        rows.push(("Code points".to_owned(), code_points(&emoji.glyph)));
        Some(PreviewHint::Details { rows })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;

    fn names(query: &str) -> Vec<&'static str> {
        search(query, 60)
            .into_iter()
            .map(|(emoji, _)| emoji.name.as_str())
            .collect()
    }

    fn plugin(trigger: Trigger) -> (EmojiPlugin, Arc<MockPlatform>) {
        let platform = MockPlatform::empty();
        (
            EmojiPlugin::new(
                trigger,
                &PasteConfig {
                    restore_clipboard: true,
                },
                platform.clone(),
            ),
            platform,
        )
    }

    #[test]
    fn the_bundled_list_is_complete_and_well_formed() {
        let all = catalog();
        assert!(all.len() > 1500, "{} emoji", all.len());
        assert!(all
            .iter()
            .all(|e| !e.glyph.is_empty() && !e.name.is_empty()));
        assert!(!DATA.contains("\r"));
        // Unique glyphs: they are the result keys.
        let mut glyphs: Vec<&str> = all.iter().map(|e| e.glyph.as_str()).collect();
        glyphs.sort_unstable();
        glyphs.dedup();
        assert_eq!(glyphs.len(), all.len());
        assert_eq!(all[0].glyph, "😀");
        assert_eq!(all[0].name, "grinning face");
    }

    #[test]
    fn names_keywords_and_prefixes_find_emoji() {
        assert_eq!(names("thumbs up")[0], "thumbs up");
        assert_eq!(names("thumbsup")[0], "thumbs up");
        assert_eq!(names(":thumbs up:")[0], "thumbs up");
        assert!(names("heart").contains(&"red heart"));
        // A keyword, not part of the name.
        assert!(names("lol").contains(&"grinning face with smiling eyes"));
        // Prefixes of words.
        assert!(names("smil")[0].contains("smil"));
        // The emoji itself finds itself.
        assert_eq!(names("🔥")[0], "fire");
    }

    #[test]
    fn every_word_must_match_and_better_matches_come_first() {
        let both = names("red heart");
        assert_eq!(both[0], "red heart");
        assert!(names("heart zzzzqq").is_empty());
        // An exact word beats a prefix: "cat" before "category"-like words.
        let cats = names("cat");
        assert_eq!(cats[0], "cat");
        // Case does not matter.
        assert_eq!(names("FIRE")[0], "fire");
    }

    #[test]
    fn an_empty_query_lists_the_first_emoji_in_unicode_order() {
        let first = search("", 5);
        assert_eq!(first.len(), 5);
        assert_eq!(first[0].0.name, "grinning face");
        assert!(first.windows(2).all(|w| w[0].1 > w[1].1));
        assert_eq!(search(":", 5).len(), 5);
        assert_eq!(search("  ", 3).len(), 3);
    }

    #[test]
    fn answers_are_bounded_and_below_keyword_scores() {
        let rows = search("a", MAX_TILES);
        assert!(rows.len() <= MAX_TILES);
        assert!(rows.iter().all(|(_, s)| *s < score::KEYWORD));
        assert!(search("zzzzzzzz", 10).is_empty());
    }

    #[test]
    fn rows_are_tiles_that_paste() {
        let (plugin, _) = plugin(Trigger::Word);
        let rows = plugin.query("fire");
        let first = &rows[0];
        assert_eq!(first.id, "emoji:word:🔥");
        assert_eq!(first.title, "fire");
        assert!(first.is_tile());
        assert_eq!(
            first.view,
            Some(sevak_core::ViewHint::Grid {
                glyph: Some("🔥".into())
            })
        );
        assert_eq!(
            first.action,
            Action::PasteText {
                text: "🔥".into(),
                restore_clipboard: true
            }
        );
        assert_eq!(first.secondary.len(), 1);
        assert_eq!(first.secondary[0].modifier, Some(Modifier::Shift));
        assert!(rows.iter().all(ResultItem::is_tile));
    }

    #[test]
    fn where_pasting_is_unavailable_the_emoji_is_copied() {
        let (plugin, platform) = plugin(Trigger::Colon);
        *platform.copy_only.lock().unwrap() = Some("no paste here".into());
        let first = plugin.query("fire").remove(0);
        assert_eq!(first.id, "emoji:colon:🔥");
        assert_eq!(
            first.action,
            Action::CopyText {
                text: "🔥".into()
            }
        );
        assert!(first.secondary.is_empty());
        plugin.execute(&first).unwrap();
        assert_eq!(*platform.clipboard.lock().unwrap(), vec!["🔥".to_owned()]);
    }

    #[test]
    fn enter_pastes_and_the_secondary_copies() {
        let (plugin, platform) = plugin(Trigger::Word);
        let first = plugin.query("fire").remove(0);
        plugin.execute(&first).unwrap();
        assert_eq!(
            *platform.pasted.lock().unwrap(),
            vec![("🔥".to_owned(), true)]
        );
        let copy = first.secondary_as_primary(0).unwrap();
        plugin.execute(&copy).unwrap();
        assert_eq!(*platform.clipboard.lock().unwrap(), vec!["🔥".to_owned()]);
    }

    #[test]
    fn the_two_instances_have_their_own_keywords() {
        let (word, _) = plugin(Trigger::Word);
        let (colon, _) = plugin(Trigger::Colon);
        assert_eq!((word.id(), word.keyword()), ("emoji:word", Some("emoji")));
        assert_eq!((colon.id(), colon.keyword()), ("emoji:colon", Some(":")));
        assert!(!word.global() && !colon.global());
    }

    #[test]
    fn the_preview_names_keywords_and_code_points() {
        let (plugin, _) = plugin(Trigger::Word);
        let first = plugin.query("red heart").remove(0);
        let Some(PreviewHint::Details { rows }) = plugin.preview(&first) else {
            panic!("expected details");
        };
        assert_eq!(rows[0], ("Name".to_owned(), "red heart".to_owned()));
        assert!(rows
            .iter()
            .any(|(label, value)| label == "Keywords" && value.contains("love")));
        assert!(rows
            .iter()
            .any(|(label, value)| label == "Code points" && value.starts_with("U+2764")));
        let stranger = ResultItem::new(
            "emoji:word",
            "nope",
            "x",
            Action::CopyText { text: "x".into() },
        );
        assert_eq!(plugin.preview(&stranger), None);
    }

    #[test]
    fn code_points_are_formatted() {
        assert_eq!(code_points("😀"), "U+1F600");
        assert_eq!(code_points("❤️"), "U+2764 U+FE0F");
    }
}
