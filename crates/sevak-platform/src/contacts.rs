//! Address-book data shared by the contacts plugin and the OS back ends.
//!
//! A [`Contact`] is the little the launcher needs (a name, e-mail addresses,
//! phone numbers, a company). It is produced by three kinds of source:
//!
//! - vCard text ([`parse_vcards`]): `.vcf` files exported from any address book,
//!   and the vCards Evolution Data Server stores on Linux;
//! - the macOS Contacts framework and the Windows People store, behind
//!   [`PlatformProvider`](crate::PlatformProvider);
//! - nothing else. Sevak never writes to an address book.
//!
//! Contacts are kept in memory only; nothing here touches the disk except the
//! functions that are handed a path.

use std::path::{Path, PathBuf};

/// One person or organization from an address book.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Contact {
    /// Stable within its source (the vCard `UID`, or a hash of the card's
    /// content). Used to build result ids, so it must not change between runs.
    pub id: String,
    /// Display name; never empty.
    pub name: String,
    /// E-mail addresses, preferred ones first.
    pub emails: Vec<String>,
    /// Phone numbers as written in the address book, preferred ones first.
    pub phones: Vec<String>,
    pub company: Option<String>,
    /// The OS's own identifier, for opening the contact's card
    /// (`addressbook://<id>` on macOS). `None` for vCard files.
    pub card_id: Option<String>,
}

/// Whether Sevak may read the OS address book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContactsAccess {
    /// The address book can be read.
    Granted,
    /// The OS asks the user once; [`PlatformProvider::request_contacts_access`]
    /// shows that question.
    ///
    /// [`PlatformProvider::request_contacts_access`]: crate::PlatformProvider::request_contacts_access
    NotDetermined,
    /// The user (or an administrator) said no; the text says where to change it.
    Denied(String),
    /// This system has no address book Sevak can read; use vCard files.
    Unsupported,
}

/// Longest vCard file that is read (bytes). Address books with photos embedded
/// can be large; the limit keeps a wrong file from filling memory.
pub const MAX_VCARD_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Parses every `BEGIN:VCARD` .. `END:VCARD` block in `text` (vCard 2.1, 3.0
/// and 4.0). Cards without any usable name, e-mail or phone are skipped, and
/// malformed lines are ignored rather than failing the file.
pub fn parse_vcards(text: &str) -> Vec<Contact> {
    let lines = logical_lines(text);
    let mut contacts = Vec::new();
    let mut card: Option<Vec<Property>> = None;
    for line in lines {
        let Some(property) = Property::parse(&line) else {
            continue;
        };
        match (property.name.as_str(), &mut card) {
            ("BEGIN", _) if property.value.eq_ignore_ascii_case("VCARD") => {
                card = Some(Vec::new());
            }
            ("END", slot @ Some(_)) if property.value.eq_ignore_ascii_case("VCARD") => {
                if let Some(contact) = slot.take().and_then(|props| build_contact(&props)) {
                    contacts.push(contact);
                }
            }
            (_, Some(props)) => props.push(property),
            _ => {}
        }
    }
    contacts
}

/// Reads and parses a `.vcf` file. `None` if it cannot be read, is not UTF-8
/// (vCard 2.1 files in a legacy code page are decoded lossily) or is larger
/// than [`MAX_VCARD_FILE_BYTES`].
pub fn read_vcard_file(path: &Path) -> Option<Vec<Contact>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_VCARD_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    // A byte order mark would hide the first BEGIN line.
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    Some(parse_vcards(&String::from_utf8_lossy(bytes)))
}

/// Evolution Data Server's local address books: the `contacts.db` of every
/// folder below `<data dir>/evolution/addressbook`. Empty off Linux and when
/// Evolution was never used. Nothing is read.
pub fn evolution_databases() -> Vec<PathBuf> {
    if cfg!(target_os = "linux") {
        dirs::data_dir()
            .map(|data| evolution_databases_in(&data.join("evolution").join("addressbook")))
            .unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// The `contacts.db` files directly below the folders of `root`.
pub fn evolution_databases_in(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("contacts.db"))
        .filter(|db| db.is_file())
        .collect();
    found.sort();
    found
}

// ---------------------------------------------------------------------------
// vCard parsing
// ---------------------------------------------------------------------------

/// One content line: `group.NAME;PARAM=x:value`, already unfolded.
#[derive(Debug)]
struct Property {
    /// Upper case, without a group prefix.
    name: String,
    /// Parameters, upper-cased (`TYPE=PREF`, `PREF`, `PREF=1`, ...).
    params: Vec<String>,
    /// Raw value, decoded from quoted-printable but not yet unescaped.
    value: String,
}

impl Property {
    fn parse(line: &str) -> Option<Self> {
        let colon = find_unquoted(line, ':')?;
        let (head, value) = (&line[..colon], &line[colon + 1..]);
        let mut parts = head.split(';');
        let name = parts.next()?.trim();
        let name = name.rsplit('.').next().unwrap_or(name).to_ascii_uppercase();
        if name.is_empty() {
            return None;
        }
        let params: Vec<String> = parts
            .map(|p| p.trim().trim_matches('"').to_ascii_uppercase())
            .collect();
        let value = if params
            .iter()
            .any(|p| p == "QUOTED-PRINTABLE" || p == "ENCODING=QUOTED-PRINTABLE")
        {
            decode_quoted_printable(value)
        } else {
            value.to_owned()
        };
        Some(Self {
            name,
            params,
            value,
        })
    }

    /// `TYPE=PREF` (3.0), a bare `PREF` (2.1) or `PREF=1` (4.0).
    fn is_preferred(&self) -> bool {
        self.params.iter().any(|p| {
            p == "PREF"
                || p == "TYPE=PREF"
                || p.starts_with("PREF=")
                || (p.starts_with("TYPE=") && p.split([',', '=']).any(|t| t == "PREF"))
        })
    }
}

/// The index of the first `c` outside double quotes.
fn find_unquoted(line: &str, c: char) -> Option<usize> {
    let mut quoted = false;
    for (i, ch) in line.char_indices() {
        if ch == '"' {
            quoted = !quoted;
        } else if ch == c && !quoted {
            return Some(i);
        }
    }
    None
}

/// Unfolds continuation lines (a line starting with a space or tab continues
/// the previous one) and quoted-printable soft breaks (a line ending in `=`).
fn logical_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut soft_break = false;
    for raw in text.lines() {
        if soft_break {
            if let Some(last) = lines.last_mut() {
                last.pop(); // the '='
                last.push_str(raw);
                soft_break = last.ends_with('=');
                continue;
            }
        }
        if raw.starts_with([' ', '\t']) && !lines.is_empty() {
            if let Some(last) = lines.last_mut() {
                last.push_str(&raw[1..]);
                continue;
            }
        }
        if raw.trim().is_empty() {
            continue;
        }
        soft_break = raw.ends_with('=') && raw.to_ascii_uppercase().contains("QUOTED-PRINTABLE");
        lines.push(raw.to_owned());
    }
    lines
}

/// Decodes `=XX` escapes into bytes and reads the result as UTF-8.
fn decode_quoted_printable(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|d| d as u8)
}

/// Undoes vCard text escaping: `\n`, `\,`, `\;` and `\\`.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push(' '),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out.trim().to_owned()
}

/// The components of a structured value (`N`, `ORG`), split at unescaped `;`.
fn components(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                current.push(c);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ';' => parts.push(unescape(&std::mem::take(&mut current))),
            _ => current.push(c),
        }
    }
    parts.push(unescape(&current));
    parts
}

fn build_contact(props: &[Property]) -> Option<Contact> {
    let mut contact = Contact::default();
    let mut structured_name = String::new();
    let mut uid = None;
    let mut emails: Vec<(bool, String)> = Vec::new();
    let mut phones: Vec<(bool, String)> = Vec::new();

    for prop in props {
        match prop.name.as_str() {
            "FN" if contact.name.is_empty() => contact.name = unescape(&prop.value),
            "N" if structured_name.is_empty() => {
                // family; given; additional; prefix; suffix
                let n = components(&prop.value);
                let part = |i: usize| n.get(i).map(String::as_str).unwrap_or("");
                structured_name = [part(3), part(1), part(2), part(0), part(4)]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" ");
            }
            "ORG" if contact.company.is_none() => {
                contact.company = components(&prop.value)
                    .into_iter()
                    .next()
                    .filter(|c| !c.is_empty());
            }
            "EMAIL" => {
                let address = unescape(&prop.value);
                let address = strip_scheme(&address, "mailto:");
                if address.contains('@') {
                    emails.push((prop.is_preferred(), address.to_owned()));
                }
            }
            "TEL" => {
                let number = unescape(&prop.value);
                let number = strip_scheme(&number, "tel:");
                if number.chars().any(|c| c.is_ascii_digit()) {
                    phones.push((prop.is_preferred(), number.to_owned()));
                }
            }
            "UID" if uid.is_none() => {
                let value = unescape(&prop.value);
                if !value.is_empty() {
                    uid = Some(value);
                }
            }
            _ => {}
        }
    }

    // Stable sort: preferred entries first, otherwise the card's order.
    emails.sort_by_key(|(preferred, _)| !preferred);
    phones.sort_by_key(|(preferred, _)| !preferred);
    contact.emails = dedupe(emails.into_iter().map(|(_, e)| e));
    contact.phones = dedupe(phones.into_iter().map(|(_, p)| p));

    if contact.name.is_empty() {
        contact.name = structured_name;
    }
    if contact.name.is_empty() {
        contact.name = contact
            .company
            .clone()
            .or_else(|| contact.emails.first().cloned())
            .or_else(|| contact.phones.first().cloned())
            .unwrap_or_default();
    }
    if contact.name.is_empty() {
        return None;
    }
    contact.id = uid.unwrap_or_else(|| content_id(&contact));
    Some(contact)
}

fn strip_scheme<'a>(value: &'a str, scheme: &str) -> &'a str {
    let value = value.trim();
    match value.get(..scheme.len()) {
        Some(head) if head.eq_ignore_ascii_case(scheme) => value[scheme.len()..].trim(),
        _ => value,
    }
}

fn dedupe(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if !out.iter().any(|seen| seen.eq_ignore_ascii_case(&item)) {
            out.push(item);
        }
    }
    out
}

/// An id for a card without a `UID`: FNV-1a of what identifies the person.
fn content_id(contact: &Contact) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |text: &str| {
        for byte in text.bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(&contact.name);
    feed(contact.emails.first().map_or("", String::as_str));
    feed(contact.phones.first().map_or("", String::as_str));
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const V3: &str =
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ada Lovelace\r\nN:Lovelace;Ada;Augusta;;\r\n\
        ORG:Analytical Engines\\, Ltd.;Research\r\nTEL;TYPE=WORK,VOICE:+44 20 7946 0000\r\n\
        TEL;TYPE=CELL,PREF:+44 7700 900123\r\nitem1.EMAIL;TYPE=INTERNET:ada@example.org\r\n\
        EMAIL;TYPE=INTERNET,PREF:ada.l@work.example\r\nUID:abc-123\r\nEND:VCARD\r\n";

    #[test]
    fn parses_a_version_3_card() {
        let cards = parse_vcards(V3);
        assert_eq!(cards.len(), 1);
        let ada = &cards[0];
        assert_eq!(ada.id, "abc-123");
        assert_eq!(ada.name, "Ada Lovelace");
        assert_eq!(ada.company.as_deref(), Some("Analytical Engines, Ltd."));
        // The preferred entry comes first, the rest keep the card's order.
        assert_eq!(ada.phones, ["+44 7700 900123", "+44 20 7946 0000"]);
        assert_eq!(ada.emails, ["ada.l@work.example", "ada@example.org"]);
        assert_eq!(ada.card_id, None);
    }

    #[test]
    fn parses_a_version_4_card_with_uri_values() {
        let text =
            "BEGIN:VCARD\nVERSION:4.0\nFN:Grace Hopper\nTEL;VALUE=uri;PREF=1:tel:+1-555-0100\n\
            EMAIL:mailto:grace@navy.example\nEND:VCARD\n";
        let cards = parse_vcards(text);
        assert_eq!(cards[0].phones, ["+1-555-0100"]);
        assert_eq!(cards[0].emails, ["grace@navy.example"]);
    }

    #[test]
    fn name_falls_back_to_n_then_company_then_email() {
        let cards = parse_vcards(
            "BEGIN:VCARD\nN:Hopper;Grace;;Rear Adm.;\nEND:VCARD\n\
             BEGIN:VCARD\nORG:Acme\nEND:VCARD\n\
             BEGIN:VCARD\nEMAIL:x@y.example\nEND:VCARD\n\
             BEGIN:VCARD\nNOTE:nothing usable\nEND:VCARD\n",
        );
        let names: Vec<_> = cards.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Rear Adm. Grace Hopper", "Acme", "x@y.example"]);
    }

    #[test]
    fn unfolds_continuation_lines() {
        let text = "BEGIN:VCARD\nFN:A very long\n  name that wraps\nEND:VCARD\n";
        assert_eq!(parse_vcards(text)[0].name, "A very long name that wraps");
    }

    #[test]
    fn decodes_quoted_printable_with_soft_breaks() {
        let text =
            "BEGIN:VCARD\nVERSION:2.1\nFN;CHARSET=UTF-8;ENCODING=QUOTED-PRINTABLE:Ren=C3=A9 =\n\
            Descartes\nTEL;CELL:555 0101\nEND:VCARD\n";
        let cards = parse_vcards(text);
        assert_eq!(cards[0].name, "René Descartes");
        assert_eq!(cards[0].phones, ["555 0101"]);
    }

    #[test]
    fn a_card_without_a_uid_gets_a_stable_content_id() {
        let text = "BEGIN:VCARD\nFN:No Uid\nTEL:1234\nEND:VCARD\n";
        let a = parse_vcards(text);
        let b = parse_vcards(text);
        assert_eq!(a[0].id, b[0].id);
        assert_eq!(a[0].id.len(), 16);
        let other = parse_vcards("BEGIN:VCARD\nFN:No Uid\nTEL:9999\nEND:VCARD\n");
        assert_ne!(a[0].id, other[0].id);
    }

    #[test]
    fn garbage_and_unterminated_cards_do_not_panic() {
        assert!(parse_vcards("").is_empty());
        assert!(parse_vcards("not a vcard at all").is_empty());
        assert!(parse_vcards("BEGIN:VCARD\nFN:Cut off").is_empty());
        assert!(parse_vcards("END:VCARD\nFN:Orphan\n").is_empty());
        // A trailing '=' and a lone '=XY' in a quoted-printable value.
        let _ = parse_vcards("BEGIN:VCARD\nFN;ENCODING=QUOTED-PRINTABLE:a=ZZb=\nEND:VCARD\n");
        let cards = parse_vcards("BEGIN:VCARD\nFN;ENCODING=QUOTED-PRINTABLE:a=ZZb=4\nEND:VCARD\n");
        assert_eq!(cards[0].name, "a=ZZb=4");
    }

    #[test]
    fn duplicate_numbers_and_addresses_are_listed_once() {
        let text = "BEGIN:VCARD\nFN:Dup\nEMAIL:a@b.example\nEMAIL:A@B.EXAMPLE\nTEL:1\u{20}2\nTEL:1\u{20}2\nEND:VCARD\n";
        let cards = parse_vcards(text);
        assert_eq!(cards[0].emails.len(), 1);
        assert_eq!(cards[0].phones.len(), 1);
    }

    #[test]
    fn reads_files_and_skips_missing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("people.vcf");
        std::fs::write(&path, format!("\u{feff}{V3}")).unwrap();
        assert_eq!(read_vcard_file(&path).unwrap().len(), 1);
        assert!(read_vcard_file(&dir.path().join("missing.vcf")).is_none());
        assert!(read_vcard_file(dir.path()).is_none());
    }

    #[test]
    fn finds_evolution_databases_one_level_down() {
        let dir = tempfile::tempdir().unwrap();
        for folder in ["system", "trash", "empty"] {
            std::fs::create_dir_all(dir.path().join(folder)).unwrap();
        }
        std::fs::write(dir.path().join("system/contacts.db"), b"").unwrap();
        std::fs::write(dir.path().join("trash/contacts.db"), b"").unwrap();
        let found = evolution_databases_in(dir.path());
        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("system/contacts.db"));
        assert!(evolution_databases_in(&dir.path().join("nope")).is_empty());
    }
}
