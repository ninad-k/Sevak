//! macOS Dictionary Services: the definitions the Dictionary app shows, offline.
//!
//! `DCSCopyTextDefinition` returns one block of plain text for a word, drawn
//! from the user's enabled dictionaries (the New Oxford American Dictionary by
//! default). The functions are declared by hand: they are plain C, and this
//! avoids a dependency for two calls.

use std::ffi::{c_char, c_void};

type CFTypeRef = *const c_void;
type CFIndex = isize;

#[repr(C)]
#[derive(Clone, Copy)]
struct CFRange {
    location: CFIndex,
    length: CFIndex,
}

/// `kCFStringEncodingUTF8`
const UTF8: u32 = 0x0800_0100;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithBytes(
        allocator: CFTypeRef,
        bytes: *const u8,
        length: CFIndex,
        encoding: u32,
        is_external_representation: u8,
    ) -> CFTypeRef;
    fn CFStringGetLength(string: CFTypeRef) -> CFIndex;
    fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: u32) -> CFIndex;
    fn CFStringGetCString(
        string: CFTypeRef,
        buffer: *mut c_char,
        size: CFIndex,
        encoding: u32,
    ) -> u8;
    fn CFRelease(object: CFTypeRef);
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn DCSCopyTextDefinition(dictionary: CFTypeRef, text: CFTypeRef, range: CFRange) -> CFTypeRef;
}

/// The Dictionary's text for `word`, or `None` if there is no entry.
pub(super) fn definition(word: &str) -> Option<String> {
    let word = word.trim();
    if word.is_empty() || word.len() > 200 {
        return None;
    }
    // SAFETY: every CoreFoundation object created here is released on every
    // path; `bytes` outlives the string creation, which copies it; the buffer is
    // sized by CoreFoundation's own maximum for the string's UTF-8 length.
    unsafe {
        let text = CFStringCreateWithBytes(
            std::ptr::null(),
            word.as_ptr(),
            word.len() as CFIndex,
            UTF8,
            0,
        );
        if text.is_null() {
            return None;
        }
        let range = CFRange {
            location: 0,
            length: CFStringGetLength(text),
        };
        let found = DCSCopyTextDefinition(std::ptr::null(), text, range);
        CFRelease(text);
        if found.is_null() {
            return None;
        }
        let capacity = CFStringGetMaximumSizeForEncoding(CFStringGetLength(found), UTF8) + 1;
        let mut buffer = vec![0u8; capacity.max(1) as usize];
        let ok = CFStringGetCString(found, buffer.as_mut_ptr().cast(), capacity, UTF8) != 0;
        CFRelease(found);
        if !ok {
            return None;
        }
        let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
        buffer.truncate(end);
        String::from_utf8(buffer).ok()
    }
}

#[cfg(test)]
mod tests {
    /// Asks the real Dictionary: `cargo test -p sevak-platform -- --ignored
    /// --nocapture dictionary_services`.
    #[test]
    #[ignore = "needs macOS Dictionary Services"]
    fn dictionary_services() {
        let text = super::definition("dictionary").expect("no definition");
        println!("{text}");
        assert!(super::definition("qzxqzxqzx").is_none());
    }
}
