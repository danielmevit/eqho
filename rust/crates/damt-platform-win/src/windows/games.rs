//! Read-only foreground and Game Bar probes. This module never writes the registry.

use damt_platform::{ForegroundObservation, ForegroundProbe};
use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, HANDLE, STILL_ACTIVE};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::SHQueryUserNotificationState;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::{Error, PWSTR};

use super::registry::{self, ENUM_READ_ACCESS};
use crate::game_bar_exe_path;

/// `HKCU\System\GameConfigStore\Children` — Game Bar's per-game store.
const GAME_BAR_CHILDREN: &str = r"System\GameConfigStore\Children";

/// Foreground process and Game Bar game list. Every method returns an empty or unknown value
/// on failure and never writes outside the process.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinForeground;

impl ForegroundProbe for WinForeground {
    fn is_supported(&self) -> bool {
        true
    }

    fn observe(&self) -> ForegroundObservation {
        let notification_state = query_notification_state();
        let Some((process_id, exe_path)) = foreground_process() else {
            return ForegroundObservation {
                notification_state,
                exe_name: None,
                exe_path: None,
                process_id: None,
            };
        };
        let exe_name = exe_path
            .as_deref()
            .and_then(|path| path.rsplit(['\\', '/']).next())
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        ForegroundObservation {
            notification_state,
            exe_name,
            exe_path,
            process_id: Some(process_id),
        }
    }

    fn game_bar_exe_paths(&self) -> Vec<String> {
        read_game_bar_exe_paths()
    }

    fn process_alive(&self, process_id: u32) -> bool {
        process_is_alive(process_id)
    }
}

fn query_notification_state() -> Option<i32> {
    // SAFETY: the function takes no arguments and writes only into its own out-parameter.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => Some(state.0),
        Err(_) => None,
    }
}

/// Foreground pid plus its image path when the path could be read.
///
/// A live process that refuses the query (access denied) is returned with `None` for the path
/// so exclusive full-screen games still have a pid for hold-until-close.
fn foreground_process() -> Option<(u32, Option<String>)> {
    // SAFETY: GetForegroundWindow reads the desktop and takes no arguments.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `hwnd` came from GetForegroundWindow and `pid` is a valid out-parameter.
    let _thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
    if pid == 0 {
        return None;
    }
    match open_query_handle(pid) {
        Ok(handle) => Some((pid, query_image_path(handle.0))),
        Err(err) if open_error_means_alive(&err) => Some((pid, None)),
        Err(_) => None,
    }
}

fn process_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    match open_query_handle(pid) {
        Ok(handle) => {
            let mut code = 0u32;
            // SAFETY: `handle` is open and `code` is a valid out-parameter.
            let queried = unsafe { GetExitCodeProcess(handle.0, &mut code) };
            // STILL_ACTIVE (259) means the process has not exited.
            let still_active = u32::try_from(STILL_ACTIVE.0).unwrap_or(259);
            queried.is_ok() && code == still_active
        }
        Err(err) => open_error_means_alive(&err),
    }
}

/// Opens `pid` for a limited query.
fn open_query_handle(pid: u32) -> Result<ProcessHandle, Error> {
    // SAFETY: OpenProcess does not dereference caller memory; the rights are the limited query set.
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map(ProcessHandle)
}

fn query_image_path(handle: HANDLE) -> Option<String> {
    query_image_path_with(handle, 1024).or_else(|| query_image_path_with(handle, 32_768))
}

fn query_image_path_with(handle: HANDLE, capacity: usize) -> Option<String> {
    let mut buffer = vec![0u16; capacity];
    let mut len = u32::try_from(buffer.len()).ok()?;
    // SAFETY: `buffer` is writable for `len` UTF-16 code units and `handle` is a live process handle.
    let result = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    };
    if result.is_err() {
        return None;
    }
    let count = usize::try_from(len).unwrap_or(0).min(buffer.len());
    let path = String::from_utf16_lossy(&buffer[..count]);
    if path.is_empty() { None } else { Some(path) }
}

/// Access denied (and any failure other than "no such process") means the process is still there.
/// `ERROR_INVALID_PARAMETER` is what `OpenProcess` returns for a pid that has already exited.
fn open_error_means_alive(err: &Error) -> bool {
    win32_code(err) != ERROR_INVALID_PARAMETER.0
}

fn win32_code(err: &Error) -> u32 {
    let code = err.code().0 as u32;
    // HRESULT_FROM_WIN32: facility 7, code in the low 16 bits.
    if code & 0xFFFF_0000 == 0x8007_0000 {
        code & 0xFFFF
    } else {
        code
    }
}

struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: the handle came from OpenProcess and is closed exactly once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn read_game_bar_exe_paths() -> Vec<String> {
    let Some(root) = registry::open_key(GAME_BAR_CHILDREN, ENUM_READ_ACCESS) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for name in registry::enum_subkey_names(&root) {
        let child_path = format!(r"{GAME_BAR_CHILDREN}\{name}");
        let Some(child) = registry::open_key(&child_path, registry::READ_ACCESS) else {
            continue;
        };
        for value_name in registry::enum_value_names(&child) {
            let Some(value) = registry::read_string(&child, &value_name) else {
                continue;
            };
            if let Some(path) = game_bar_exe_path(&value_name, &value) {
                paths.push(path);
            }
        }
    }
    paths
}
