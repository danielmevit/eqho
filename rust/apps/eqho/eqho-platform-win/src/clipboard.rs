//! Win32 Unicode clipboard service with privacy opt-out formats (`EQ-CLIP`).

use std::time::Duration;

use eqho_platform::{ClipboardService, PlatformError};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::core::HSTRING;

use crate::pure::utf16_with_nul;

const CF_UNICODETEXT: u32 = 13;

const OPEN_RETRIES: usize = 20;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(10);
const PRIVATE_FORMATS: [&str; 3] = [
    "ExcludeClipboardContentFromMonitorProcessing",
    "CanIncludeInClipboardHistory",
    "CanUploadToCloudClipboard",
];

/// Win32 clipboard service supporting private clipboard opt-out (`EQ-CLIP`).
#[derive(Debug, Default, Clone, Copy)]
pub struct WinClipboardService;

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: Constructed only after a successful `OpenClipboard` call on this thread.
        let _ = unsafe { CloseClipboard() };
    }
}

struct GlobalLockGuard(HGLOBAL);

impl Drop for GlobalLockGuard {
    fn drop(&mut self) {
        // SAFETY: Constructed only after a successful `GlobalLock` on `self.0`.
        let _ = unsafe { GlobalUnlock(self.0) };
    }
}

fn open_clipboard_with_retry() -> Result<ClipboardGuard, PlatformError> {
    for _ in 0..OPEN_RETRIES {
        // SAFETY: `OpenClipboard(None)` associates the clipboard with the current task.
        if unsafe { OpenClipboard(None) }.is_ok() {
            return Ok(ClipboardGuard);
        }
        std::thread::sleep(OPEN_RETRY_DELAY);
    }
    Err(PlatformError("clipboard busy (OpenClipboard failed)".to_owned()))
}

fn alloc_global_bytes(bytes: &[u8]) -> Result<HGLOBAL, PlatformError> {
    // SAFETY: `GlobalAlloc(GMEM_MOVEABLE, bytes.len())` allocates movable global memory.
    let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }
        .map_err(|e| PlatformError(format!("GlobalAlloc failed: {e}")))?;
    // SAFETY: `hglobal` is a valid newly allocated movable `HGLOBAL`.
    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        // SAFETY: Lock failed; free `hglobal` so it does not leak.
        let _ = unsafe { GlobalFree(Some(hglobal)) };
        return Err(PlatformError("GlobalLock returned null".to_owned()));
    }
    // SAFETY: `ptr` is locked for at least `bytes.len()` bytes and does not overlap `bytes`.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len()) };
    // SAFETY: `hglobal` was locked above and is unlocked before passing to `SetClipboardData`.
    let _ = unsafe { GlobalUnlock(hglobal) };
    Ok(hglobal)
}

impl ClipboardService for WinClipboardService {
    fn get_text(&self) -> Option<String> {
        let _clipboard = open_clipboard_with_retry().ok()?;
        // SAFETY: Clipboard is open on this thread.
        unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) }.ok()?;
        // SAFETY: Clipboard is open and `CF_UNICODETEXT` is available.
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }.ok()?;
        let hglobal = HGLOBAL(handle.0);
        // SAFETY: `hglobal` is the clipboard's `CF_UNICODETEXT` handle while clipboard is open.
        let ptr = unsafe { GlobalLock(hglobal) }.cast::<u16>();
        if ptr.is_null() {
            return None;
        }
        let _lock = GlobalLockGuard(hglobal);

        let mut len = 0usize;
        // SAFETY: `CF_UNICODETEXT` data is NUL-terminated UTF-16LE within the locked `HGLOBAL`.
        while unsafe { *ptr.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: `ptr` is valid for `len` `u16` elements until `_lock` drops.
        let units = unsafe { std::slice::from_raw_parts(ptr, len) };
        Some(String::from_utf16_lossy(units))
    }

    fn set_text(&self, text: &str, private: bool) -> Result<(), PlatformError> {
        let _clipboard = open_clipboard_with_retry()?;
        // SAFETY: Clipboard is open on this thread.
        unsafe { EmptyClipboard() }
            .map_err(|e| PlatformError(format!("EmptyClipboard failed: {e}")))?;

        let utf16 = utf16_with_nul(text);
        let mut bytes = Vec::with_capacity(utf16.len() * 2);
        for unit in utf16 {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }

        let hglobal = alloc_global_bytes(&bytes)?;
        // SAFETY: Clipboard is open and `hglobal` is unlocked movable memory. Ownership transfers
        // to the system on success; on error we free `hglobal`.
        if let Err(e) = unsafe { SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hglobal.0))) } {
            // SAFETY: `SetClipboardData` failed, so caller retains ownership of `hglobal`.
            let _ = unsafe { GlobalFree(Some(hglobal)) };
            return Err(PlatformError(format!("SetClipboardData failed: {e}")));
        }

        if private {
            let zero_dword = 0u32.to_le_bytes();
            for name in PRIVATE_FORMATS {
                let fmt_name = HSTRING::from(name);
                // SAFETY: `fmt_name` is a valid NUL-terminated UTF-16 string.
                let fmt = unsafe { RegisterClipboardFormatW(&fmt_name) };
                if fmt == 0 {
                    continue;
                }
                if let Ok(hp) = alloc_global_bytes(&zero_dword) {
                    // SAFETY: Clipboard is open and `hp` is unlocked movable memory.
                    if unsafe { SetClipboardData(fmt, Some(HANDLE(hp.0))) }.is_err() {
                        // SAFETY: `SetClipboardData` failed, so caller retains ownership of `hp`.
                        let _ = unsafe { GlobalFree(Some(hp)) };
                    }
                }
            }
        }

        Ok(())
    }
}
