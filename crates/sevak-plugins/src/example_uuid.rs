//! Example plugin: a UUID generator, written as a tutorial.
//!
//! Read this file top to bottom if you want to write a built-in plugin. The
//! whole walkthrough, including how to register the plugin and how to
//! disable it, lives in `docs/plugins.md`.
//!
//! # What it does
//!
//! | You type     | You get                                              |
//! |--------------|------------------------------------------------------|
//! | `uuid `      | one fresh random (version 4) UUID, lowercase         |
//! | `uuid 5`     | five different UUIDs (capped at [`MAX_COUNT`])       |
//! | `uuid upper` | one UUID in uppercase (`upper` and a count combine)  |
//!
//! The keyword only routes once it is followed by a space (`uuid `), like every
//! keyword in Sevak; that is how the engine tells "I am typing a keyword
//! query" from "I am typing an app name that starts with these letters".
//!
//! Pressing Enter copies the highlighted UUID to the clipboard.
//!
//! # The anatomy of a plugin
//!
//! A plugin is any type implementing [`sevak_core::Plugin`]. It owns whatever
//! state it needs (here: just a handle to the platform provider, which is how
//! it touches the OS) and answers three questions for the search engine:
//!
//! 1. **Who are you?**  [`Plugin::id`], [`Plugin::name`],
//!    [`Plugin::description`], [`Plugin::keyword`], [`Plugin::global`].
//! 2. **What matches this input?**  [`Plugin::query`].
//! 3. **What happens on Enter?**  [`Plugin::execute`].
//!
//! Optionally, [`Plugin::refresh`] rebuilds an index. This plugin has none, so
//! it keeps the default no-op.
//!
//! # Why `getrandom` and not the `uuid` crate
//!
//! A v4 UUID is 122 random bits plus six fixed version/variant bits; formatting
//! it is a dozen lines. The `getrandom` crate (a thin wrapper over the OS random
//! number generator, already a transitive dependency of the workspace) is all
//! that is needed. The `uuid` crate would add features, parsing and
//! other versions that a launcher does not use.

use std::fmt::Write as _;
use std::sync::Arc;

use sevak_core::model::score;
use sevak_core::{Action, IconSource, Plugin, PluginResult, ResultItem};
use sevak_platform::PlatformProvider;

use crate::actions::execute_action;

/// Most UUIDs `uuid <n>` will produce. A launcher shows a handful of rows, so
/// anything beyond this would only be scrolled past; the cap also keeps one
/// keystroke's work bounded no matter what the user types (`uuid 99999999`).
pub const MAX_COUNT: usize = 20;

/// The keyword that routes a query to this plugin.
const KEYWORD: &str = "uuid";

/// Generates random (version 4) UUIDs on request.
///
/// The struct holds only what [`Plugin::execute`] needs: a handle to the
/// platform provider. Plugins are shared across threads (`Plugin: Send + Sync`),
/// so state that changes after construction needs interior mutability (see
/// `AppsPlugin`, which swaps its index behind an `RwLock`). This one is
/// stateless.
pub struct UuidPlugin {
    platform: Arc<dyn PlatformProvider>,
}

impl UuidPlugin {
    /// Plugins take their dependencies in the constructor. The registry
    /// descriptor (see `registry.rs`) is the only place that builds them, so
    /// the constructor's shape is up to you.
    pub fn new(platform: Arc<dyn PlatformProvider>) -> Self {
        Self { platform }
    }
}

impl Plugin for UuidPlugin {
    /// The plugin's identity.
    ///
    /// Contract: **unique and stable forever.** The id is baked into every
    /// result id (`uuid:1`), into usage statistics (what the user picked
    /// before, which feeds ranking) and into the `[plugins] disabled` list in
    /// `config.toml`. Renaming it orphans the user's history and silently
    /// re-enables a plugin they turned off. Pick a short lowercase id; plugins
    /// that come in several instances use `family:instance` (`web:g`).
    fn id(&self) -> &str {
        "uuid"
    }

    /// Display name for the settings UI. Free to change at any time.
    fn name(&self) -> &str {
        "UUID generator"
    }

    /// One-line summary, shown beside the enable/disable toggle in settings.
    fn description(&self) -> &str {
        "Type `uuid` to generate random UUIDs; Enter copies one."
    }

    /// The trigger word.
    ///
    /// Contract (keyword routing): when the user types `uuid` followed by
    /// whitespace (`"uuid 5"`, or just `"uuid "`), the engine queries *only*
    /// plugins with this keyword and hands them the rest of the input
    /// (`"5"`), trimmed at the start. All other plugins are silenced, so
    /// keyword results never compete with app launches. Matching is
    /// case-sensitive, and a bare `"uuid"` with no trailing space is an
    /// ordinary global query that this plugin does not see (see [`global`]).
    ///
    /// Return `None` for plugins that should only ever answer global queries
    /// (like the calculator).
    ///
    /// [`global`]: Plugin::global
    fn keyword(&self) -> Option<&str> {
        Some(KEYWORD)
    }

    /// Whether this plugin is also asked about input *without* its keyword.
    ///
    /// The default is `keyword().is_none()`: keyword plugins are keyword-only.
    /// That is what we want here. Typing `ap` for Firefox should never pop up
    /// a random UUID. A plugin may return `true` to show a (down-weighted by
    /// the engine) secondary result for plain queries; `FilesPlugin` does.
    /// We spell the default out because it is a decision worth seeing.
    fn global(&self) -> bool {
        false
    }

    /// Turns the text after the keyword into result rows.
    ///
    /// Contract:
    /// - **Fast and non-blocking.** The engine calls this on a worker thread
    ///   for *every keystroke* and logs a warning when a whole query exceeds
    ///   16 ms. No file or network I/O, no waiting on locks that a slow
    ///   operation holds. If you need data from a slow source, load it in
    ///   [`Plugin::refresh`] into memory and read it here. (Reading the OS
    ///   random number generator costs microseconds, which is fine.)
    /// - **Never panic.** The release build aborts the process on panic, so a
    ///   buggy plugin takes Sevak down. Return an empty `Vec` on anything
    ///   unexpected, as this function does when the OS cannot supply random
    ///   bytes.
    /// - **Ids are stable keys, not content.** [`ResultItem::new`] builds the
    ///   id as `<plugin id>:<key>`. Usage statistics are keyed by it, and the
    ///   engine deduplicates by it, so keys must be unique within one answer
    ///   but should not embed volatile data. A UUID changes on every
    ///   keystroke, so using it as the key would fill the usage history
    ///   with ids that never occur again. We use the row position (`1`, `2`,
    ///   ...) instead; the copied text is carried by the [`Action`], not the
    ///   id.
    /// - **Scores decide order.** Fuzzy plugins use the matcher's score; plugins
    ///   that know exactly what they want to show use the constants in
    ///   [`score`]: `EXACT_ANSWER` (10 000, "this *is* the answer", used by the
    ///   calculator), `KEYWORD` (5 000, "you asked for me explicitly") and
    ///   `FALLBACK` (0). The engine does not apply usage boosts to anything
    ///   scoring `KEYWORD` or more, which is the behaviour we want for
    ///   generated values. Rows with equal scores are ordered by title.
    fn query(&self, input: &str) -> Vec<ResultItem> {
        // `input` is already the text after the keyword, but may contain
        // stray whitespace, so parse defensively.
        let Some(request) = Request::parse(input) else {
            // Unknown argument (`uuid banana`): show nothing rather than guess.
            return Vec::new();
        };

        let mut items = Vec::with_capacity(request.count);
        for position in 1..=request.count {
            let Some(mut uuid) = random_uuid_v4() else {
                return Vec::new();
            };
            if request.upper {
                uuid.make_ascii_uppercase();
            }
            items.push(
                ResultItem::new(
                    self.id(),
                    position.to_string(),
                    &uuid,
                    // The action carries everything `execute` needs, so the
                    // plugin does not have to remember what it showed. This
                    // matters: the engine may call `execute` long after
                    // `query`, and it never promises which thread it is on.
                    Action::CopyText { text: uuid.clone() },
                )
                .with_subtitle("Enter to copy")
                // Icons are a closed set of glyphs bundled with the UI; see
                // `IconSource::Builtin`. Use `File` or `Shell` for real images.
                .with_icon(IconSource::builtin("copy"))
                .with_score(score::KEYWORD),
            );
        }
        items
    }

    /// Carries out the result the user picked.
    ///
    /// Contract: `item` is one of the rows this plugin returned from
    /// [`Plugin::query`], with its [`Action`] intact. Standard actions (launch,
    /// open a path or URL, copy text) are delegated to
    /// [`execute_action`], which calls the platform provider. That keeps
    /// OS-specific behaviour and URL scheme filtering in one place. Do not
    /// shell out or touch the clipboard yourself.
    ///
    /// Use [`Action::Custom`] only when the standard actions cannot express
    /// what you need; its payload comes back here and nowhere else. Return an
    /// error (never panic) if the action fails; the shell logs it.
    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        execute_action(self.platform.as_ref(), &item.action)
    }

    // `refresh` is deliberately not overridden.
    //
    // Contract, for plugins that need it: `refresh` is called on a
    // background thread at startup, on request ("Reload index" in the tray)
    // and periodically. It may be slow: scan directories, read a database,
    // call an API. It rebuilds the plugin's in-memory index, which `query`
    // reads. Build the new index off to the side and swap it in under a
    // short write lock so queries are never blocked (see `FilesPlugin`).
}

/// What the user asked for after the keyword.
#[derive(Debug, PartialEq, Eq)]
struct Request {
    count: usize,
    upper: bool,
}

impl Request {
    /// Parses `""`, `"5"`, `"upper"`, `"5 upper"` and `"upper 5"`. Anything
    /// else is `None`. Counts are clamped to `1..=`[`MAX_COUNT`], so `uuid 0`
    /// still gives one and `uuid 500` gives twenty.
    fn parse(input: &str) -> Option<Self> {
        let mut request = Request {
            count: 1,
            upper: false,
        };
        for word in input.split_whitespace() {
            if word.eq_ignore_ascii_case("upper") {
                request.upper = true;
            } else if word.chars().all(|c| c.is_ascii_digit()) {
                // Digits only, so the sole parse failure is overflow; treat
                // that as "a lot" and let the clamp below handle it.
                request.count = word.parse::<usize>().unwrap_or(usize::MAX);
            } else {
                return None;
            }
        }
        request.count = request.count.clamp(1, MAX_COUNT);
        Some(request)
    }
}

/// A random UUID (version 4, RFC 9562) in the canonical lowercase hyphenated
/// form, or `None` if the OS random number generator is unavailable.
fn random_uuid_v4() -> Option<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(format_v4(bytes))
}

/// Stamps the version and variant bits onto 16 random bytes and formats them
/// as `xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`, where `y` is one of `8 9 a b`.
fn format_v4(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    let mut out = String::with_capacity(36);
    for (i, byte) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        // Writing to a `String` cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;
    use std::collections::HashSet;

    fn is_v4(uuid: &str) -> bool {
        let b = uuid.as_bytes();
        uuid.len() == 36
            && [8, 13, 18, 23].iter().all(|&i| b[i] == b'-')
            && b[14] == b'4'
            && matches!(b[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B')
            && uuid
                .chars()
                .enumerate()
                .all(|(i, c)| [8, 13, 18, 23].contains(&i) || c.is_ascii_hexdigit())
    }

    #[test]
    fn formats_known_bytes() {
        assert_eq!(format_v4([0; 16]), "00000000-0000-4000-8000-000000000000");
        assert_eq!(
            format_v4([0xff; 16]),
            "ffffffff-ffff-4fff-bfff-ffffffffffff"
        );
    }

    #[test]
    fn metadata() {
        let plugin = UuidPlugin::new(MockPlatform::empty());
        assert_eq!(plugin.id(), "uuid");
        assert_eq!(plugin.name(), "UUID generator");
        assert!(!plugin.description().is_empty());
        assert_eq!(plugin.keyword(), Some("uuid"));
        assert!(!plugin.global());
    }

    #[test]
    fn empty_input_gives_one_lowercase_uuid() {
        let plugin = UuidPlugin::new(MockPlatform::empty());
        for input in ["", "   "] {
            let items = plugin.query(input);
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert!(is_v4(&item.title), "{}", item.title);
            assert_eq!(item.title, item.title.to_lowercase());
            assert_eq!(item.subtitle, "Enter to copy");
            assert_eq!(item.icon, Some(IconSource::builtin("copy")));
            assert_eq!(item.score, score::KEYWORD);
            assert_eq!(item.plugin_id, "uuid");
            assert_eq!(item.id, "uuid:1");
            assert_eq!(
                item.action,
                Action::CopyText {
                    text: item.title.clone()
                }
            );
        }
    }

    #[test]
    fn count_gives_distinct_uuids_with_distinct_ids() {
        let plugin = UuidPlugin::new(MockPlatform::empty());
        let items = plugin.query("5");
        assert_eq!(items.len(), 5);
        let titles: HashSet<_> = items.iter().map(|i| i.title.clone()).collect();
        let ids: HashSet<_> = items.iter().map(|i| i.id.clone()).collect();
        assert_eq!(titles.len(), 5);
        assert_eq!(ids.len(), 5);
        assert!(items.iter().all(|i| is_v4(&i.title)));
    }

    #[test]
    fn successive_queries_differ() {
        let plugin = UuidPlugin::new(MockPlatform::empty());
        assert_ne!(plugin.query("")[0].title, plugin.query("")[0].title);
    }

    #[test]
    fn count_is_clamped() {
        let plugin = UuidPlugin::new(MockPlatform::empty());
        assert_eq!(plugin.query("0").len(), 1);
        assert_eq!(plugin.query("20").len(), 20);
        assert_eq!(plugin.query("21").len(), MAX_COUNT);
        assert_eq!(plugin.query("99999999999999999999999").len(), MAX_COUNT);
    }

    #[test]
    fn upper_variant() {
        let plugin = UuidPlugin::new(MockPlatform::empty());
        for input in ["upper", "UPPER", "3 upper", "upper 3"] {
            let items = plugin.query(input);
            assert!(!items.is_empty());
            for item in &items {
                assert!(is_v4(&item.title), "{}", item.title);
                assert_eq!(item.title, item.title.to_uppercase());
                assert_eq!(
                    item.action,
                    Action::CopyText {
                        text: item.title.clone()
                    }
                );
            }
        }
        assert_eq!(plugin.query("upper 3").len(), 3);
    }

    #[test]
    fn unknown_arguments_give_nothing() {
        let plugin = UuidPlugin::new(MockPlatform::empty());
        for input in ["banana", "5 banana", "-1", "2.5"] {
            assert!(plugin.query(input).is_empty(), "{input}");
        }
    }

    #[test]
    fn execute_copies_to_the_clipboard() {
        let platform = MockPlatform::empty();
        let plugin = UuidPlugin::new(platform.clone());
        let item = plugin.query("").remove(0);
        plugin.execute(&item).unwrap();
        assert_eq!(*platform.clipboard.lock().unwrap(), vec![item.title]);
    }
}
