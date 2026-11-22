//! Foreground window capture, verified focus restoration, and process basename lookup.

use std::ffi::c_void;

use eqho_platform::{FocusTracker, WindowHandle};
use windows::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};
use windows::core::PWSTR;

use crate::pure::from_utf16_nul;

/// Windows foreground window tracker.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinFocusTracker;

struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: `self.0` was returned by a successful `OpenProcess` and is closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn query_image_path_with(handle: HANDLE, capacity: usize) -> Option<String> {
    let mut buffer = vec![0u16; capacity];
    let mut size = buffer.len() as u32;
    // SAFETY: `buffer` has `size` UTF-16 code units of capacity and `size` is a valid out-param.
    match unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    } {
        Ok(()) => {
            let len = (size as usize).min(buffer.len());
            Some(from_utf16_nul(&buffer[..len]))
        }
        Err(err) if err.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => None,
        Err(_) => None,
    }
}

impl FocusTracker for WinFocusTracker {
    fn get_foreground_window(&self) -> WindowHandle {
        // SAFETY: `GetForegroundWindow` takes no arguments and returns an opaque `HWND`.
        let hwnd = unsafe { GetForegroundWindow() };
        WindowHandle(hwnd.0 as usize)
    }

    fn restore_foreground_window(&self, window: WindowHandle) -> bool {
        if !window.is_valid() {
            return false;
        }
        let target = HWND(window.0 as *mut c_void);
        // SAFETY: `target` is a non-null `HWND` from `WindowHandle`.
        if !unsafe { SetForegroundWindow(target) }.as_bool() {
            return false;
        }
        // SAFETY: Verify `target` actually became the active foreground window.
        let active = unsafe { GetForegroundWindow() };
        active == target
    }

    fn get_window_process_name(&self, window: WindowHandle) -> Option<String> {
        if !window.is_valid() {
            return None;
        }
        let target = HWND(window.0 as *mut c_void);
        let mut pid = 0u32;
        // SAFETY: `target` is non-null and `&raw mut pid` points to a valid stack `u32`.
        let _thread = unsafe { GetWindowThreadProcessId(target, Some(&raw mut pid)) };
        if pid == 0 {
            return None;
        }

        // SAFETY: `OpenProcess` with `PROCESS_QUERY_LIMITED_INFORMATION` does not dereference memory.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .ok()
            .map(ProcessHandle)?;

        let path =
            query_image_path_with(handle.0, 1024).or_else(|| query_image_path_with(handle.0, 32_768))?;
        path.rsplit(['\\', '/'])
            .next()
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
    }
}
