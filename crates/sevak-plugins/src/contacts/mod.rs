//! Contacts: `c <name>` or `@name` finds people by name, e-mail address, phone
//! number or company.
//!
//! Opt-in (`[contacts] enabled = true`). Enter copies the e-mail address; the
//! action panel (`→` / `Ctrl+K`) offers the rest:
//!
//! | Key | Action |
//! |---|---|
//! | Enter | copy the e-mail address (the phone number if there is none) |
//! | `Ctrl+Enter` | write an e-mail (`mailto:`) |
//! | `Shift+Enter` | copy the phone number |
//! | `Alt+Enter` | call (`tel:`, handled by whatever phone app is installed) |
//! | panel | open the contact card (macOS), copy other addresses and numbers |
//! | `Ctrl+L` | the phone number in Large Type |
//!
//! # Where the data comes from
//!
//! vCard files and folders from `[contacts] vcard_files` on every OS, plus the
//! system address book when `[contacts] use_system` is on: macOS Contacts
//! (needs the user's permission, asked only when they press Enter on the
//! "Allow access" row), the Windows People store, and Evolution's local
//! address books on Linux. See [`sources`].
//!
//! # Privacy
//!
//! Contacts are read on a background thread by [`Plugin::refresh`] and kept in
//! memory only: no cache file, no log of names, numbers or addresses (the log
//! has counts), and picking a contact is kept out of the usage statistics and
//! search history (`Plugin::tracks_usage`). Nothing is sent anywhere. Two instances serve the two keywords
//! (`contacts` for `c`, `contacts:at` for `@`) and share one copy of the data.

pub mod sources;

use std::sync::{Arc, Mutex, RwLock};

use sevak_core::config::ContactsConfig;
use sevak_core::model::score;
use sevak_core::plugin::ResultsNotifier;
use sevak_core::{
    Action, FuzzyQuery, IconSource, Modifier, Plugin, PluginError, PluginResult, ResultItem,
};
use sevak_platform::{Contact, ContactsAccess, DeepLink, PlatformProvider};

use crate::actions::execute_action;
use crate::files::{expand_home, home_dir};

/// The default keyword; `[contacts] keyword` changes it.
pub const KEYWORD: &str = "c";
/// The symbol keyword that always works as well: `@ada` needs no space.
pub const AT_KEYWORD: &str = "@";

const FAMILY_ID: &str = "contacts";
const AT_ID: &str = "contacts:at";

/// Rows returned for one query.
const MAX_ROWS: usize = 30;
/// Contacts kept; more than this is not an address book, it is a mistake.
const MAX_CONTACTS: usize = 100_000;
/// Extra addresses/numbers offered in the action panel.
const MAX_EXTRA: usize = 3;
/// A name or company that starts with the input beats a looser match.
const PREFIX_BONUS: f64 = 60.0;
const WORD_BONUS: f64 = 30.0;
/// Weight of a company match against a name match.
const COMPANY_WEIGHT: f64 = 0.6;
/// Per typed character for an e-mail match, on nucleo's scale.
const EMAIL_CHAR_SCORE: f64 = 16.0;
/// Base score of a phone number match; digits are a deliberate query.
const PHONE_MATCH_SCORE: f64 = 400.0;

const ENABLE_SNIPPET: &str = "[contacts]\nenabled = true";
const PAYLOAD_GRANT: &str = "grant";
const PAYLOAD_NOTHING: &str = "nothing";
const PAYLOAD_CALL: &str = "call:";
const PAYLOAD_CARD: &str = "card:";

/// A contact with what matching needs, computed once at load time.
struct Entry {
    key: String,
    contact: Contact,
    emails_lower: Vec<String>,
    /// Digits only (a leading `+` dropped), one per phone number.
    phone_digits: Vec<String>,
}

/// What the last load found.
#[derive(Default)]
struct Snapshot {
    entries: Vec<Entry>,
    /// The system address book needs the user's permission first.
    needs_permission: bool,
    /// Why the system address book cannot be read, if it cannot.
    problem: Option<String>,
    /// Set once a load finished, so "loading" is not shown forever.
    loaded: bool,
}

struct Shared {
    config: ContactsConfig,
    platform: Arc<dyn PlatformProvider>,
    snapshot: RwLock<Arc<Snapshot>>,
    notifier: Mutex<Option<ResultsNotifier>>,
}

impl Shared {
    fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot
            .read()
            .map(|snapshot| Arc::clone(&snapshot))
            .unwrap_or_default()
    }

    /// Reads every source and swaps the result in. Slow; background thread.
    fn load(&self) {
        let home = home_dir();
        let mut contacts: Vec<Contact> = Vec::new();
        let mut files = 0;
        for path in &self.config.vcard_files {
            let path = expand_home(path.trim(), home.as_deref());
            contacts.extend(sources::read_vcard_path(&path));
            files += 1;
        }

        let mut needs_permission = false;
        let mut problem = None;
        if self.config.use_system {
            match self.platform.contacts_access() {
                ContactsAccess::Granted => match self.platform.system_contacts() {
                    Ok(list) => contacts.extend(list),
                    Err(err) => {
                        tracing::warn!(%err, "could not read the system address book");
                        problem = Some(format!("The system address book could not be read: {err}"));
                    }
                },
                ContactsAccess::NotDetermined => needs_permission = true,
                ContactsAccess::Denied(how) => problem = Some(how),
                ContactsAccess::Unsupported => {}
            }
            for db in self.platform.evolution_address_books() {
                match sources::read_evolution(&db) {
                    Ok(list) => contacts.extend(list),
                    Err(err) => tracing::debug!(%err, "could not read an Evolution address book"),
                }
            }
        }

        let entries = index(contacts);
        tracing::info!(
            contacts = entries.len(),
            vcard_sources = files,
            "loaded contacts"
        );
        if let Ok(mut slot) = self.snapshot.write() {
            *slot = Arc::new(Snapshot {
                entries,
                needs_permission,
                problem,
                loaded: true,
            });
        }
    }

    /// Asks the OS for permission, reloads, and tells the launcher.
    fn grant(self: &Arc<Self>) {
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("sevak-contacts".into())
            .spawn(move || {
                if let Err(err) = shared.platform.request_contacts_access() {
                    tracing::warn!(%err, "asking for access to contacts failed");
                }
                shared.load();
                let notifier = shared.notifier.lock().ok().and_then(|n| n.clone());
                if let Some(notify) = notifier {
                    notify(FAMILY_ID);
                }
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "could not start the contacts thread");
        }
    }
}

/// Sorts, drops duplicates (the same person exported twice, or present in two
/// sources) and gives every contact a unique key.
fn index(mut contacts: Vec<Contact>) -> Vec<Entry> {
    contacts.truncate(MAX_CONTACTS);
    contacts.sort_by_cached_key(|c| (c.name.to_lowercase(), c.id.clone()));
    contacts.dedup_by(|a, b| {
        a.name.eq_ignore_ascii_case(&b.name)
            && a.emails.first() == b.emails.first()
            && a.phones.first() == b.phones.first()
    });

    let mut used = std::collections::HashSet::new();
    contacts
        .into_iter()
        .map(|contact| {
            let mut key = contact.id.clone();
            let mut n = 1;
            while !used.insert(key.clone()) {
                n += 1;
                key = format!("{}#{n}", contact.id);
            }
            Entry {
                key,
                emails_lower: contact.emails.iter().map(|e| e.to_lowercase()).collect(),
                phone_digits: contact.phones.iter().map(|p| digits_of(p)).collect(),
                contact,
            }
        })
        .collect()
}

fn digits_of(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// The digits of `input` if it looks like (part of) a phone number: only digits
/// and `+ - . ( )` and spaces, and at least three digits.
fn phone_query(input: &str) -> Option<String> {
    let phone_like = input
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | '(' | ')' | ' '));
    let digits = digits_of(input);
    (phone_like && digits.len() >= 3).then_some(digits)
}

/// `mailto:` for an address, percent-encoding anything outside a safe set.
pub fn mailto_url(address: &str) -> Option<String> {
    let address = address.trim();
    if !address.contains('@') || address.chars().any(char::is_control) {
        return None;
    }
    let mut url = String::from("mailto:");
    for byte in address.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'.' | b'_' | b'-' | b'+' | b'~') {
            url.push(byte as char);
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    Some(url)
}

/// One of the two instances of the contacts plugin.
pub struct ContactsPlugin {
    id: &'static str,
    keyword: String,
    /// `None` while `[contacts] enabled` is off: nothing is loaded.
    shared: Option<Arc<Shared>>,
    platform: Arc<dyn PlatformProvider>,
}

impl ContactsPlugin {
    /// The plugin instances for `config`: the configured keyword (`c`) and the
    /// `@` alias, sharing one copy of the contacts.
    pub fn instances(
        config: &ContactsConfig,
        platform: Arc<dyn PlatformProvider>,
    ) -> Vec<Arc<dyn Plugin>> {
        let shared = config.enabled.then(|| {
            Arc::new(Shared {
                config: config.clone(),
                platform: platform.clone(),
                snapshot: RwLock::new(Arc::new(Snapshot::default())),
                notifier: Mutex::new(None),
            })
        });
        let primary = Self {
            id: FAMILY_ID,
            keyword: config.keyword.clone(),
            shared: shared.clone(),
            platform: platform.clone(),
        };
        let mut plugins: Vec<Arc<dyn Plugin>> = vec![Arc::new(primary)];
        if config.keyword != AT_KEYWORD {
            plugins.push(Arc::new(Self {
                id: AT_ID,
                keyword: AT_KEYWORD.to_owned(),
                shared,
                platform,
            }));
        }
        plugins
    }

    fn status_row(&self, key: &str, title: &str, subtitle: &str, action: Action) -> ResultItem {
        ResultItem::new(self.id, key, title, action)
            .with_subtitle(subtitle)
            .with_icon(IconSource::builtin("plugin"))
            .with_score(score::KEYWORD)
    }

    fn rows(&self, input: &str) -> Vec<ResultItem> {
        let Some(shared) = &self.shared else {
            return vec![self.status_row(
                "off",
                "Contacts are off",
                "Enter copies the setting to add to config.toml; then choose Reload index",
                Action::CopyText {
                    text: ENABLE_SNIPPET.to_owned(),
                },
            )];
        };
        let snapshot = shared.snapshot();

        let mut rows = self.matches(&snapshot, input.trim());
        let status = if snapshot.needs_permission {
            Some(self.status_row(
                "grant",
                "Allow Sevak to read your Contacts",
                "Enter shows the macOS question; vCard files work without it",
                Action::Custom {
                    payload: PAYLOAD_GRANT.to_owned(),
                },
            ))
        } else if let Some(problem) = &snapshot.problem {
            Some(self.status_row(
                "problem",
                "Cannot read the system address book",
                problem,
                nothing(),
            ))
        } else if !snapshot.loaded {
            Some(self.status_row(
                "loading",
                "Loading contacts…",
                "Try again in a moment",
                nothing(),
            ))
        } else if snapshot.entries.is_empty() {
            Some(self.status_row(
                "empty",
                "No contacts found",
                "Add vCard files with [contacts] vcard_files, or allow the system address book",
                nothing(),
            ))
        } else {
            None
        };
        if let Some(mut status) = status {
            if !rows.is_empty() {
                // Behind the people that were found.
                status.score = 0.0;
            }
            rows.push(status);
        }
        rows
    }

    fn matches(&self, snapshot: &Snapshot, input: &str) -> Vec<ResultItem> {
        if input.is_empty() {
            // The address book in alphabetical order.
            return snapshot
                .entries
                .iter()
                .take(MAX_ROWS)
                .enumerate()
                .map(|(i, entry)| self.row(entry).with_score(score::KEYWORD - i as f64))
                .collect();
        }

        let input_lower = input.to_lowercase();
        let digits = phone_query(input);
        let mut query = FuzzyQuery::new(input);
        let mut scored: Vec<(f64, &Entry)> = snapshot
            .entries
            .iter()
            .filter_map(|entry| {
                score_entry(entry, &mut query, &input_lower, digits.as_deref())
                    .map(|score| (score, entry))
            })
            .collect();
        scored.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then_with(|| a.1.contact.name.cmp(&b.1.contact.name))
        });
        scored
            .into_iter()
            .take(MAX_ROWS)
            .map(|(score, entry)| self.row(entry).with_score(score))
            .collect()
    }

    fn row(&self, entry: &Entry) -> ResultItem {
        let contact = &entry.contact;
        let (primary, hint) = match (contact.emails.first(), contact.phones.first()) {
            (Some(email), _) => (email.clone(), "Enter to copy the email"),
            (None, Some(phone)) => (phone.clone(), "Enter to copy the number"),
            (None, None) => (contact.name.clone(), "Enter to copy the name"),
        };
        let mut details: Vec<&str> = Vec::new();
        details.extend(contact.emails.first().map(String::as_str));
        details.extend(contact.phones.first().map(String::as_str));
        details.extend(contact.company.as_deref());
        let subtitle = if details.is_empty() {
            hint.to_owned()
        } else {
            format!("{} · {hint}", details.join(" · "))
        };

        let mut item = ResultItem::new(
            self.id,
            &entry.key,
            &contact.name,
            Action::CopyText { text: primary },
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin("plugin"));

        if let Some(url) = contact.emails.first().and_then(|e| mailto_url(e)) {
            item = item.with_secondary(
                "Write an email",
                Some(Modifier::Ctrl),
                Action::OpenUrl { url },
            );
        }
        if let Some(phone) = contact.phones.first() {
            if !contact.emails.is_empty() {
                item = item.with_secondary(
                    "Copy phone number",
                    Some(Modifier::Shift),
                    Action::CopyText {
                        text: phone.clone(),
                    },
                );
            }
            if DeepLink::tel(phone).is_some() {
                item = item.with_secondary(
                    "Call",
                    Some(Modifier::Alt),
                    Action::Custom {
                        payload: format!("{PAYLOAD_CALL}{phone}"),
                    },
                );
            }
        }
        if let Some(card) = &contact.card_id {
            item = item.with_secondary(
                "Open contact card",
                None,
                Action::Custom {
                    payload: format!("{PAYLOAD_CARD}{card}"),
                },
            );
        }
        for email in contact.emails.iter().skip(1).take(MAX_EXTRA) {
            item = item.with_secondary(
                format!("Copy {email}"),
                None,
                Action::CopyText {
                    text: email.clone(),
                },
            );
        }
        // The first number is already the row's action or "Copy phone number".
        for phone in contact.phones.iter().skip(1).take(MAX_EXTRA) {
            item = item.with_secondary(
                format!("Copy {phone}"),
                None,
                Action::CopyText {
                    text: phone.clone(),
                },
            );
        }

        // Ctrl+L: the number to read out or dial, else the address.
        if let Some(large) = contact.phones.first().or(contact.emails.first()) {
            item = item.with_large_text(large.clone());
        }
        item
    }
}

fn nothing() -> Action {
    Action::Custom {
        payload: PAYLOAD_NOTHING.to_owned(),
    }
}

/// How well `entry` matches the typed text, or `None`.
fn score_entry(
    entry: &Entry,
    query: &mut FuzzyQuery,
    input_lower: &str,
    digits: Option<&str>,
) -> Option<f64> {
    let contact = &entry.contact;
    let mut best: Option<f64> = None;
    let mut consider = |score: f64| best = Some(best.map_or(score, |b| b.max(score)));

    if let Some(matched) = query.score(&contact.name) {
        let lower = contact.name.to_lowercase();
        let bonus = if lower.starts_with(input_lower) {
            PREFIX_BONUS
        } else if lower.split_whitespace().any(|w| w.starts_with(input_lower)) {
            WORD_BONUS
        } else {
            0.0
        };
        consider(f64::from(matched) + bonus);
    }
    let typed = input_lower.chars().count() as f64;
    for email in &entry.emails_lower {
        if email.contains(input_lower) {
            let bonus = if email.starts_with(input_lower) {
                PREFIX_BONUS
            } else {
                0.0
            };
            consider(EMAIL_CHAR_SCORE * typed + bonus);
        }
    }
    if let Some(company) = &contact.company {
        if let Some(matched) = query.score(company) {
            consider(f64::from(matched) * COMPANY_WEIGHT);
        }
    }
    if let Some(digits) = digits {
        if entry.phone_digits.iter().any(|p| p.contains(digits)) {
            consider(PHONE_MATCH_SCORE + digits.len() as f64);
        }
    }
    best
}

impl Plugin for ContactsPlugin {
    fn id(&self) -> &str {
        self.id
    }

    fn name(&self) -> &str {
        if self.id == AT_ID {
            "Contacts (@)"
        } else {
            "Contacts"
        }
    }

    fn description(&self) -> &str {
        "Type `c` or `@` to find people by name, email, phone or company. Off until [contacts] enabled = true."
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.keyword)
    }

    fn global(&self) -> bool {
        false
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
        if payload == PAYLOAD_GRANT {
            if let Some(shared) = &self.shared {
                shared.grant();
            }
            return Ok(());
        }
        if let Some(number) = payload.strip_prefix(PAYLOAD_CALL) {
            let link = DeepLink::tel(number).ok_or_else(|| {
                PluginError::Message("that is not a number Sevak can dial".into())
            })?;
            return self.platform.open_link(&link).map_err(PluginError::other);
        }
        if let Some(card) = payload.strip_prefix(PAYLOAD_CARD) {
            let link = DeepLink::address_book_card(card)
                .ok_or_else(|| PluginError::Message("that contact card cannot be opened".into()))?;
            return self.platform.open_link(&link).map_err(PluginError::other);
        }
        Err(PluginError::Unsupported(item.id.clone()))
    }

    /// Who you looked up and which logins you opened stay out of `usage.json`
    /// and the search history.
    fn tracks_usage(&self) -> bool {
        false
    }

    fn refresh(&self) -> PluginResult<()> {
        // Both instances refresh; the second load is a cheap repeat, so only
        // the family instance loads.
        if self.id == FAMILY_ID {
            if let Some(shared) = &self.shared {
                shared.load();
            }
        }
        Ok(())
    }

    fn attach_notifier(&self, notifier: ResultsNotifier) {
        if let Some(shared) = &self.shared {
            if let Ok(mut slot) = shared.notifier.lock() {
                slot.get_or_insert(notifier);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_util::MockPlatform;

    fn contact(
        id: &str,
        name: &str,
        emails: &[&str],
        phones: &[&str],
        company: Option<&str>,
    ) -> Contact {
        Contact {
            id: id.to_owned(),
            name: name.to_owned(),
            emails: emails.iter().map(|s| (*s).to_owned()).collect(),
            phones: phones.iter().map(|s| (*s).to_owned()).collect(),
            company: company.map(str::to_owned),
            card_id: None,
        }
    }

    fn people() -> Vec<Contact> {
        vec![
            contact(
                "ada",
                "Ada Lovelace",
                &["ada@example.org", "ada.l@work.example"],
                &["+44 20 7946 0000", "+44 7700 900123"],
                Some("Analytical Engines"),
            ),
            contact(
                "grace",
                "Grace Hopper",
                &["grace@navy.example"],
                &[],
                Some("US Navy"),
            ),
            contact("alan", "Alan Turing", &[], &["(555) 010-0199"], None),
            contact("nobody", "No Details", &[], &[], None),
        ]
    }

    fn enabled() -> ContactsConfig {
        ContactsConfig {
            enabled: true,
            ..ContactsConfig::default()
        }
    }

    /// The `c` instance, loaded from the mock's system address book.
    fn loaded(platform: &Arc<MockPlatform>, config: ContactsConfig) -> Vec<Arc<dyn Plugin>> {
        let plugins = ContactsPlugin::instances(&config, platform.clone());
        plugins[0].refresh().unwrap();
        plugins
    }

    fn granted(list: Vec<Contact>) -> Arc<MockPlatform> {
        let platform = MockPlatform::empty();
        *platform.contacts_access.lock().unwrap() = Some(ContactsAccess::Granted);
        *platform.system_contacts.lock().unwrap() = list;
        platform
    }

    fn titles(rows: &[ResultItem]) -> Vec<&str> {
        rows.iter().map(|r| r.title.as_str()).collect()
    }

    #[test]
    fn two_instances_serve_the_two_keywords() {
        let plugins = ContactsPlugin::instances(&enabled(), MockPlatform::empty());
        let keywords: Vec<_> = plugins.iter().map(|p| (p.id(), p.keyword())).collect();
        assert_eq!(
            keywords,
            [("contacts", Some("c")), ("contacts:at", Some("@"))]
        );
        assert!(plugins.iter().all(|p| !p.global() && !p.tracks_usage()));

        let mut config = enabled();
        config.keyword = "@".into();
        assert_eq!(
            ContactsPlugin::instances(&config, MockPlatform::empty()).len(),
            1
        );
    }

    #[test]
    fn off_by_default_with_a_row_that_says_how_to_turn_it_on() {
        let platform = granted(people());
        let plugins = ContactsPlugin::instances(&ContactsConfig::default(), platform);
        plugins[0].refresh().unwrap();
        let rows = plugins[0].query("ada");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Contacts are off");
        assert_eq!(
            rows[0].action,
            Action::CopyText {
                text: ENABLE_SNIPPET.to_owned()
            }
        );
    }

    #[test]
    fn finds_people_by_name_email_company_and_phone() {
        let platform = granted(people());
        let plugins = loaded(&platform, enabled());
        let ask = |q: &str| {
            titles(&plugins[0].query(q))
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };

        assert_eq!(ask("ada")[0], "Ada Lovelace");
        assert_eq!(ask("hopper")[0], "Grace Hopper");
        assert_eq!(ask("navy.example")[0], "Grace Hopper");
        assert_eq!(ask("analytical")[0], "Ada Lovelace");
        assert_eq!(ask("555 010")[0], "Alan Turing");
        assert_eq!(ask("+44 7700")[0], "Ada Lovelace");
        assert_eq!(ask("0199")[0], "Alan Turing");
        assert!(ask("zzzzqq").is_empty());
    }

    #[test]
    fn the_same_rows_come_from_the_at_instance() {
        let platform = granted(people());
        let plugins = loaded(&platform, enabled());
        let from_c = titles(&plugins[0].query("gra")).len();
        let at = plugins[1].query("gra");
        assert_eq!(at.len(), from_c);
        assert_eq!(at[0].plugin_id, "contacts:at");
        assert!(at[0].id.starts_with("contacts:at:"));
    }

    #[test]
    fn an_empty_query_lists_the_address_book_alphabetically() {
        let platform = granted(people());
        let plugins = loaded(&platform, enabled());
        let rows = plugins[0].query("");
        assert_eq!(
            titles(&rows),
            ["Ada Lovelace", "Alan Turing", "Grace Hopper", "No Details"]
        );
        assert!(rows.windows(2).all(|w| w[0].score > w[1].score));
    }

    #[test]
    fn a_row_copies_the_email_and_offers_the_rest_as_secondary_actions() {
        let platform = granted(people());
        let plugins = loaded(&platform, enabled());
        let ada = plugins[0].query("ada lovelace").remove(0);

        assert_eq!(
            ada.action,
            Action::CopyText {
                text: "ada@example.org".into()
            }
        );
        assert!(ada
            .subtitle
            .starts_with("ada@example.org · +44 20 7946 0000 · Analytical Engines"));
        assert_eq!(ada.large_text.as_deref(), Some("+44 20 7946 0000"));

        let by_label = |label: &str| ada.secondary.iter().find(|s| s.label == label).unwrap();
        let mail = by_label("Write an email");
        assert_eq!(mail.modifier, Some(Modifier::Ctrl));
        assert_eq!(
            mail.action,
            Action::OpenUrl {
                url: "mailto:ada@example.org".into()
            }
        );
        assert_eq!(
            by_label("Copy phone number").modifier,
            Some(Modifier::Shift)
        );
        let call = by_label("Call");
        assert_eq!(call.modifier, Some(Modifier::Alt));
        assert_eq!(
            call.action,
            Action::Custom {
                payload: "call:+44 20 7946 0000".into()
            }
        );
        assert_eq!(
            by_label("Copy ada.l@work.example").action,
            Action::CopyText {
                text: "ada.l@work.example".into()
            }
        );
        assert_eq!(
            by_label("Copy +44 7700 900123").action,
            Action::CopyText {
                text: "+44 7700 900123".into()
            }
        );
        // One action per modifier.
        let modifiers: Vec<_> = ada.secondary.iter().filter_map(|s| s.modifier).collect();
        assert_eq!(modifiers, [Modifier::Ctrl, Modifier::Shift, Modifier::Alt]);
    }

    #[test]
    fn a_contact_without_an_email_copies_the_number_and_one_without_anything_the_name() {
        let platform = granted(people());
        let plugins = loaded(&platform, enabled());
        let alan = plugins[0].query("alan").remove(0);
        assert_eq!(
            alan.action,
            Action::CopyText {
                text: "(555) 010-0199".into()
            }
        );
        // The number is the row's own action, so it is not offered twice.
        assert!(alan
            .secondary
            .iter()
            .all(|s| s.label != "Copy phone number"));
        assert!(alan.secondary.iter().any(|s| s.label == "Call"));
        assert!(alan.secondary.iter().all(|s| s.label != "Write an email"));

        let nobody = plugins[0].query("no details").remove(0);
        assert_eq!(
            nobody.action,
            Action::CopyText {
                text: "No Details".into()
            }
        );
        assert!(nobody.secondary.is_empty());
        assert_eq!(nobody.large_text, None);
    }

    #[test]
    fn execute_copies_composes_and_calls() {
        let platform = granted(people());
        let plugins = loaded(&platform, enabled());
        let plugin = &plugins[0];
        let ada = plugin.query("ada lovelace").remove(0);

        plugin.execute(&ada).unwrap();
        assert_eq!(*platform.clipboard.lock().unwrap(), ["ada@example.org"]);

        for index in 0..ada.secondary.len() {
            plugin
                .execute(&ada.secondary_as_primary(index).unwrap())
                .unwrap();
        }
        assert_eq!(
            *platform.opened_urls.lock().unwrap(),
            ["mailto:ada@example.org"]
        );
        assert_eq!(
            *platform.opened_links.lock().unwrap(),
            ["tel:+442079460000"]
        );
    }

    #[test]
    fn a_number_that_is_not_dialable_is_refused() {
        let platform = MockPlatform::empty();
        let plugin = &ContactsPlugin::instances(&enabled(), platform.clone())[0];
        let bad = ResultItem::new(
            "contacts",
            "x",
            "X",
            Action::Custom {
                payload: "call:555; calc.exe".into(),
            },
        );
        assert!(plugin.execute(&bad).is_err());
        assert!(platform.opened_links.lock().unwrap().is_empty());
        let unknown = ResultItem::new(
            "contacts",
            "x",
            "X",
            Action::Custom {
                payload: "other".into(),
            },
        );
        assert!(matches!(
            plugin.execute(&unknown),
            Err(PluginError::Unsupported(_))
        ));
    }

    #[test]
    fn the_contact_card_opens_through_the_allow_list() {
        let mut list = people();
        list[0].card_id = Some("AB12-34:ABPerson".into());
        let platform = granted(list);
        let plugins = loaded(&platform, enabled());
        let ada = plugins[0].query("ada lovelace").remove(0);
        let card = ada
            .secondary
            .iter()
            .position(|s| s.label == "Open contact card")
            .unwrap();
        plugins[0]
            .execute(&ada.secondary_as_primary(card).unwrap())
            .unwrap();
        assert_eq!(
            *platform.opened_links.lock().unwrap(),
            ["addressbook://AB12-34:ABPerson"]
        );
    }

    #[test]
    fn reads_vcard_files_and_folders_with_a_tilde() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("one.vcf"),
            "BEGIN:VCARD\nFN:Vera File\nEMAIL:vera@file.example\nEND:VCARD\n",
        )
        .unwrap();
        let config = ContactsConfig {
            enabled: true,
            use_system: false,
            vcard_files: vec![dir.path().display().to_string()],
            ..ContactsConfig::default()
        };
        let platform = granted(people());
        let plugins = loaded(&platform, config);
        // The system address book is not read when use_system is off.
        assert_eq!(titles(&plugins[0].query("")), ["Vera File"]);
    }

    #[test]
    fn duplicates_across_sources_are_listed_once_and_keys_stay_unique() {
        let mut list = people();
        list.push(contact(
            "ada",
            "Ada Lovelace",
            &["ada@example.org"],
            &["+44 20 7946 0000"],
            None,
        ));
        list.push(contact("ada", "Ada Other", &[], &[], None));
        let platform = granted(list);
        let plugins = loaded(&platform, enabled());
        let rows = plugins[0].query("ada");
        let ada_rows: Vec<_> = rows.iter().filter(|r| r.title.starts_with("Ada")).collect();
        assert_eq!(ada_rows.len(), 2, "{:?}", titles(&rows));
        let ids: std::collections::HashSet<_> = rows.iter().map(|r| &r.id).collect();
        assert_eq!(ids.len(), rows.len());
    }

    #[test]
    fn permission_is_asked_only_when_the_user_presses_enter() {
        let platform = MockPlatform::empty();
        *platform.contacts_access.lock().unwrap() = Some(ContactsAccess::NotDetermined);
        *platform.contacts_after_request.lock().unwrap() = Some(ContactsAccess::Granted);
        *platform.system_contacts.lock().unwrap() = people();
        let plugins = loaded(&platform, enabled());

        let rows = plugins[0].query("ada");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Allow Sevak to read your Contacts");
        // Loading and typing never ask.
        assert_eq!(
            *platform.contacts_access.lock().unwrap(),
            Some(ContactsAccess::NotDetermined)
        );

        let (tx, rx) = std::sync::mpsc::channel();
        plugins[0].attach_notifier(Arc::new(move |id: &str| {
            let _ = tx.send(id.to_owned());
        }));
        plugins[0].execute(&rows[0]).unwrap();
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            "contacts"
        );
        assert_eq!(plugins[0].query("ada")[0].title, "Ada Lovelace");
    }

    #[test]
    fn a_denied_address_book_explains_itself_but_vcard_contacts_still_show() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.vcf");
        fs::write(&file, "BEGIN:VCARD\nFN:Vera File\nEND:VCARD\n").unwrap();
        let platform = MockPlatform::empty();
        *platform.contacts_access.lock().unwrap() =
            Some(ContactsAccess::Denied("Allow it in Settings.".into()));
        let config = ContactsConfig {
            enabled: true,
            vcard_files: vec![file.display().to_string()],
            ..ContactsConfig::default()
        };
        let plugins = loaded(&platform, config);
        let rows = plugins[0].query("");
        assert_eq!(rows[0].title, "Vera File");
        assert_eq!(rows[1].title, "Cannot read the system address book");
        assert_eq!(rows[1].subtitle, "Allow it in Settings.");
    }

    #[test]
    fn before_the_first_load_and_with_no_sources_it_says_so() {
        let platform = MockPlatform::empty();
        let plugins = ContactsPlugin::instances(&enabled(), platform.clone());
        assert_eq!(plugins[0].query("x")[0].title, "Loading contacts…");
        plugins[0].refresh().unwrap();
        assert_eq!(plugins[0].query("x")[0].title, "No contacts found");
    }

    #[test]
    fn mailto_links_are_encoded_and_addresses_validated() {
        assert_eq!(
            mailto_url("a.b+c@x.org").as_deref(),
            Some("mailto:a.b+c@x.org")
        );
        assert_eq!(
            mailto_url("o'neil@x.org?cc=evil@y").as_deref(),
            Some("mailto:o%27neil@x.org%3Fcc%3Devil@y")
        );
        assert_eq!(mailto_url("no at sign"), None);
        assert_eq!(mailto_url("a@b\nBcc: c@d"), None);
    }

    #[test]
    fn phone_queries_need_digits_and_nothing_else() {
        assert_eq!(phone_query("+44 7700").as_deref(), Some("447700"));
        assert_eq!(phone_query("(555) 01").as_deref(), Some("55501"));
        assert_eq!(phone_query("12"), None);
        assert_eq!(phone_query("ada 555"), None);
    }
}
