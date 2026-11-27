//! X11 EWMH foreground window tracking, verified focus restoration, and process basename lookup.

use std::ffi::{c_int, c_long, c_uchar, c_ulong};
use std::ptr::{null, null_mut};
use std::thread;
use std::time::Duration;

use eqho_platform::{FocusTracker, WindowHandle};

use crate::pure::is_wayland_session;
use crate::x11_ffi::{CLIENT_MESSAGE, CURRENT_TIME, X11Api, XClientMessageEvent, XEvent};

const SUBSTRUCTURE_REDIRECT_MASK: c_long = 1 << 20;
const SUBSTRUCTURE_NOTIFY_MASK: c_long = 1 << 19;

/// Tracks and restores the foreground X11 window via `_NET_ACTIVE_WINDOW` / `_NET_WM_PID`.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxFocusTracker;

impl LinuxFocusTracker {
    fn is_wayland() -> bool {
        let wl = std::env::var("WAYLAND_DISPLAY").ok();
        let st = std::env::var("XDG_SESSION_TYPE").ok();
        is_wayland_session(wl.as_deref(), st.as_deref())
    }

    unsafe fn read_u32_prop(
        api: &X11Api,
        dpy: *mut std::ffi::c_void,
        win: c_ulong,
        atom: c_ulong,
    ) -> Option<u32> {
        let mut actual_type: c_ulong = 0;
        let mut actual_format: c_int = 0;
        let mut nitems: c_ulong = 0;
        let mut bytes_after: c_ulong = 0;
        let mut prop: *mut c_uchar = null_mut();
        // SAFETY: Valid X11 display and property pointer outputs.
        let status = unsafe {
            (api.get_window_property)(
                dpy,
                win,
                atom,
                0,
                1,
                0,
                0, // AnyPropertyType
                &mut actual_type,
                &mut actual_format,
                &mut nitems,
                &mut bytes_after,
                &mut prop,
            )
        };
        if status != 0 || prop.is_null() {
            return None;
        }
        let val = if nitems >= 1 && actual_format == 32 {
            // Xlib returns 32-bit properties as `c_ulong` elements on 64-bit Linux!
            // SAFETY: `prop` contains at least one `c_ulong` element.
            let raw = unsafe { *(prop as *const c_ulong) };
            u32::try_from(raw).ok()
        } else {
            None
        };
        // SAFETY: `prop` was allocated by `XGetWindowProperty`.
        unsafe {
            (api.free)(prop.cast());
        }
        val
    }
}

impl FocusTracker for LinuxFocusTracker {
    fn get_foreground_window(&self) -> WindowHandle {
        // On Wayland, XWayland's `_NET_ACTIVE_WINDOW` points to Mutter's hidden 1x1 dummy focus
        // proxy window rather than the user's native Wayland editor. Return `NONE` so injection
        // skips bogus EWMH focus-restore loops.
        if Self::is_wayland() {
            return WindowHandle::NONE;
        }
        let Some(api) = X11Api::load() else {
            return WindowHandle::NONE;
        };
        // SAFETY: Opening and closing a short-lived X11 connection.
        unsafe {
            let dpy = (api.open_display)(null());
            if dpy.is_null() {
                return WindowHandle::NONE;
            }
            let root = (api.default_root_window)(dpy);
            let atom = (api.intern_atom)(dpy, c"_NET_ACTIVE_WINDOW".as_ptr(), 1);
            let handle = if atom != 0 {
                Self::read_u32_prop(api, dpy, root, atom)
                    .map(|id| WindowHandle(id as usize))
                    .unwrap_or(WindowHandle::NONE)
            } else {
                WindowHandle::NONE
            };
            (api.close_display)(dpy);
            handle
        }
    }

    fn restore_foreground_window(&self, window: WindowHandle) -> bool {
        if !window.is_valid() {
            return false;
        }
        let Some(api) = X11Api::load() else {
            return false;
        };
        // SAFETY: Sending standard EWMH `_NET_ACTIVE_WINDOW` request to root window.
        unsafe {
            let dpy = (api.open_display)(null());
            if dpy.is_null() {
                return false;
            }
            let root = (api.default_root_window)(dpy);
            let atom = (api.intern_atom)(dpy, c"_NET_ACTIVE_WINDOW".as_ptr(), 0);
            if atom != 0 {
                let mut ev = XEvent {
                    xclient: XClientMessageEvent {
                        r#type: CLIENT_MESSAGE,
                        serial: 0,
                        send_event: 1,
                        display: dpy,
                        window: window.0 as c_ulong,
                        message_type: atom,
                        format: 32,
                        data: [1, CURRENT_TIME as c_long, 0, 0, 0],
                    },
                };
                (api.send_event)(
                    dpy,
                    root,
                    0,
                    SUBSTRUCTURE_REDIRECT_MASK | SUBSTRUCTURE_NOTIFY_MASK,
                    &mut ev,
                );
                (api.raise_window)(dpy, window.0 as c_ulong);
                (api.flush)(dpy);
            }
            (api.close_display)(dpy);
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
        let api = X11Api::load()?;
        // SAFETY: Reading `_NET_WM_PID` on the target X11 window.
        let pid = unsafe {
            let dpy = (api.open_display)(null());
            if dpy.is_null() {
                return None;
            }
            let atom = (api.intern_atom)(dpy, c"_NET_WM_PID".as_ptr(), 1);
            let pid = if atom != 0 {
                Self::read_u32_prop(api, dpy, window.0 as c_ulong, atom)
            } else {
                None
            };
            (api.close_display)(dpy);
            pid
        }?;
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        let name = comm.trim();
        if name.is_empty() {
            None
        } else {
            Some(name.to_owned())
        }
    }

    fn is_non_paste_target(&self, window: WindowHandle) -> bool {
        if !window.is_valid() {
            return false;
        }
        if let Some(name) = self.get_window_process_name(window) {
            let lower = name.to_ascii_lowercase();
            return matches!(lower.as_str(), "eqho" | "gnome-shell" | "plasmashell");
        }
        false
    }
}
