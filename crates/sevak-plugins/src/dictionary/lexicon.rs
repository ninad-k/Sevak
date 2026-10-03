//! The bundled English dictionary: lookup, inflection and spelling.
//!
//! The data is Princeton WordNet 3.0, trimmed to single words and short
//! definitions by `scripts/build-dictionary.py` and stored zlib-compressed in
//! `data/wordnet-en.z` (about 2.7 MB; 87,000 words, 8 MB unpacked in memory).
//! WordNet's licence is in the file's header and in `THIRD_PARTY_NOTICES.md`.
//!
//! # Format
//!
//! After `#` header lines, one line per word, sorted:
//!
//! ```text
//! word TAB frequency TAB sense [TAB sense]...
//! ```
//!
//! A sense is `<pos> <definition>` (`n`, `v`, `a`, `r`) or `>base`, which marks
//! an irregular inflected form (`children` is `>child`). The frequency is the
//! number of times the word was tagged in WordNet's sample text: a rough
//! measure of how common it is, used to rank spelling suggestions.

use std::cmp::Ordering;
use std::sync::OnceLock;

/// A part of speech.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pos {
    Noun,
    Verb,
    Adjective,
    Adverb,
}

impl Pos {
    fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "n" => Some(Self::Noun),
            "v" => Some(Self::Verb),
            "a" => Some(Self::Adjective),
            "r" => Some(Self::Adverb),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Noun => "noun",
            Self::Verb => "verb",
            Self::Adjective => "adjective",
            Self::Adverb => "adverb",
        }
    }

    fn bit(self) -> u8 {
        match self {
            Self::Noun => N,
            Self::Verb => V,
            Self::Adjective => A,
            Self::Adverb => R,
        }
    }
}

// Part-of-speech sets, as bits.
const N: u8 = 1;
const V: u8 = 2;
const A: u8 = 4;
const R: u8 = 8;

/// One meaning of a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sense<'a> {
    pub pos: Pos,
    pub definition: &'a str,
}

/// What the dictionary knows about one word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<'a> {
    pub word: &'a str,
    pub frequency: u32,
    pub senses: Vec<Sense<'a>>,
    /// Set when the word is an irregular form of another (`children` -> `child`).
    pub form_of: Option<&'a str>,
}

impl Entry<'_> {
    fn pos_set(&self) -> u8 {
        self.senses
            .iter()
            .fold(0, |set, sense| set | sense.pos.bit())
    }
}

/// Words WordNet leaves out (it holds only nouns, verbs, adjectives and
/// adverbs) but a spell checker must accept.
#[rustfmt::skip]
const FUNCTION_WORDS: &[&str] = &[
    "a", "about", "above", "across", "after", "again", "against", "all", "also", "am", "an",
    "and", "any", "are", "aren't", "as", "at", "be", "because", "been", "before", "being",
    "below", "between", "both", "but", "by", "can", "can't", "cannot", "could", "couldn't",
    "did", "didn't", "do", "does", "doesn't", "doing", "don't", "down", "during", "each",
    "either", "else", "few", "for", "from", "further", "had", "hadn't", "has", "hasn't", "have",
    "haven't", "having", "he", "he'd", "he'll", "he's", "her", "here", "here's", "hers",
    "herself", "him", "himself", "his", "how", "how's", "i", "i'd", "i'll", "i'm", "i've", "if",
    "in", "into", "is", "isn't", "it", "it's", "its", "itself", "let's", "me", "more", "most",
    "mustn't", "my", "myself", "neither", "no", "nor", "not", "of", "off", "on", "once", "only",
    "or", "other", "ought", "our", "ours", "ourselves", "out", "over", "own", "same", "shan't",
    "she", "she'd", "she'll", "she's", "should", "shouldn't", "so", "some", "such", "than",
    "that", "that's", "the", "their", "theirs", "them", "themselves", "then", "there",
    "there's", "these", "they", "they'd", "they'll", "they're", "they've", "this", "those",
    "through", "to", "too", "under", "until", "up", "upon", "very", "was", "wasn't", "we",
    "we'd", "we'll", "we're", "we've", "were", "weren't", "what", "what's", "when", "when's",
    "where", "where's", "which", "while", "who", "who's", "whom", "whose", "why", "why's",
    "will", "with", "won't", "would", "wouldn't", "yet", "you", "you'd", "you'll", "you're",
    "you've", "your", "yours", "yourself", "yourselves", "okay", "ok",
];

/// How common function words count when ranking suggestions: very.
const FUNCTION_WORD_FREQUENCY: u32 = 1_000;

/// Verbs ending in a consonant-vowel-consonant whose last syllable is stressed,
/// so the consonant doubles (`occurred`, `beginning`, `referred`). Shorter
/// verbs double too, as the rule for one-syllable words; other longer verbs do
/// not (`visited`, `opening`), except a final `l` (`travelled`, British).
#[rustfmt::skip]
const STRESSED_FINAL: &[&str] = &[
    "abet", "acquit", "admit", "allot", "begin", "commit", "compel", "concur", "confer",
    "control", "defer", "deter", "emit", "equip", "excel", "expel", "forget", "impel", "incur",
    "infer", "occur", "omit", "outrun", "overrun", "patrol", "permit", "prefer", "propel",
    "rebel", "recur", "refer", "regret", "remit", "repel", "rerun", "submit", "transfer",
    "transmit", "unplug", "upset",
];

/// Inflection rules, WordNet's detachment rules plus a few: (suffix on the
/// word, text that replaces it to make the base form, parts of speech the base
/// form must have).
const RULES: &[(&str, &str, u8)] = &[
    ("ies", "y", N | V),
    ("ied", "y", V),
    ("ier", "y", A | R),
    ("iest", "y", A | R),
    ("ses", "s", N | V),
    ("xes", "x", N | V),
    ("zes", "z", N | V),
    ("ches", "ch", N | V),
    ("shes", "sh", N | V),
    ("men", "man", N),
    ("es", "e", N | V),
    ("es", "", N | V),
    ("s", "", N | V),
    ("ed", "e", V),
    ("ed", "", V),
    ("ing", "e", V),
    ("ing", "", V),
    ("er", "e", A | R),
    ("er", "", A | R),
    ("est", "e", A | R),
    ("est", "", A | R),
    ("ly", "", A),
    ("ily", "y", A),
];

/// Where the suggestion search looks at a glance: where a word starts, how long
/// it is, and which letters it has.
#[derive(Clone, Copy)]
struct Meta {
    start: u32,
    len: u8,
    /// Bit `n` set when the word contains the `n`th letter of the alphabet.
    /// `NOT_SUGGESTED` for words that are not plain lower-case letters.
    letters: u32,
}

const NOT_SUGGESTED: u32 = 1 << 31;

fn letter_set(word: &[u8]) -> u32 {
    let mut set = 0;
    for &b in word {
        if b.is_ascii_lowercase() {
            set |= 1 << (b - b'a');
        } else {
            return NOT_SUGGESTED;
        }
    }
    set
}

/// The dictionary and word list.
pub struct Lexicon {
    text: String,
    /// One per word line, in order.
    index: Vec<Meta>,
    /// Extra words for spelling only (system word lists): sorted, lower case.
    /// Set once; the lexicon is shared between plugin instances.
    extra: OnceLock<Vec<(String, u32)>>,
}

impl Lexicon {
    /// Builds a lexicon from text in the format described in the module docs.
    pub fn from_text(text: String) -> Self {
        let mut index = Vec::new();
        let mut start = 0usize;
        for line in text.split_inclusive('\n') {
            if !line.starts_with('#') && start <= u32::MAX as usize {
                if let Some((word, _)) = line.split_once('\t') {
                    index.push(Meta {
                        start: start as u32,
                        len: word.len().min(255) as u8,
                        letters: if word.len() > 255 {
                            NOT_SUGGESTED
                        } else {
                            letter_set(word.as_bytes())
                        },
                    });
                }
            }
            start += line.len();
        }
        Self {
            text,
            index,
            extra: OnceLock::new(),
        }
    }

    /// Sets words that count as correctly spelled and can be suggested, though
    /// they have no definition. Only the first call has an effect.
    pub fn set_spelling_words(&self, words: impl IntoIterator<Item = String>) {
        let mut words: Vec<String> = words.into_iter().map(|w| w.to_lowercase()).collect();
        words.sort_unstable();
        words.dedup();
        let _ = self.extra.set(
            words
                .into_iter()
                .map(|w| {
                    let letters = letter_set(w.as_bytes());
                    (w, letters)
                })
                .collect(),
        );
    }

    fn extra(&self) -> &[(String, u32)] {
        self.extra.get().map_or(&[], Vec::as_slice)
    }

    /// Number of words with a definition.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    fn word_at(&self, index: usize) -> &str {
        let meta = self.index[index];
        let start = meta.start as usize;
        // `len` was capped for absurd lines; such a word never matches anything.
        let line = self.text[start..].split('\n').next().unwrap_or("");
        line.split('\t').next().unwrap_or("")
    }

    fn position(&self, word: &str) -> Option<usize> {
        let mut low = 0;
        let mut high = self.index.len();
        while low < high {
            let mid = (low + high) / 2;
            match self.word_at(mid).cmp(word) {
                Ordering::Less => low = mid + 1,
                Ordering::Greater => high = mid,
                Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    /// The entry for exactly `word` (lower case).
    pub fn entry(&self, word: &str) -> Option<Entry<'_>> {
        let index = self.position(word)?;
        let start = self.index[index].start as usize;
        let line = self.text[start..].split('\n').next()?;
        let mut fields = line.split('\t');
        let word = fields.next()?;
        let frequency = fields.next()?.parse().unwrap_or(0);
        let mut senses = Vec::new();
        let mut form_of = None;
        for field in fields {
            if let Some(base) = field.strip_prefix('>') {
                form_of = Some(base);
            } else if let Some((tag, definition)) = field.split_once(' ') {
                if let Some(pos) = Pos::from_tag(tag) {
                    senses.push(Sense { pos, definition });
                }
            }
        }
        Some(Entry {
            word,
            frequency,
            senses,
            form_of,
        })
    }

    /// Whether `word` is a headword, a listed irregular form, a system-list word
    /// or a function word.
    pub fn has_word(&self, word: &str) -> bool {
        self.position(word).is_some() || self.extra_has(word) || FUNCTION_WORDS.contains(&word)
    }

    fn extra_has(&self, word: &str) -> bool {
        self.extra()
            .binary_search_by(|(w, _)| w.as_str().cmp(word))
            .is_ok()
    }

    /// The base forms `word` could be a regular or irregular inflection of that
    /// are in the dictionary, most plausible first: `running` -> `run`,
    /// `churches` -> `church`. The word itself is not included.
    ///
    /// A base form must have the part of speech the ending belongs to (`-ed`
    /// needs a verb, `-er` an adjective), and a short word ending in
    /// consonant-vowel-consonant must double its last letter (`stopped`, not
    /// `stoped`), which keeps `grammer` and `occured` from passing as
    /// inflections of `gramme` and `occur`.
    pub fn base_forms(&self, word: &str) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        if let Some(entry) = self.entry(word) {
            if let Some(base) = entry.form_of {
                found.push(base.to_owned());
            }
        }
        for (suffix, replacement, needs) in RULES {
            let Some(stem) = word.strip_suffix(suffix) else {
                continue;
            };
            if stem.is_empty() {
                continue;
            }
            let mut bases = vec![format!("{stem}{replacement}")];
            // stopp -> stop
            if replacement.is_empty() {
                if let Some(single) = undouble(stem) {
                    bases.push(single.to_owned());
                }
            }
            for base in bases {
                if base != word
                    && !found.contains(&base)
                    && self.accepts_as_base(&base, *needs)
                    && doubling_is_right(&base, word, suffix)
                {
                    found.push(base);
                }
            }
        }
        found
    }

    /// Whether `base` is a dictionary word with one of the parts of speech in
    /// `needs`. Words known only from a system list have no parts of speech and
    /// are accepted for any ending.
    fn accepts_as_base(&self, base: &str, needs: u8) -> bool {
        match self.entry(base) {
            Some(entry) => entry.pos_set() & needs != 0,
            None => self.extra_has(base),
        }
    }

    /// Whether `word` is spelled correctly: a headword, a function word, or an
    /// inflection of one. Case-insensitive; words with digits are accepted.
    pub fn is_correct(&self, word: &str) -> bool {
        let word = word.trim().to_lowercase();
        if word.is_empty() || word.chars().any(|c| c.is_ascii_digit()) {
            return true;
        }
        self.has_word(&word) || !self.base_forms(&word).is_empty()
    }

    /// Spelling suggestions for `word`, best first, at most `limit`.
    ///
    /// Candidates are the words within a Damerau-Levenshtein distance of 2
    /// (1 for words up to four letters). They are ranked by distance, then by
    /// being a rearrangement of the typed letters (`recieve` -> `receive`), then
    /// by sharing the first letter, then by how common the word is, then by how
    /// close the lengths are, then alphabetically.
    pub fn suggestions(&self, word: &str, limit: usize) -> Vec<String> {
        let typed_chars: Vec<char> = word.trim().to_lowercase().chars().collect();
        if typed_chars.is_empty() || typed_chars.len() > MAX_SUGGESTED_LEN {
            return Vec::new();
        }
        // Candidates are plain ASCII, so anything else only needs to differ.
        let typed: Vec<u8> = typed_chars
            .iter()
            .map(|&c| if c.is_ascii() { c as u8 } else { b'?' })
            .collect();
        let max_distance = if typed.len() <= 4 { 1 } else { 2 };
        let typed_letters = letter_set(&typed);
        let mut typed_sorted = typed.clone();
        typed_sorted.sort_unstable();

        let mut found: Vec<Candidate> = Vec::new();
        // Quick tests that rule most of the dictionary out before any distance
        // is computed: a word of the wrong length, or with too many letters of
        // the alphabet in or out (each edit changes at most two of them).
        let plausible = |len: usize, letters: u32| {
            letters & NOT_SUGGESTED == 0
                && len.abs_diff(typed.len()) <= max_distance
                && (typed_letters & NOT_SUGGESTED != 0
                    || (letters ^ typed_letters).count_ones() as usize <= 2 * max_distance)
        };
        let mut consider = |candidate: &str, frequency: u32| {
            let bytes = candidate.as_bytes();
            let Some(distance) = bounded_distance(&typed, bytes, max_distance) else {
                return;
            };
            if distance == 0 {
                return;
            }
            let mut sorted = bytes.to_vec();
            sorted.sort_unstable();
            found.push(Candidate {
                word: candidate.to_owned(),
                distance,
                rearranged: sorted == typed_sorted,
                same_first: bytes.first() == typed.first(),
                frequency,
                length_gap: bytes.len().abs_diff(typed.len()),
            });
        };
        for meta in &self.index {
            if !plausible(meta.len as usize, meta.letters) {
                continue;
            }
            let start = meta.start as usize;
            let end = start + meta.len as usize;
            let frequency = self.text[end + 1..]
                .split(['\t', '\n'])
                .next()
                .and_then(|f| f.parse().ok())
                .unwrap_or(0);
            consider(&self.text[start..end], frequency);
        }
        for (candidate, letters) in self.extra() {
            if plausible(candidate.len(), *letters) && self.position(candidate).is_none() {
                consider(candidate, 0);
            }
        }
        for candidate in FUNCTION_WORDS {
            if plausible(candidate.len(), letter_set(candidate.as_bytes()))
                && self.position(candidate).is_none()
            {
                consider(candidate, FUNCTION_WORD_FREQUENCY);
            }
        }

        found.sort_by(|a, b| {
            a.distance
                .cmp(&b.distance)
                .then_with(|| b.rearranged.cmp(&a.rearranged))
                .then_with(|| b.same_first.cmp(&a.same_first))
                .then_with(|| b.frequency.cmp(&a.frequency))
                .then_with(|| a.length_gap.cmp(&b.length_gap))
                .then_with(|| a.word.cmp(&b.word))
        });
        found.dedup_by(|a, b| a.word == b.word);
        found.into_iter().take(limit).map(|c| c.word).collect()
    }
}

/// Longest typed word that gets suggestions.
const MAX_SUGGESTED_LEN: usize = 40;

struct Candidate {
    word: String,
    distance: usize,
    rearranged: bool,
    same_first: bool,
    frequency: u32,
    length_gap: usize,
}

/// `stopp` -> `stop`: the stem without its doubled last consonant.
fn undouble(stem: &str) -> Option<&str> {
    let mut chars = stem.chars().rev();
    let (last, before) = (chars.next()?, chars.next()?);
    (last == before && last.is_ascii_alphabetic() && !is_vowel(last))
        .then(|| &stem[..stem.len() - last.len_utf8()])
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

/// Whether the base ends in consonant, vowel, consonant (not `w`, `x`, `y`).
fn ends_consonant_vowel_consonant(base: &str) -> bool {
    let b = base.as_bytes();
    if b.len() < 3 {
        return false;
    }
    let c = |i: usize| b[i].is_ascii_lowercase() && !is_vowel(b[i] as char);
    let v = |i: usize| is_vowel(b[i] as char);
    let n = b.len();
    c(n - 1) && !matches!(b[n - 1], b'w' | b'x' | b'y') && v(n - 2) && c(n - 3)
}

fn syllables(word: &str) -> usize {
    let mut count = 0;
    let mut in_vowels = false;
    for c in word.chars() {
        let vowel = is_vowel(c) || c == 'y';
        if vowel && !in_vowels {
            count += 1;
        }
        in_vowels = vowel;
    }
    count.max(1)
}

/// Whether the base form takes a doubled final consonant before `-ed`, `-ing`,
/// `-er` and `-est`.
fn doubles(base: &str) -> bool {
    ends_consonant_vowel_consonant(base) && (syllables(base) == 1 || STRESSED_FINAL.contains(&base))
}

/// For `-ed`, `-ing`, `-er` and `-est` forms of a consonant-vowel-consonant
/// base: the last letter is doubled exactly where English doubles it. Other
/// endings and bases are not constrained.
fn doubling_is_right(base: &str, word: &str, suffix: &str) -> bool {
    if !matches!(suffix, "ed" | "ing" | "er" | "est") || !ends_consonant_vowel_consonant(base) {
        return true;
    }
    let Some(stem) = word.strip_suffix(suffix) else {
        return true;
    };
    if stem == base {
        !doubles(base)
    } else {
        // Doubled: right for short words, the stressed list and British `-lled`.
        doubles(base) || base.ends_with('l')
    }
}

/// The optimal-string-alignment distance between `a` and `b` (insert, delete,
/// substitute, swap two neighbours), or `None` if it exceeds `max`. Words up to
/// 62 bytes; longer ones are never within reach of a suggestion.
pub fn bounded_distance(a: &[u8], b: &[u8], max: usize) -> Option<usize> {
    const WIDTH: usize = 64;
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    if a.len() >= WIDTH - 1 || b.len() >= WIDTH - 1 {
        return None;
    }
    let (n, m) = (a.len(), b.len());
    // Three rows are enough: the swap rule looks two rows back.
    let mut before = [0usize; WIDTH];
    let mut previous = [0usize; WIDTH];
    let mut current = [0usize; WIDTH];
    for (j, slot) in previous.iter_mut().enumerate().take(m + 1) {
        *slot = j;
    }
    for i in 1..=n {
        current[0] = i;
        let mut row_min = current[0];
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (previous[j] + 1)
                .min(current[j - 1] + 1)
                .min(previous[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(before[j - 2] + 1);
            }
            current[j] = best;
            row_min = row_min.min(best);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut before, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    let distance = previous[m];
    (distance <= max).then_some(distance)
}

#[cfg(test)]
pub(crate) mod fixture {
    use super::Lexicon;

    /// A tiny dictionary in the real format.
    pub fn tiny() -> Lexicon {
        Lexicon::from_text(
            "#WordNet fixture\n\
             #licence text would be here\n\
             apple\t12\tn the fruit of the apple tree\tn a tree of the rose family\n\
             child\t30\tn a young person\n\
             children\t0\t>child\n\
             church\t20\tn a place for Christian worship\n\
             gramme\t1\tn a metric unit of mass\n\
             occur\t30\tv happen\n\
             receive\t40\tv get something\tv experience as a reaction\n\
             recipe\t8\tn directions for cooking\n\
             relieve\t5\tv make less severe\n\
             run\t60\tn a score in baseball\tv move fast by using one's feet\ta (of a liquid) flowing\n\
             stop\t55\tn the act of stopping\tv cease moving\n\
             tall\t7\ta of great height\n\
             travel\t20\tv go on a journey\n\
             tree\t25\tn a tall perennial woody plant\n\
             visit\t18\tv go to see\n\
             well-known\t3\ta widely known\n\
             wonderful\t9\ta extraordinarily good\n"
                .to_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::tiny;
    use super::*;

    #[test]
    fn looks_up_entries_with_senses_by_part_of_speech() {
        let lexicon = tiny();
        assert_eq!(lexicon.len(), 17);
        let run = lexicon.entry("run").unwrap();
        assert_eq!(run.frequency, 60);
        assert_eq!(run.senses.len(), 3);
        assert_eq!(run.senses[0].pos, Pos::Noun);
        assert_eq!(run.senses[1].pos, Pos::Verb);
        assert_eq!(run.senses[1].definition, "move fast by using one's feet");
        assert_eq!(run.senses[2].pos, Pos::Adjective);
        assert!(lexicon.entry("missing").is_none());
        assert!(lexicon.entry("").is_none());
        // The header is not an entry.
        assert!(lexicon.entry("#wordnet fixture").is_none());
    }

    #[test]
    fn irregular_forms_point_to_their_base() {
        let lexicon = tiny();
        let children = lexicon.entry("children").unwrap();
        assert_eq!(children.form_of, Some("child"));
        assert!(children.senses.is_empty());
        assert_eq!(lexicon.base_forms("children"), ["child"]);
    }

    #[test]
    fn regular_inflections_resolve_to_a_headword() {
        let lexicon = tiny();
        assert_eq!(lexicon.base_forms("apples"), ["apple"]);
        assert_eq!(lexicon.base_forms("churches"), ["church"]);
        assert_eq!(lexicon.base_forms("running"), ["run"]);
        assert_eq!(lexicon.base_forms("stopped"), ["stop"]);
        assert_eq!(lexicon.base_forms("received"), ["receive"]);
        assert_eq!(lexicon.base_forms("receiving"), ["receive"]);
        assert_eq!(lexicon.base_forms("taller"), ["tall"]);
        assert!(lexicon.base_forms("zzzs").is_empty());
        assert!(lexicon.base_forms("apple").is_empty());
    }

    #[test]
    fn an_ending_needs_a_base_with_the_right_part_of_speech() {
        let lexicon = tiny();
        // `gramme` is only a noun, so it has no comparative.
        assert!(lexicon.base_forms("grammer").is_empty());
        // `tree` is only a noun: no past tense.
        assert!(lexicon.base_forms("treed").is_empty());
        assert!(lexicon.base_forms("trees").contains(&"tree".to_owned()));
    }

    #[test]
    fn short_words_double_their_last_letter_and_long_ones_do_not() {
        let lexicon = tiny();
        for ok in [
            "stopped",
            "stopping",
            "running",
            "occurred",
            "occurring",
            "visited",
            "visiting",
            "travelled",
            "traveled",
            "received",
        ] {
            assert!(lexicon.is_correct(ok), "{ok}");
        }
        for bad in [
            "stoped",
            "stoping",
            "runing",
            "occured",
            "occuring",
            "visitted",
            "visitting",
            "recieved",
        ] {
            assert!(!lexicon.is_correct(bad), "{bad}");
        }
    }

    #[test]
    fn spelling_correctness() {
        let lexicon = tiny();
        for ok in [
            "apple",
            "Apple",
            "APPLES",
            "children",
            "running",
            "the",
            "don't",
            "route66",
            "well-known",
        ] {
            assert!(lexicon.is_correct(ok), "{ok}");
        }
        for bad in ["recieve", "aple", "wonderfull", "xyzzy"] {
            assert!(!lexicon.is_correct(bad), "{bad}");
        }
    }

    #[test]
    fn extra_spelling_words_count_but_have_no_definition() {
        let lexicon = tiny();
        lexicon.set_spelling_words(["Kubernetes".to_owned(), "apple".to_owned()]);
        assert!(lexicon.is_correct("kubernetes"));
        assert!(lexicon.entry("kubernetes").is_none());
        assert!(lexicon
            .suggestions("kubernetis", 3)
            .contains(&"kubernetes".to_owned()));
        // They are bases for regular inflections too.
        assert!(lexicon.is_correct("kuberneteses"));
    }

    #[test]
    fn suggestions_are_ranked_by_distance_then_commonness() {
        let lexicon = tiny();
        // One transposition away from "receive"; "relieve" is one substitution
        // away but not a rearrangement; "recipe" is further.
        assert_eq!(lexicon.suggestions("recieve", 3)[0], "receive");
        assert_eq!(lexicon.suggestions("wonderfull", 3), ["wonderful"]);
        assert_eq!(lexicon.suggestions("aple", 3), ["apple"]);
        // Distance 1 beats distance 2 whatever the frequency.
        let s = lexicon.suggestions("chuch", 5);
        assert_eq!(s[0], "church");
        assert!(lexicon.suggestions("xyzzyxyzzy", 5).is_empty());
        assert!(lexicon.suggestions("", 5).is_empty());
        // Correct words are not their own suggestion.
        assert!(!lexicon
            .suggestions("apple", 5)
            .contains(&"apple".to_owned()));
        assert!(lexicon.suggestions("recieve", 1).len() <= 1);
        // Absurdly long input is not searched.
        assert!(lexicon.suggestions(&"a".repeat(41), 5).is_empty());
        // Non-ASCII letters still find their plain spelling.
        assert_eq!(lexicon.suggestions("rëceive", 1), ["receive"]);
    }

    #[test]
    fn rearranged_letters_and_the_first_letter_and_frequency_break_ties() {
        let lexicon = Lexicon::from_text(
            "cables\t99\tn x\n\
             fables\t5\tn x\n\
             gables\t50\tn x\n\
             mabels\t1\tn x\n\
             tables\t1\tn x\n"
                .to_owned(),
        );
        // "mables" is one edit from each; "mabels" is a rearrangement of it
        // (and shares the first letter), so it leads whatever the frequencies;
        // the others go by frequency.
        assert_eq!(
            lexicon.suggestions("mables", 5),
            ["mabels", "cables", "gables", "fables", "tables"]
        );
        // Without a rearrangement, sharing the first letter beats frequency.
        let lexicon = Lexicon::from_text("mabled\t1\tn x\ntables\t99\tn x\n".to_owned());
        assert_eq!(lexicon.suggestions("mables", 2), ["mabled", "tables"]);
    }

    #[test]
    fn function_words_are_suggested_once_and_rank_as_common() {
        let lexicon = Lexicon::from_text("ten\t5\tn x\n".to_owned());
        // "the" (a function word) beats the rarer dictionary word "ten" for the
        // classic typo, and nothing is listed twice.
        let s = lexicon.suggestions("teh", 5);
        assert_eq!(s[0], "the");
        assert_eq!(s.iter().filter(|w| *w == "ten").count(), 1);
        let lexicon = Lexicon::from_text("the\t1\tn x\n".to_owned());
        assert_eq!(
            lexicon
                .suggestions("teh", 5)
                .iter()
                .filter(|w| *w == "the")
                .count(),
            1
        );
    }

    #[test]
    fn damerau_levenshtein_with_a_bound() {
        let d = |a: &str, b: &str, max| bounded_distance(a.as_bytes(), b.as_bytes(), max);
        assert_eq!(d("kitten", "kitten", 2), Some(0));
        assert_eq!(d("recieve", "receive", 2), Some(1)); // swap
        assert_eq!(d("aple", "apple", 2), Some(1)); // insertion
        assert_eq!(d("appple", "apple", 2), Some(1)); // deletion
        assert_eq!(d("cat", "cut", 2), Some(1)); // substitution
        assert_eq!(d("kitten", "sitting", 2), None);
        assert_eq!(d("kitten", "sitting", 3), Some(3));
        assert_eq!(d("a", "abcd", 2), None);
        assert_eq!(d("", "ab", 2), Some(2));
        assert_eq!(d(&"a".repeat(70), &"a".repeat(70), 2), None);
    }

    #[test]
    fn letter_sets_let_obvious_non_matches_be_skipped() {
        assert_eq!(letter_set(b"abc"), 0b111);
        assert_eq!(letter_set(b"don't") & NOT_SUGGESTED, NOT_SUGGESTED);
        assert_eq!(letter_set(b""), 0);
    }

    #[test]
    fn the_bundled_dictionary_is_well_formed() {
        let lexicon = super::super::bundled();
        assert!(lexicon.len() > 50_000, "{}", lexicon.len());
        let dictionary = lexicon.entry("dictionary").expect("dictionary");
        assert!(dictionary.senses[0].definition.contains("alphabetical"));
        assert_eq!(lexicon.entry("children").unwrap().form_of, Some("child"));
        for ok in [
            "running",
            "the",
            "occurred",
            "stopped",
            "dictionaries",
            "happily",
            "bigger",
        ] {
            assert!(lexicon.is_correct(ok), "{ok}");
        }
        for bad in [
            "recieve",
            "occured",
            "stoped",
            "grammer",
            "runing",
            "definately",
        ] {
            assert!(!lexicon.is_correct(bad), "{bad}");
        }
    }

    #[test]
    fn the_bundled_dictionary_corrects_common_typos() {
        let lexicon = super::super::bundled();
        let first = |typo: &str| lexicon.suggestions(typo, 3);
        for (typo, wanted) in [
            ("recieve", "receive"),
            ("definately", "definitely"),
            ("seperate", "separate"),
            ("occured", "occurred"),
            ("accomodate", "accommodate"),
            ("teh", "the"),
            ("beleive", "believe"),
            ("neccessary", "necessary"),
            ("tommorow", "tomorrow"),
            ("grammer", "grammar"),
            ("freind", "friend"),
            ("wierd", "weird"),
        ] {
            assert_eq!(first(typo)[0], wanted, "{typo}: {:?}", first(typo));
        }
    }
}
