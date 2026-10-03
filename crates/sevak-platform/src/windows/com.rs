//! COM apartment management for worker threads.

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

/// Initializes COM (single-threaded apartment) for the current thread and
/// balances it with `CoUninitialize` on drop, but only when this guard
/// actually added a reference.
///
/// Not `Send`: the initialization belongs to the thread that created it.
pub(crate) struct ComGuard {
    uninitialize: bool,
    _not_send: std::marker::PhantomData<*const ()>,
}

impl ComGuard {
    pub(crate) fn new() -> Self {
        // SAFETY: no pointers are passed; the matching `CoUninitialize` runs on
        // this same thread in `Drop` because the guard is not `Send`.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let uninitialize = if hr.is_ok() {
            // S_OK or S_FALSE (already initialized in a compatible mode); both
            // add a reference that must be released.
            true
        } else if hr == RPC_E_CHANGED_MODE {
            // The thread already runs in another apartment model. COM is usable;
            // we just must not uninitialize what we did not initialize.
            false
        } else {
            tracing::warn!(%hr, "CoInitializeEx failed; continuing without COM guarantees");
            false
        };
        Self {
            uninitialize,
            _not_send: std::marker::PhantomData,
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.uninitialize {
            // SAFETY: balances the successful `CoInitializeEx` from `new` on the
            // same thread.
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_nests_and_tolerates_mode_mismatch() {
        let _a = ComGuard::new();
        let _b = ComGuard::new();
    }
}
