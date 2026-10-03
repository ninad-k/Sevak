//! The Windows People store (`Windows.ApplicationModel.Contacts`), read-only.
//!
//! This is the address book behind the Mail and People apps: contacts synced
//! from Microsoft, Exchange and Google accounts. An unpackaged desktop app can
//! open it read-only without any manifest capability. If a machine's policy
//! refuses, the error is returned and the contacts plugin falls back to vCard
//! files.

use windows::ApplicationModel::Contacts::{
    Contact as PeopleContact, ContactManager, ContactStoreAccessType,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use crate::contacts::Contact;
use crate::error::{PlatformError, Result};

/// A guard against an address book with an absurd number of entries.
const MAX_CONTACTS: u32 = 50_000;

/// Reads the People store on a thread of its own: WinRT's blocking wait needs a
/// multithreaded COM apartment, and the caller's thread may already be in
/// another model.
pub(super) fn read_contacts() -> Result<Vec<Contact>> {
    let worker = std::thread::Builder::new()
        .name("sevak-people".into())
        .spawn(|| {
            // SAFETY: no pointers; balanced by `CoUninitialize` below on this thread.
            let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let result = read();
            if initialized {
                // SAFETY: matches the successful `CoInitializeEx` above.
                unsafe { CoUninitialize() };
            }
            result
        })
        .map_err(|err| os_error(&err.to_string()))?;
    worker
        .join()
        .unwrap_or_else(|_| Err(os_error("the contacts reader stopped unexpectedly")))
}

fn read() -> Result<Vec<Contact>> {
    let store = ContactManager::RequestStoreAsyncWithAccessType(
        ContactStoreAccessType::AllContactsReadOnly,
    )
    .and_then(|op| op.join())
    .map_err(|err| os_error(&err.message()))?;
    let found = store
        .FindContactsAsync()
        .and_then(|op| op.join())
        .map_err(|err| os_error(&err.message()))?;

    let count = found.Size().unwrap_or(0).min(MAX_CONTACTS);
    let mut contacts = Vec::with_capacity(count as usize);
    for index in 0..count {
        if let Some(contact) = found.GetAt(index).ok().and_then(|c| convert(&c)) {
            contacts.push(contact);
        }
    }
    Ok(contacts)
}

fn convert(source: &PeopleContact) -> Option<Contact> {
    let mut contact = Contact {
        id: source.Id().ok()?.to_string_lossy(),
        name: source
            .DisplayName()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default(),
        ..Contact::default()
    };

    if let Ok(emails) = source.Emails() {
        for i in 0..emails.Size().unwrap_or(0) {
            let address = emails
                .GetAt(i)
                .and_then(|e| e.Address())
                .map(|a| a.to_string_lossy());
            if let Ok(address) = address {
                if address.contains('@') && !contact.emails.contains(&address) {
                    contact.emails.push(address);
                }
            }
        }
    }
    if let Ok(phones) = source.Phones() {
        for i in 0..phones.Size().unwrap_or(0) {
            let number = phones
                .GetAt(i)
                .and_then(|p| p.Number())
                .map(|n| n.to_string_lossy());
            if let Ok(number) = number {
                if number.chars().any(|c| c.is_ascii_digit()) && !contact.phones.contains(&number) {
                    contact.phones.push(number);
                }
            }
        }
    }
    if let Ok(jobs) = source.JobInfo() {
        contact.company = (0..jobs.Size().unwrap_or(0))
            .filter_map(|i| jobs.GetAt(i).and_then(|j| j.CompanyName()).ok())
            .map(|name| name.to_string_lossy())
            .find(|name| !name.is_empty());
    }

    if contact.name.trim().is_empty() {
        contact.name = contact
            .company
            .clone()
            .or_else(|| contact.emails.first().cloned())
            .or_else(|| contact.phones.first().cloned())?;
    }
    Some(contact)
}

fn os_error(message: &str) -> PlatformError {
    PlatformError::Os {
        operation: "read the People store",
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    /// Opens the real People store and prints how many contacts it holds (never
    /// their content): `cargo test -p sevak-platform -- --ignored --nocapture
    /// people_store`.
    #[test]
    #[ignore = "reads the real People store; run manually"]
    fn people_store() {
        let outcome = super::read_contacts();
        println!("people store: {:?}", outcome.as_ref().map(Vec::len));
    }
}
