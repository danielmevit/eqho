//! Lightweight dynamic `libX11.so.6` and `libXtst.so.6` FFI bindings via `libloading`.

use std::ffi::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};
use std::sync::OnceLock;

use libloading::Library;

pub const KEY_PRESS: c_int = 2;
pub const KEY_RELEASE: c_int = 3;
pub const SELECTION_CLEAR: c_int = 29;
pub const SELECTION_REQUEST: c_int = 30;
pub const SELECTION_NOTIFY: c_int = 31;
pub const CLIENT_MESSAGE: c_int = 33;
pub const GRAB_MODE_ASYNC: c_int = 1;
pub const PROP_MODE_REPLACE: c_int = 0;
pub const XA_ATOM: c_ulong = 4;
pub const XA_STRING: c_ulong = 31;
pub const CURRENT_TIME: c_ulong = 0;

pub const LOCK_MASK: c_uint = 1 << 1; // CapsLock
pub const MOD2_MASK: c_uint = 1 << 4; // NumLock

#[repr(C)]
#[derive(Clone, Copy)]
pub struct XKeyEvent {
    pub r#type: c_int,
    pub serial: c_ulong,
    pub send_event: c_int,
    pub display: *mut c_void,
    pub window: c_ulong,
    pub root: c_ulong,
    pub subwindow: c_ulong,
    pub time: c_ulong,
    pub x: c_int,
    pub y: c_int,
    pub x_root: c_int,
    pub y_root: c_int,
    pub state: c_uint,
    pub keycode: c_uint,
    pub same_screen: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct XClientMessageEvent {
    pub r#type: c_int,
    pub serial: c_ulong,
    pub send_event: c_int,
    pub display: *mut c_void,
    pub window: c_ulong,
    pub message_type: c_ulong,
    pub format: c_int,
    pub data: [c_long; 5],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct XSelectionRequestEvent {
    pub r#type: c_int,
    pub serial: c_ulong,
    pub send_event: c_int,
    pub display: *mut c_void,
    pub owner: c_ulong,
    pub requestor: c_ulong,
    pub selection: c_ulong,
    pub target: c_ulong,
    pub property: c_ulong,
    pub time: c_ulong,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct XSelectionEvent {
    pub r#type: c_int,
    pub serial: c_ulong,
    pub send_event: c_int,
    pub display: *mut c_void,
    pub requestor: c_ulong,
    pub selection: c_ulong,
    pub target: c_ulong,
    pub property: c_ulong,
    pub time: c_ulong,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union XEvent {
    pub r#type: c_int,
    pub xkey: XKeyEvent,
    pub xclient: XClientMessageEvent,
    pub xselectionrequest: XSelectionRequestEvent,
    pub xselection: XSelectionEvent,
    pub pad: [c_long; 24],
}

pub type XErrorHandler = Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>;

/// Process-lifetime handle to `libX11.so.6` and optional `libXtst.so.6`.
///
/// Kept alive in a `OnceLock` so Xlib extension libraries (`libXtst`) are never `dlclose`-d
/// while any X11 display connection exists.
pub struct X11Api {
    _x11: Library,
    _xtst: Option<Library>,
    pub open_display: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    pub close_display: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub default_root_window: unsafe extern "C" fn(*mut c_void) -> c_ulong,
    pub create_simple_window: unsafe extern "C" fn(
        *mut c_void,
        c_ulong,
        c_int,
        c_int,
        c_uint,
        c_uint,
        c_uint,
        c_ulong,
        c_ulong,
    ) -> c_ulong,
    pub destroy_window: unsafe extern "C" fn(*mut c_void, c_ulong) -> c_int,
    pub intern_atom: unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_ulong,
    pub get_window_property: unsafe extern "C" fn(
        *mut c_void,
        c_ulong,
        c_ulong,
        c_long,
        c_long,
        c_int,
        c_ulong,
        *mut c_ulong,
        *mut c_int,
        *mut c_ulong,
        *mut c_ulong,
        *mut *mut c_uchar,
    ) -> c_int,
    pub change_property: unsafe extern "C" fn(
        *mut c_void,
        c_ulong,
        c_ulong,
        c_ulong,
        c_int,
        c_int,
        *const c_uchar,
        c_int,
    ) -> c_int,
    pub delete_property: unsafe extern "C" fn(*mut c_void, c_ulong, c_ulong) -> c_int,
    pub set_selection_owner: unsafe extern "C" fn(*mut c_void, c_ulong, c_ulong, c_ulong) -> c_int,
    pub get_selection_owner: unsafe extern "C" fn(*mut c_void, c_ulong) -> c_ulong,
    pub convert_selection:
        unsafe extern "C" fn(*mut c_void, c_ulong, c_ulong, c_ulong, c_ulong, c_ulong) -> c_int,
    pub free: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub send_event:
        unsafe extern "C" fn(*mut c_void, c_ulong, c_int, c_long, *mut XEvent) -> c_int,
    pub raise_window: unsafe extern "C" fn(*mut c_void, c_ulong) -> c_int,
    pub flush: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub sync: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    pub keysym_to_keycode: unsafe extern "C" fn(*mut c_void, c_ulong) -> c_uchar,
    pub query_keymap: unsafe extern "C" fn(*mut c_void, *mut c_char) -> c_int,
    pub grab_key:
        unsafe extern "C" fn(*mut c_void, c_int, c_uint, c_ulong, c_int, c_int, c_int) -> c_int,
    pub ungrab_key: unsafe extern "C" fn(*mut c_void, c_int, c_uint, c_ulong) -> c_int,
    pub pending: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub next_event: unsafe extern "C" fn(*mut c_void, *mut XEvent) -> c_int,
    pub peek_event: unsafe extern "C" fn(*mut c_void, *mut XEvent) -> c_int,
    pub set_error_handler: unsafe extern "C" fn(XErrorHandler) -> XErrorHandler,
    pub xtest_fake_key_event:
        Option<unsafe extern "C" fn(*mut c_void, c_uint, c_int, c_ulong) -> c_int>,
}

// SAFETY: `X11Api` stores only immutable function pointers and process-lifetime library handles.
unsafe impl Send for X11Api {}
unsafe impl Sync for X11Api {}

static X11_API: OnceLock<Option<X11Api>> = OnceLock::new();

impl X11Api {
    pub fn load() -> Option<&'static Self> {
        std::env::var_os("DISPLAY")?;
        X11_API
            .get_or_init(|| {
                // SAFETY: Standard Xlib / XTest C ABI functions in `libX11.so.6` and `libXtst.so.6`.
                unsafe {
                    let x11 = Library::new("libX11.so.6").ok()?;
                    let xtst = Library::new("libXtst.so.6").ok();
                    let xtest_fake_key_event = xtst.as_ref().and_then(|lib| {
                        lib.get::<unsafe extern "C" fn(*mut c_void, c_uint, c_int, c_ulong) -> c_int>(
                            b"XTestFakeKeyEvent\0",
                        )
                        .ok()
                        .map(|s| *s)
                    });

                    Some(Self {
                        open_display: *x11.get(b"XOpenDisplay\0").ok()?,
                        close_display: *x11.get(b"XCloseDisplay\0").ok()?,
                        default_root_window: *x11.get(b"XDefaultRootWindow\0").ok()?,
                        create_simple_window: *x11.get(b"XCreateSimpleWindow\0").ok()?,
                        destroy_window: *x11.get(b"XDestroyWindow\0").ok()?,
                        intern_atom: *x11.get(b"XInternAtom\0").ok()?,
                        get_window_property: *x11.get(b"XGetWindowProperty\0").ok()?,
                        change_property: *x11.get(b"XChangeProperty\0").ok()?,
                        delete_property: *x11.get(b"XDeleteProperty\0").ok()?,
                        set_selection_owner: *x11.get(b"XSetSelectionOwner\0").ok()?,
                        get_selection_owner: *x11.get(b"XGetSelectionOwner\0").ok()?,
                        convert_selection: *x11.get(b"XConvertSelection\0").ok()?,
                        free: *x11.get(b"XFree\0").ok()?,
                        send_event: *x11.get(b"XSendEvent\0").ok()?,
                        raise_window: *x11.get(b"XRaiseWindow\0").ok()?,
                        flush: *x11.get(b"XFlush\0").ok()?,
                        sync: *x11.get(b"XSync\0").ok()?,
                        keysym_to_keycode: *x11.get(b"XKeysymToKeycode\0").ok()?,
                        query_keymap: *x11.get(b"XQueryKeymap\0").ok()?,
                        grab_key: *x11.get(b"XGrabKey\0").ok()?,
                        ungrab_key: *x11.get(b"XUngrabKey\0").ok()?,
                        pending: *x11.get(b"XPending\0").ok()?,
                        next_event: *x11.get(b"XNextEvent\0").ok()?,
                        peek_event: *x11.get(b"XPeekEvent\0").ok()?,
                        set_error_handler: *x11.get(b"XSetErrorHandler\0").ok()?,
                        xtest_fake_key_event,
                        _x11: x11,
                        _xtst: xtst,
                    })
                }
            })
            .as_ref()
    }
}
