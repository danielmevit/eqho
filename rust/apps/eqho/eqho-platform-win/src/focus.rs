//! Foreground window capture, verified focus restoration, and process basename lookup.

use std::ffi::c_void;
use std::thread;
use std::time::Duration;

use eqho_platform::{FocusTracker, WindowHandle};
use windows::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, HWND};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    SW_RESTORE, SetForegroundWindow, ShowWindow,
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
        // SAFETY: If `target` is already the active foreground window (e.g. dictating into the
        // same window or Onboarding test box), avoid redundant activation calls.
        let current_fg = unsafe { GetForegroundWindow() };
        if current_fg == target {
            return true;
        }

        // SAFETY: If the target window was minimized while speaking, restore it first.
        if unsafe { IsIconic(target) }.as_bool() {
            unsafe {
                let _ = ShowWindow(target, SW_RESTORE);
            }
        }

        // Try direct `SetForegroundWindow` first.
        // SAFETY: `target` is a non-null `HWND` captured earlier in this session.
        if unsafe { SetForegroundWindow(target) }.as_bool()
            && unsafe { GetForegroundWindow() } == target
        {
            return true;
        }

        // Windows foreground-lock frequently blocks background tray processes from calling
        // `SetForegroundWindow` if focus briefly moved while speaking. Temporarily attach our
        // worker thread's input queue to the current foreground thread (and target thread),
        // bring `target` to top, activate it, and immediately detach.
        // SAFETY: Win32 thread-input attachment is scoped and always detached below.
        unsafe {
            let my_tid = GetCurrentThreadId();
            let fg_tid = if !current_fg.is_invalid() && !current_fg.0.is_null() {
                GetWindowThreadProcessId(current_fg, None)
            } else {
                0
            };
            let target_tid = GetWindowThreadProcessId(target, None);

            let attached_fg = fg_tid != 0 && fg_tid != my_tid
                && AttachThreadInput(my_tid, fg_tid, true).as_bool();
            let attached_target = target_tid != 0
                && target_tid != my_tid
                && target_tid != fg_tid
                && AttachThreadInput(my_tid, target_tid, true).as_bool();

            let _ = BringWindowToTop(target);
            let _ = SetForegroundWindow(target);

            if attached_target {
                let _ = AttachThreadInput(my_tid, target_tid, false);
            }
            if attached_fg {
                let _ = AttachThreadInput(my_tid, fg_tid, false);
            }
        }

        // Poll briefly for DWM/Windows activation to settle before verifying foreground ownership.
        for _ in 0..4 {
            // SAFETY: Stateless foreground query.
            if unsafe { GetForegroundWindow() } == target {
                return true;
            }
            thread::sleep(Duration::from_millis(12));
        }
        false
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

    fn is_non_paste_target(&self, window: WindowHandle) -> bool {
        if !window.is_valid() {
            return false;
        }
        let target = HWND(window.0 as *mut c_void);
        let mut pid = 0u32;
        // SAFETY: `target` is non-null and `&raw mut pid` is a valid stack `u32`.
        let _thread = unsafe { GetWindowThreadProcessId(target, Some(&raw mut pid)) };
        if pid != 0 && pid == std::process::id() {
            return true;
        }
        let mut cls_buf = [0u16; 128];
        // SAFETY: `cls_buf` is a valid writable UTF-16 buffer.
        let len = unsafe { GetClassNameW(target, &mut cls_buf) };
        if len > 0 {
            let class_name = from_utf16_nul(&cls_buf[..(len as usize).min(cls_buf.len())]);
            if is_shell_non_paste_class(&class_name) {
                return true;
            }
        }
        false
    }
}

fn is_shell_non_paste_class(class_name: &str) -> bool {
    matches!(
        class_name,
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "NotifyIconOverflowWindow"
    )
}

#[cfg(test)]
mod tests {
    use super::is_shell_non_paste_class;

    #[test]
    fn recognizes_desktop_and_taskbar_classes_only() {
        assert!(is_shell_non_paste_class("Progman"));
        assert!(is_shell_non_paste_class("WorkerW"));
        assert!(is_shell_non_paste_class("Shell_TrayWnd"));
        assert!(is_shell_non_paste_class("Shell_SecondaryTrayWnd"));
        assert!(!is_shell_non_paste_class("CabinetWClass"));
        assert!(!is_shell_non_paste_class("CASCADIA_HOSTING_WINDOW_CLASS"));
    }
}
