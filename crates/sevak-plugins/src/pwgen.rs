//! Password generator, offline: `pass`.
//!
//! | You type | You get |
//! |---|---|
//! | `pass` | a password of `[passwords] length` characters, one without symbols, and a passphrase |
//! | `pass 24` | passwords of 24 characters, with and without symbols |
//! | `pass 6 words` | passphrases of six words (`pass words` uses `[passwords] words`) |
//! | `pass 16 nosymbols`, `pass symbols` | passwords with only letters and digits, or with symbols |
//! | `pass pin`, `pass pin 8` | numbers (six digits unless a length is given) |
//!
//! Every row shows its strength in bits of entropy: how many guesses an
//! attacker who knows exactly how it was made would need, as a power of two.
//!
//! # Randomness
//!
//! Every character and word is drawn from the operating system's
//! cryptographically secure random number generator (`getrandom`), uniformly:
//! a random number is rejected rather than reduced with a bias. A password
//! with several kinds of characters is drawn again until it has at least one
//! of each (sites often insist on it); the entropy shown accounts for that.
//!
//! # Privacy
//!
//! Generated values are never written anywhere, logged, or recorded in the
//! usage statistics or search history ([`Plugin::tracks_usage`] is false; row
//! ids are positions, never values). Enter copies through
//! [`PlatformProvider::set_clipboard_text_private`], which marks the text as a
//! secret so that clipboard histories (Sevak's own, Windows Win+V, and the
//! macOS and KDE ones that honour the marker) leave it out.
//!
//! The passphrase words are Sevak's own list (`data/passphrase-words.txt`).

use std::sync::{Arc, OnceLock};

use sevak_core::config::{
    PasswordsConfig, MAX_PASSPHRASE_WORDS, MAX_PASSWORD_LENGTH, MIN_PASSPHRASE_WORDS,
    MIN_PASSWORD_LENGTH,
};
use sevak_core::model::score;
use sevak_core::{Action, IconSource, Plugin, PluginError, PluginResult, ResultItem};
use sevak_platform::PlatformProvider;
use zeroize::Zeroize;

pub const KEYWORD: &str = "pass";
const ID: &str = "pwgen";
const PAYLOAD_NOTHING: &str = "nothing";

const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
/// Symbols most sites accept, and none that need quoting in a shell or form
/// (no quotes, backslash, spaces or brackets).
const SYMBOLS: &[u8] = b"!#$%&*+-.:=?@^_~";

/// Rows offered for an explicit kind (`pass 6 words`): a few to choose from.
const CHOICES: usize = 3;
/// Digits in a `pass pin` without a length, and the most allowed.
const PIN_DIGITS: usize = 6;
const MAX_PIN_DIGITS: usize = 32;
/// Between the words of a passphrase.
const SEPARATOR: char = '-';

static WORD_DATA: &str = include_str!("../data/passphrase-words.txt");

/// The passphrase words (lines of the bundled list that are not comments).
pub fn words() -> &'static [&'static str] {
    static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| {
        WORD_DATA
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect()
    })
}

/// The kind of secret asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Characters; `symbols` adds [`SYMBOLS`] to letters and digits.
    Chars {
        length: usize,
        symbols: bool,
    },
    Words {
        count: usize,
    },
    Pin {
        digits: usize,
    },
}

/// What the text after the keyword asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The secrets to show, one row each.
    pub rows: Vec<Kind>,
}

impl Request {
    /// Parses the words after the keyword: a number, `words`, `pin`,
    /// `symbols` / `nosymbols`, in any order. `None` for anything else.
    pub fn parse(input: &str, config: &PasswordsConfig) -> Option<Self> {
        enum Mode {
            Chars,
            Words,
            Pin,
        }
        let mut mode = Mode::Chars;
        let mut number: Option<usize> = None;
        let mut symbols: Option<bool> = None;
        let mut previous_no = false;
        for word in input.split_whitespace() {
            let word = word.to_ascii_lowercase();
            let no = std::mem::take(&mut previous_no);
            match word.as_str() {
                _ if word.chars().all(|c| c.is_ascii_digit()) => {
                    if number.is_some() {
                        return None;
                    }
                    // Digits only, so the sole failure is overflow: "a lot".
                    number = Some(word.parse().unwrap_or(usize::MAX));
                }
                "word" | "words" | "phrase" | "passphrase" | "w" => mode = Mode::Words,
                "pin" | "digits" | "numeric" => mode = Mode::Pin,
                "chars" | "characters" | "char" => mode = Mode::Chars,
                "symbols" | "symbol" | "sym" | "+s" if no => symbols = Some(false),
                "symbols" | "symbol" | "sym" | "+s" => symbols = Some(true),
                "nosymbols" | "nosymbol" | "nosym" | "no-symbols" | "alnum" | "simple" | "-s" => {
                    symbols = Some(false);
                }
                "no" | "without" => previous_no = true,
                _ => return None,
            }
        }
        if previous_no {
            return None;
        }
        let rows = match mode {
            Mode::Words => {
                let count = number
                    .unwrap_or(config.words)
                    .clamp(MIN_PASSPHRASE_WORDS, MAX_PASSPHRASE_WORDS);
                vec![Kind::Words { count }; CHOICES]
            }
            Mode::Pin => {
                let digits = number
                    .unwrap_or(PIN_DIGITS)
                    .clamp(MIN_PASSWORD_LENGTH, MAX_PIN_DIGITS);
                vec![Kind::Pin { digits }; CHOICES]
            }
            Mode::Chars => {
                let length = number
                    .unwrap_or(config.length)
                    .clamp(MIN_PASSWORD_LENGTH, MAX_PASSWORD_LENGTH);
                match symbols {
                    Some(symbols) => vec![Kind::Chars { length, symbols }; CHOICES],
                    None => {
                        let mut rows = vec![Kind::Chars {
                            length,
                            symbols: config.symbols,
                        }];
                        if config.symbols {
                            rows.push(Kind::Chars {
                                length,
                                symbols: false,
                            });
                        }
                        if number.is_none() {
                            rows.push(Kind::Words {
                                count: config
                                    .words
                                    .clamp(MIN_PASSPHRASE_WORDS, MAX_PASSPHRASE_WORDS),
                            });
                        }
                        rows
                    }
                }
            }
        };
        Some(Self { rows })
    }
}

/// The OS random number generator, read in blocks. The block is wiped when
/// it is dropped, so no random bytes linger in memory.
pub struct SecureRandom {
    block: [u8; 256],
    used: usize,
}

impl SecureRandom {
    pub fn new() -> Self {
        Self {
            block: [0; 256],
            used: 256,
        }
    }

    fn next_u32(&mut self) -> Option<u32> {
        if self.used + 4 > self.block.len() {
            getrandom::fill(&mut self.block).ok()?;
            self.used = 0;
        }
        let bytes = &self.block[self.used..self.used + 4];
        let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        self.block[self.used..self.used + 4].zeroize();
        self.used += 4;
        Some(value)
    }

    /// A uniformly random number below `n` (`n > 0`). Numbers from the top
    /// of the range that would favour small results are thrown away.
    pub fn below(&mut self, n: usize) -> Option<usize> {
        let n = u64::try_from(n).ok().filter(|&n| n > 0 && n <= 1 << 32)?;
        let span = 1u64 << 32;
        let limit = span - span % n;
        loop {
            let value = u64::from(self.next_u32()?);
            if value < limit {
                return usize::try_from(value % n).ok();
            }
        }
    }
}

impl Default for SecureRandom {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SecureRandom {
    fn drop(&mut self) {
        self.block.zeroize();
    }
}

/// The character classes of a password.
fn classes(symbols: bool) -> Vec<&'static [u8]> {
    let mut classes = vec![LOWER, UPPER, DIGITS];
    if symbols {
        classes.push(SYMBOLS);
    }
    classes
}

/// A random password of `length` characters with at least one of each class
/// (when it is long enough to have them all).
pub fn password(rng: &mut SecureRandom, length: usize, symbols: bool) -> Option<String> {
    let classes = classes(symbols);
    let alphabet: Vec<u8> = classes.concat();
    let mut chars = vec![0u8; length];
    // A uniform draw has all classes with high probability (over 0.5 even at
    // four characters with symbols), so this loop ends after a few rounds; the
    // cap only guards against a broken generator.
    for _ in 0..1000 {
        for c in chars.iter_mut() {
            *c = alphabet[rng.below(alphabet.len())?];
        }
        let complete = length < classes.len()
            || classes
                .iter()
                .all(|class| chars.iter().any(|c| class.contains(c)));
        if complete {
            let text = String::from_utf8(chars.clone()).ok();
            chars.zeroize();
            return text;
        }
    }
    chars.zeroize();
    None
}

/// `count` random words from [`words`], joined by [`SEPARATOR`].
pub fn passphrase(rng: &mut SecureRandom, count: usize) -> Option<String> {
    let list = words();
    let mut out = String::new();
    for i in 0..count {
        if i > 0 {
            out.push(SEPARATOR);
        }
        out.push_str(list[rng.below(list.len())?]);
    }
    Some(out)
}

pub fn pin(rng: &mut SecureRandom, digits: usize) -> Option<String> {
    (0..digits)
        .map(|_| rng.below(10).map(|d| char::from(DIGITS[d])))
        .collect()
}

/// Bits of entropy of a secret of `kind`: log2 of the number of equally
/// likely values the generator can produce.
pub fn entropy_bits(kind: Kind) -> f64 {
    match kind {
        Kind::Words { count } => count as f64 * (words().len() as f64).log2(),
        Kind::Pin { digits } => digits as f64 * 10f64.log2(),
        Kind::Chars { length, symbols } => {
            let sizes: Vec<usize> = classes(symbols).iter().map(|c| c.len()).collect();
            let total: usize = sizes.iter().sum();
            let all = length as f64 * (total as f64).log2();
            if length < sizes.len() {
                return all;
            }
            // Passwords missing a class are drawn again, so the count is the
            // number with every class (inclusion-exclusion over the classes
            // left out), as a fraction of all `total^length` strings.
            let mut missing = 0.0;
            for subset in 1u32..(1 << sizes.len()) {
                let left_out: usize = sizes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| subset & (1 << i) != 0)
                    .map(|(_, size)| size)
                    .sum();
                let share = ((total - left_out) as f64 / total as f64).powi(length as i32);
                let sign = if subset.count_ones() % 2 == 1 {
                    1.0
                } else {
                    -1.0
                };
                missing += sign * share;
            }
            all + (1.0 - missing).log2()
        }
    }
}

fn describe(kind: Kind) -> String {
    match kind {
        Kind::Chars {
            length,
            symbols: true,
        } => format!("{length} characters with symbols"),
        Kind::Chars {
            length,
            symbols: false,
        } => format!("{length} letters and digits"),
        Kind::Words { count } => format!("{count} words"),
        Kind::Pin { digits } => format!("{digits} digits"),
    }
}

fn generate(rng: &mut SecureRandom, kind: Kind) -> Option<String> {
    match kind {
        Kind::Chars { length, symbols } => password(rng, length, symbols),
        Kind::Words { count } => passphrase(rng, count),
        Kind::Pin { digits } => pin(rng, digits),
    }
}

/// The password generator (`pass`).
pub struct PasswordPlugin {
    keyword: String,
    config: PasswordsConfig,
    platform: Arc<dyn PlatformProvider>,
}

impl PasswordPlugin {
    pub fn new(config: &PasswordsConfig, platform: Arc<dyn PlatformProvider>) -> Self {
        let keyword = match config.keyword.trim() {
            "" => KEYWORD.to_owned(),
            keyword => keyword.to_owned(),
        };
        Self {
            keyword,
            config: config.clone(),
            platform,
        }
    }

    fn hint(&self) -> ResultItem {
        ResultItem::new(
            ID,
            "hint",
            "Type pass, pass 24, pass 6 words, pass pin or pass 16 nosymbols",
            Action::Custom {
                payload: PAYLOAD_NOTHING.to_owned(),
            },
        )
        .with_subtitle("Random passwords and passphrases, made on this computer")
        .with_icon(IconSource::builtin("lock"))
        .with_score(score::KEYWORD)
    }
}

impl Plugin for PasswordPlugin {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "Password generator"
    }

    fn description(&self) -> &str {
        "Type `pass` for a random password, `pass 24` for a longer one or `pass 6 words` for a passphrase; Enter copies it, kept out of clipboard history."
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let Some(request) = Request::parse(input, &self.config) else {
            return vec![self.hint()];
        };
        let mut rng = SecureRandom::new();
        let mut items = Vec::with_capacity(request.rows.len());
        for (position, kind) in request.rows.into_iter().enumerate() {
            let Some(secret) = generate(&mut rng, kind) else {
                // No OS randomness: show nothing rather than something weak.
                return Vec::new();
            };
            let bits = entropy_bits(kind).floor();
            items.push(
                // The key is the position: ids reach the usage file and logs,
                // values must not.
                ResultItem::new(
                    ID,
                    (position + 1).to_string(),
                    secret.clone(),
                    Action::CopyText { text: secret },
                )
                .with_subtitle(format!(
                    "{bits} bits · {} · Enter copies, kept out of clipboard history",
                    describe(kind)
                ))
                .with_icon(IconSource::builtin("lock"))
                .with_score(score::KEYWORD - position as f64),
            );
        }
        items
    }

    /// Copies as a secret (see the module docs); a hint does nothing.
    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        match &item.action {
            Action::CopyText { text } => self
                .platform
                .set_clipboard_text_private(text)
                .map_err(PluginError::other),
            Action::Custom { .. } => Ok(()),
            _ => Err(PluginError::Unsupported(item.id.clone())),
        }
    }

    /// Nothing generated is recorded.
    fn tracks_usage(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::test_util::MockPlatform;

    fn config() -> PasswordsConfig {
        PasswordsConfig::default()
    }

    fn plugin() -> (Arc<MockPlatform>, PasswordPlugin) {
        let platform = MockPlatform::empty();
        let plugin = PasswordPlugin::new(&config(), platform.clone());
        (platform, plugin)
    }

    fn kinds(input: &str) -> Option<Vec<Kind>> {
        Request::parse(input, &config()).map(|r| r.rows)
    }

    #[test]
    fn the_word_list_is_clean() {
        let list = words();
        assert!(list.len() >= 2048, "{}", list.len());
        let unique: HashSet<_> = list.iter().collect();
        assert_eq!(unique.len(), list.len(), "repeated words");
        for word in list {
            assert!(
                (3..=8).contains(&word.len()) && word.chars().all(|c| c.is_ascii_lowercase()),
                "{word:?}"
            );
        }
    }

    #[test]
    fn requests_parse() {
        let chars = |length, symbols| Kind::Chars { length, symbols };
        assert_eq!(
            kinds(""),
            Some(vec![
                chars(20, true),
                chars(20, false),
                Kind::Words { count: 6 }
            ])
        );
        assert_eq!(kinds("24"), Some(vec![chars(24, true), chars(24, false)]));
        assert_eq!(kinds("6 words"), Some(vec![Kind::Words { count: 6 }; 3]));
        assert_eq!(kinds("WORDS 4"), Some(vec![Kind::Words { count: 4 }; 3]));
        assert_eq!(kinds("words"), Some(vec![Kind::Words { count: 6 }; 3]));
        assert_eq!(kinds("16 nosymbols"), Some(vec![chars(16, false); 3]));
        assert_eq!(kinds("16 no symbols"), Some(vec![chars(16, false); 3]));
        assert_eq!(kinds("symbols"), Some(vec![chars(20, true); 3]));
        assert_eq!(kinds("pin"), Some(vec![Kind::Pin { digits: 6 }; 3]));
        assert_eq!(kinds("pin 8"), Some(vec![Kind::Pin { digits: 8 }; 3]));
        // Clamped.
        let first = |input: &str| kinds(input).unwrap()[0];
        assert_eq!(first("1"), chars(MIN_PASSWORD_LENGTH, true));
        assert_eq!(
            first("99999999999999999999999"),
            chars(MAX_PASSWORD_LENGTH, true)
        );
        assert_eq!(first("100 words"), Kind::Words { count: 20 });
        assert_eq!(first("1 word"), Kind::Words { count: 2 });
        // Not understood.
        for bad in ["banana", "5 6", "-1", "no", "2.5"] {
            assert_eq!(kinds(bad), None, "{bad}");
        }
        // Without symbols by default, a bare `pass` offers one password.
        let plain = PasswordsConfig {
            symbols: false,
            ..config()
        };
        assert_eq!(
            Request::parse("", &plain).unwrap().rows,
            [chars(20, false), Kind::Words { count: 6 }]
        );
    }

    #[test]
    fn uniform_draws_stay_in_range_and_cover_it() {
        let mut rng = SecureRandom::new();
        let mut seen = [0usize; 7];
        for _ in 0..7_000 {
            seen[rng.below(7).unwrap()] += 1;
        }
        // Each value about 1000 times; a bias or an off-by-one shows up far outside this.
        assert!(seen.iter().all(|&n| (700..1300).contains(&n)), "{seen:?}");
        assert_eq!(rng.below(1), Some(0));
        assert_eq!(rng.below(0), None);
    }

    #[test]
    fn passwords_have_every_kind_of_character() {
        let mut rng = SecureRandom::new();
        for length in [4, 5, 20, 128] {
            for symbols in [true, false] {
                let pw = password(&mut rng, length, symbols).unwrap();
                assert_eq!(pw.len(), length);
                assert!(pw.bytes().any(|c| c.is_ascii_lowercase()), "{pw}");
                assert!(pw.bytes().any(|c| c.is_ascii_uppercase()), "{pw}");
                assert!(pw.bytes().any(|c| c.is_ascii_digit()), "{pw}");
                assert_eq!(pw.bytes().any(|c| SYMBOLS.contains(&c)), symbols, "{pw}");
                assert!(pw.bytes().all(|c| LOWER.contains(&c)
                    || UPPER.contains(&c)
                    || DIGITS.contains(&c)
                    || SYMBOLS.contains(&c)));
            }
        }
        let a = password(&mut rng, 20, true).unwrap();
        let b = password(&mut rng, 20, true).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn passphrases_and_pins() {
        let mut rng = SecureRandom::new();
        let phrase = passphrase(&mut rng, 6).unwrap();
        let parts: Vec<&str> = phrase.split(SEPARATOR).collect();
        assert_eq!(parts.len(), 6);
        assert!(parts.iter().all(|word| words().contains(word)), "{phrase}");
        let code = pin(&mut rng, 8).unwrap();
        assert_eq!(code.len(), 8);
        assert!(code.bytes().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn entropy_is_counted_exactly() {
        let bits = entropy_bits;
        // No class requirement below four characters' worth of classes...
        assert!((bits(Kind::Pin { digits: 6 }) - 6.0 * 10f64.log2()).abs() < 1e-9);
        let words = bits(Kind::Words { count: 6 });
        assert!((words - 6.0 * (super::words().len() as f64).log2()).abs() < 1e-9);
        assert!(words > 66.0, "{words}");
        // ...and with every class required, a little under length * log2(78).
        let full = 20.0 * 78f64.log2();
        let with = bits(Kind::Chars {
            length: 20,
            symbols: true,
        });
        assert!(with < full && with > full - 1.0, "{with} vs {full}");
        // Exact for a small case: length 4 over {26, 26, 10, 16}, every class
        // once: 4! * 26 * 26 * 10 * 16 strings.
        let exact = (24.0f64 * 26.0 * 26.0 * 10.0 * 16.0).log2();
        let four = bits(Kind::Chars {
            length: 4,
            symbols: true,
        });
        assert!((four - exact).abs() < 1e-6, "{four} vs {exact}");
        let alnum = bits(Kind::Chars {
            length: 20,
            symbols: false,
        });
        assert!(alnum < 20.0 * 62f64.log2() && alnum > 118.0, "{alnum}");
    }

    #[test]
    fn rows_show_the_secret_and_its_strength_but_key_by_position() {
        let (_, plugin) = plugin();
        let items = plugin.query("");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].title.len(), 20);
        let bits = entropy_bits(Kind::Chars {
            length: 20,
            symbols: true,
        });
        assert_eq!(bits.floor(), 125.0);
        assert!(
            items[0]
                .subtitle
                .starts_with("125 bits · 20 characters with symbols"),
            "{}",
            items[0].subtitle
        );
        assert!(
            items[1].subtitle.contains("20 letters and digits"),
            "{}",
            items[1].subtitle
        );
        assert_eq!(items[2].title.split(SEPARATOR).count(), 6);
        let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["pwgen:1", "pwgen:2", "pwgen:3"]);
        for item in &items {
            assert!(!item.id.contains(&item.title));
            assert_eq!(
                item.action,
                Action::CopyText {
                    text: item.title.clone()
                }
            );
        }
        assert!(items.windows(2).all(|pair| pair[0].score > pair[1].score));
        // A fresh value every time.
        assert_ne!(plugin.query("")[0].title, plugin.query("")[0].title);
        // Unknown input explains itself.
        let hint = plugin.query("banana");
        assert_eq!(hint.len(), 1);
        assert!(matches!(hint[0].action, Action::Custom { .. }));
    }

    #[test]
    fn enter_copies_as_a_secret_and_nothing_is_tracked() {
        let (platform, plugin) = plugin();
        assert!(!plugin.tracks_usage());
        let item = plugin.query("12").remove(0);
        plugin.execute(&item).unwrap();
        assert_eq!(
            *platform.private_clipboard.lock().unwrap(),
            std::slice::from_ref(&item.title)
        );
        // Never through the ordinary clipboard path.
        assert!(platform.clipboard.lock().unwrap().is_empty());
        // The hint does nothing.
        plugin.execute(&plugin.query("banana").remove(0)).unwrap();
        assert_eq!(platform.private_clipboard.lock().unwrap().len(), 1);
    }

    #[test]
    fn metadata() {
        let (_, plugin) = plugin();
        assert_eq!(plugin.id(), "pwgen");
        assert_eq!(plugin.keyword(), Some("pass"));
        assert!(!plugin.global());
        let custom = PasswordPlugin::new(
            &PasswordsConfig {
                keyword: " pw ".into(),
                ..config()
            },
            MockPlatform::empty(),
        );
        assert_eq!(custom.keyword(), Some("pw"));
    }
}
