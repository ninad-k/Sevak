//! The macOS Contacts framework (`CNContactStore`), read-only.
//!
//! macOS asks the user once whether Sevak may read their contacts. Sevak never
//! shows that question on its own: [`status`] only looks at the current answer,
//! and [`request`] (the user pressed Enter on "Allow access to Contacts") asks.
//! The app bundle carries `NSContactsUsageDescription` (`src-tauri/Info.plist`),
//! without which macOS would end the process at the first access.

use std::ptr::NonNull;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::AnyThread;
use objc2_contacts::{
    CNAuthorizationStatus, CNContact, CNContactEmailAddressesKey, CNContactFamilyNameKey,
    CNContactFetchRequest, CNContactGivenNameKey, CNContactIdentifierKey, CNContactMiddleNameKey,
    CNContactNamePrefixKey, CNContactNameSuffixKey, CNContactNicknameKey,
    CNContactOrganizationNameKey, CNContactPhoneNumbersKey, CNContactStore, CNEntityType,
    CNKeyDescriptor,
};
use objc2_foundation::{NSArray, NSCopying, NSError, NSString};

use crate::contacts::{Contact, ContactsAccess};
use crate::error::{PlatformError, Result};

/// A guard against an address book with an absurd number of entries.
const MAX_CONTACTS: usize = 50_000;
/// How long the permission question may stay unanswered.
const ANSWER_WITHIN: Duration = Duration::from_secs(300);

/// The current permission, without asking anything.
pub(super) fn status() -> ContactsAccess {
    // SAFETY: a class method documented as thread safe, taking a plain enum.
    let status =
        unsafe { CNContactStore::authorizationStatusForEntityType(CNEntityType::Contacts) };
    match status {
        CNAuthorizationStatus::Authorized | CNAuthorizationStatus::Limited => {
            ContactsAccess::Granted
        }
        CNAuthorizationStatus::NotDetermined => ContactsAccess::NotDetermined,
        _ => ContactsAccess::Denied(
            "Allow Sevak under System Settings > Privacy & Security > Contacts, then reload the index."
                .to_owned(),
        ),
    }
}

/// Shows the permission question (the first time only) and waits for the answer.
pub(super) fn request() -> Result<ContactsAccess> {
    let (sender, answer) = mpsc::channel::<bool>();
    let sender = Mutex::new(sender);
    let handler = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if let Ok(sender) = sender.lock() {
            let _ = sender.send(granted.as_bool());
        }
    });
    // SAFETY: the store and the block live for the whole call; the handler only
    // sends on a channel and may run on any queue.
    unsafe {
        let store = CNContactStore::new();
        store.requestAccessForEntityType_completionHandler(CNEntityType::Contacts, &handler);
    }
    // Whatever the answer was, `status` is the truth.
    let _ = answer.recv_timeout(ANSWER_WITHIN);
    Ok(status())
}

/// Reads every contact (unified, so a person with several cards appears once).
pub(super) fn read_contacts() -> Result<Vec<Contact>> {
    // SAFETY: Objective-C calls on objects created here. The keys are the
    // framework's own constants, and a contact only has the keys requested
    // below, which are the only ones read.
    unsafe {
        let key = |name: &'static NSString| -> Retained<ProtocolObject<dyn CNKeyDescriptor>> {
            ProtocolObject::from_retained(name.copy())
        };
        let keys = NSArray::from_retained_slice(&[
            key(CNContactIdentifierKey),
            key(CNContactNamePrefixKey),
            key(CNContactGivenNameKey),
            key(CNContactMiddleNameKey),
            key(CNContactFamilyNameKey),
            key(CNContactNameSuffixKey),
            key(CNContactNicknameKey),
            key(CNContactOrganizationNameKey),
            key(CNContactPhoneNumbersKey),
            key(CNContactEmailAddressesKey),
        ]);
        let request =
            CNContactFetchRequest::initWithKeysToFetch(CNContactFetchRequest::alloc(), &keys);
        request.setUnifyResults(true);

        let found: Arc<Mutex<Vec<Contact>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&found);
        let visit = RcBlock::new(move |contact: NonNull<CNContact>, stop: NonNull<Bool>| {
            let Ok(mut list) = sink.lock() else {
                return;
            };
            if let Some(contact) = convert(contact.as_ref()) {
                list.push(contact);
            }
            if list.len() >= MAX_CONTACTS {
                *stop.as_ptr() = Bool::YES;
            }
        });

        let store = CNContactStore::new();
        let mut error: Option<Retained<NSError>> = None;
        let ok = store.enumerateContactsWithFetchRequest_error_usingBlock(
            &request,
            Some(&mut error),
            &visit,
        );
        if !ok {
            let message = error
                .map(|e| e.localizedDescription().to_string())
                .unwrap_or_else(|| "the Contacts framework refused".to_owned());
            return Err(PlatformError::Os {
                operation: "read Contacts",
                message,
            });
        }
        let contacts = found.lock().map(|list| list.clone()).unwrap_or_default();
        Ok(contacts)
    }
}

/// # Safety
/// `contact` must have been fetched with the keys requested in [`read_contacts`].
unsafe fn convert(contact: &CNContact) -> Option<Contact> {
    let text = |s: Retained<NSString>| s.to_string().trim().to_owned();

    let identifier = text(contact.identifier());
    let name = [
        text(contact.namePrefix()),
        text(contact.givenName()),
        text(contact.middleName()),
        text(contact.familyName()),
        text(contact.nameSuffix()),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ");
    let company = Some(text(contact.organizationName())).filter(|c| !c.is_empty());

    let mut emails = Vec::new();
    for labeled in contact.emailAddresses().iter() {
        let address = text(labeled.value());
        if address.contains('@') && !emails.contains(&address) {
            emails.push(address);
        }
    }
    let mut phones = Vec::new();
    for labeled in contact.phoneNumbers().iter() {
        let number = text(labeled.value().stringValue());
        if number.chars().any(|c| c.is_ascii_digit()) && !phones.contains(&number) {
            phones.push(number);
        }
    }

    let name = if name.is_empty() {
        Some(text(contact.nickname()))
            .filter(|n| !n.is_empty())
            .or_else(|| company.clone())
            .or_else(|| emails.first().cloned())
            .or_else(|| phones.first().cloned())?
    } else {
        name
    };
    Some(Contact {
        id: identifier.clone(),
        name,
        emails,
        phones,
        company,
        card_id: Some(identifier).filter(|id| !id.is_empty()),
    })
}
