//! A closed list of non-web links Sevak may open.
//!
//! [`crate::open::open_url`] accepts only `http(s):` and `mailto:` on purpose:
//! links come from search results and user configuration, and other schemes can
//! start arbitrary handlers. A few plugins have a real need for one more
//! scheme each, so instead of loosening `open_url` they build a [`DeepLink`].
//!
//! A `DeepLink` can only be made by the constructors below, each of which builds
//! the whole URL from validated pieces:
//!
//! | Constructor | Scheme | Used by |
//! |---|---|---|
//! | [`DeepLink::tel`] | `tel:` | contacts: call a number |
//! | [`DeepLink::address_book_card`] | `addressbook://` | contacts: open a card in macOS Contacts |
//! | [`DeepLink::onepassword_item`] | `onepassword://view-item/` | 1Password: show an item in the app |
//!
//! Anything else (a different scheme, extra path or query pieces, characters
//! that could end the value early) is refused with `None`.

use std::fmt;

/// A link on the allow-list; open it with
/// [`PlatformProvider::open_link`](crate::PlatformProvider::open_link).
#[derive(Clone, PartialEq, Eq)]
pub struct DeepLink {
    url: String,
}

// By hand: a `tel:` number or an item id is personal.
impl fmt::Debug for DeepLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = self.url.split_once(':').map_or("", |(scheme, _)| scheme);
        write!(f, "DeepLink({scheme}:…)")
    }
}

impl DeepLink {
    /// `tel:+15551234567` from a phone number as people write it
    /// (`+1 (555) 123-4567`, `020 7946 0000`). Only digits and a leading `+`
    /// are kept; 3 to 20 digits are required. Extensions and other text make
    /// the number unusable, so `None`.
    pub fn tel(number: &str) -> Option<Self> {
        let number = number.trim();
        let mut cleaned = String::new();
        for (i, c) in number.char_indices() {
            match c {
                '+' if i == 0 => cleaned.push('+'),
                '0'..='9' => cleaned.push(c),
                ' ' | '-' | '.' | '(' | ')' | '\u{a0}' => {}
                _ => return None,
            }
        }
        let digits = cleaned.chars().filter(char::is_ascii_digit).count();
        (3..=20).contains(&digits).then(|| Self {
            url: format!("tel:{cleaned}"),
        })
    }

    /// `addressbook://<id>`: a contact card in macOS Contacts. The id is the
    /// Contacts framework's contact identifier (`UUID:ABPerson`).
    pub fn address_book_card(id: &str) -> Option<Self> {
        let valid = (1..=100).contains(&id.len())
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_'));
        valid.then(|| Self {
            url: format!("addressbook://{id}"),
        })
    }

    /// `onepassword://view-item/?a=<account>&v=<vault>&i=<item>`: shows an item
    /// in the 1Password app. Each id is the 26 character identifier `op` prints
    /// (letters and digits; case does not matter to 1Password).
    pub fn onepassword_item(account: &str, vault: &str, item: &str) -> Option<Self> {
        [account, vault, item]
            .iter()
            .all(|id| is_onepassword_id(id))
            .then(|| Self {
                url: format!("onepassword://view-item/?a={account}&v={vault}&i={item}"),
            })
    }

    /// The URL to hand to the operating system.
    pub fn as_str(&self) -> &str {
        &self.url
    }
}

fn is_onepassword_id(id: &str) -> bool {
    (20..=40).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tel_keeps_digits_and_a_leading_plus() {
        let url = |n: &str| DeepLink::tel(n).map(|l| l.as_str().to_owned());
        assert_eq!(
            url("+1 (555) 123-4567").as_deref(),
            Some("tel:+15551234567")
        );
        assert_eq!(url(" 020 7946 0000 ").as_deref(), Some("tel:02079460000"));
        assert_eq!(url("555.0100").as_deref(), Some("tel:5550100"));
    }

    #[test]
    fn tel_refuses_anything_that_is_not_a_phone_number() {
        for bad in [
            "",
            "12",
            "call me",
            "555-0100 ext 5",
            "+1 555 0100; rm",
            "1+555",
            "tel:5550100",
            "555%0100",
            "123456789012345678901",
            "javascript:1234",
        ] {
            assert!(DeepLink::tel(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn address_book_ids_are_a_narrow_alphabet() {
        let id = "AB1C2D3E-1234-4ABC-9DEF-0123456789AB:ABPerson";
        assert_eq!(
            DeepLink::address_book_card(id).unwrap().as_str(),
            format!("addressbook://{id}")
        );
        for bad in [
            "",
            "a b",
            "a/b",
            "a?x=1",
            "a#b",
            "a@b",
            "../x",
            &"x".repeat(101),
        ] {
            assert!(DeepLink::address_book_card(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn onepassword_links_need_three_plain_ids() {
        let account = "A3TS2BEDIFCXBJGPI4QZ5XLMOQ";
        let vault = "kxbbwsorulrz4gcjwqhpxn55pm";
        let item = "6a6gaw4xuvyzzx7kzefbbmeq4i";
        let link = DeepLink::onepassword_item(account, vault, item).unwrap();
        assert_eq!(
            link.as_str(),
            format!("onepassword://view-item/?a={account}&v={vault}&i={item}")
        );
        assert_eq!(format!("{link:?}"), "DeepLink(onepassword:…)");

        assert!(DeepLink::onepassword_item("", vault, item).is_none());
        assert!(DeepLink::onepassword_item(account, "short", item).is_none());
        // Anything that could add a parameter or change the host is refused.
        assert!(DeepLink::onepassword_item(account, vault, &format!("{item}&h=evil")).is_none());
        assert!(DeepLink::onepassword_item(account, vault, "../../etc/passwd/xxxxxxxx").is_none());
        assert!(DeepLink::onepassword_item(&format!("{account}#"), vault, item).is_none());
    }
}
