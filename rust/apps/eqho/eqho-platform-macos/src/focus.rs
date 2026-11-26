//! macOS foreground application tracker via `NSWorkspace` / `NSRunningApplication`.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::thread;
use std::time::Duration;

use eqho_platform::{FocusTracker, WindowHandle};

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
}

/// Tracks and restores the frontmost macOS application by PID using `NSWorkspace`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacFocusTracker;

impl FocusTracker for MacFocusTracker {
    fn get_foreground_window(&self) -> WindowHandle {
        // SAFETY: Querying `NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier`.
        unsafe {
            let ws_cls = objc_getClass(c"NSWorkspace".as_ptr());
            if ws_cls.is_null() {
                return WindowHandle::NONE;
            }
            type MsgPtr = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
            type MsgPid = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
            let send_ptr: MsgPtr =
                std::mem::transmute::<*const (), MsgPtr>(objc_msgSend as *const ());
            let send_pid: MsgPid =
                std::mem::transmute::<*const (), MsgPid>(objc_msgSend as *const ());

            let ws = send_ptr(ws_cls, sel_registerName(c"sharedWorkspace".as_ptr()));
            if ws.is_null() {
                return WindowHandle::NONE;
            }
            let app = send_ptr(ws, sel_registerName(c"frontmostApplication".as_ptr()));
            if app.is_null() {
                return WindowHandle::NONE;
            }
            let pid = send_pid(app, sel_registerName(c"processIdentifier".as_ptr()));
            if pid > 0 {
                WindowHandle(pid as usize)
            } else {
                WindowHandle::NONE
            }
        }
    }

    fn restore_foreground_window(&self, window: WindowHandle) -> bool {
        if !window.is_valid() {
            return false;
        }
        let pid = i32::try_from(window.0).unwrap_or(0);
        if pid <= 0 {
            return false;
        }
        // SAFETY: Calling `NSRunningApplication.runningApplicationWithProcessIdentifier:pid` + `activateWithOptions:`.
        unsafe {
            let cls = objc_getClass(c"NSRunningApplication".as_ptr());
            if cls.is_null() {
                return false;
            }
            type MsgByPid = unsafe extern "C" fn(*mut c_void, *mut c_void, c_int) -> *mut c_void;
            type MsgActivate = unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> u8;
            let send_pid: MsgByPid =
                std::mem::transmute::<*const (), MsgByPid>(objc_msgSend as *const ());
            let send_act: MsgActivate =
                std::mem::transmute::<*const (), MsgActivate>(objc_msgSend as *const ());

            let app = send_pid(
                cls,
                sel_registerName(c"runningApplicationWithProcessIdentifier:".as_ptr()),
                pid,
            );
            if app.is_null() {
                return false;
            }
            // NSApplicationActivateIgnoringOtherApps = 1 << 1
            let _ = send_act(app, sel_registerName(c"activateWithOptions:".as_ptr()), 1 << 1);
        }
        for _ in 0..5 {
            if self.get_foreground_window() == window {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn get_window_process_name(&self, window: WindowHandle) -> Option<String> {
        if !window.is_valid() {
            return None;
        }
        let pid = i32::try_from(window.0).ok()?;
        // SAFETY: Reading `NSRunningApplication.localizedName.UTF8String`.
        unsafe {
            let cls = objc_getClass(c"NSRunningApplication".as_ptr());
            if cls.is_null() {
                return None;
            }
            type MsgByPid = unsafe extern "C" fn(*mut c_void, *mut c_void, c_int) -> *mut c_void;
            type MsgPtr = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
            type MsgCstr = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *const c_char;
            let send_pid: MsgByPid =
                std::mem::transmute::<*const (), MsgByPid>(objc_msgSend as *const ());
            let send_ptr: MsgPtr =
                std::mem::transmute::<*const (), MsgPtr>(objc_msgSend as *const ());
            let send_cstr: MsgCstr =
                std::mem::transmute::<*const (), MsgCstr>(objc_msgSend as *const ());

            let app = send_pid(
                cls,
                sel_registerName(c"runningApplicationWithProcessIdentifier:".as_ptr()),
                pid,
            );
            if app.is_null() {
                return None;
            }
            let ns_name = send_ptr(app, sel_registerName(c"localizedName".as_ptr()));
            if ns_name.is_null() {
                return None;
            }
            let cptr = send_cstr(ns_name, sel_registerName(c"UTF8String".as_ptr()));
            if cptr.is_null() {
                return None;
            }
            let name = CStr::from_ptr(cptr).to_string_lossy().trim().to_owned();
            if name.is_empty() { None } else { Some(name) }
        }
    }
}
