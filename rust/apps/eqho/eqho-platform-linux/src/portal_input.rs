//! Persistent Wayland keyboard injection via `org.freedesktop.portal.RemoteDesktop` v2 over
//! dynamically loaded `libdbus-1.so.3` (zero external CLI tools, zero new Cargo dependencies).
//!
//! Unlike XWayland `XTest` (which triggers GNOME's "Remote Interaction" popup on every restart,
//! silently drops keystrokes while the dialog is open, and never reports denial), the
//! `RemoteDesktop` v2 portal:
//! - Requests keyboard-only access with `persist_mode = 2` + saved `restore_token`, so the user
//!   only clicks **Allow** once across app restarts and reboots.
//! - Waits synchronously for the permission dialog before injecting `Ctrl+V`, so even the very
//!   first dictation pastes cleanly.
//! - Returns `PortalOutcome::Denied` when the user cancels, allowing Eqho to keep the text on
//!   the clipboard and immediately surface the floating Recovery Card.

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::ptr::null;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use libloading::Library;

use crate::pure::{char_to_xkb_keysym, dbus_unique_name_to_portal_sender};

const DBUS_BUS_SESSION: c_int = 0;
const DBUS_TYPE_INT32: c_int = b'i' as c_int;
const DBUS_TYPE_UINT32: c_int = b'u' as c_int;
const DBUS_TYPE_STRING: c_int = b's' as c_int;
const DBUS_TYPE_OBJECT_PATH: c_int = b'o' as c_int;
const DBUS_TYPE_ARRAY: c_int = b'a' as c_int;
const DBUS_TYPE_VARIANT: c_int = b'v' as c_int;
const DBUS_TYPE_DICT_ENTRY: c_int = b'e' as c_int;

const PORTAL_BUS_NAME: &CStr = c"org.freedesktop.portal.Desktop";
const PORTAL_OBJECT_PATH: &CStr = c"/org/freedesktop/portal/desktop";
const PORTAL_RD_IFACE: &CStr = c"org.freedesktop.portal.RemoteDesktop";
const PORTAL_REQ_IFACE: &CStr = c"org.freedesktop.portal.Request";
const PORTAL_SESS_IFACE: &CStr = c"org.freedesktop.portal.Session";

const DEVICE_KEYBOARD: u32 = 1;
const PERSIST_UNTIL_REVOKED: u32 = 2;

/// Linux `evdev` keycodes (from `<linux/input-event-codes.h>`).
pub(crate) const EVDEV_KEY_BACKSPACE: i32 = 14;
pub(crate) const EVDEV_KEY_LEFTCTRL: i32 = 29;
pub(crate) const EVDEV_KEY_V: i32 = 47;

/// Oversized 8-byte-aligned buffer for `DBusError` (actual C struct is 32 B on 64-bit Linux).
#[repr(C, align(8))]
struct RawDbusError([u64; 8]);

impl RawDbusError {
    fn new() -> Self {
        Self([0; 8])
    }

    fn as_mut_ptr(&mut self) -> *mut c_void {
        self.0.as_mut_ptr().cast()
    }
}

/// Oversized 8-byte-aligned buffer for `DBusMessageIter` (actual C struct is 80 B on 64-bit Linux).
#[repr(C, align(8))]
struct RawDbusIter([u64; 16]);

impl RawDbusIter {
    fn new() -> Self {
        Self([0; 16])
    }

    fn as_mut_ptr(&mut self) -> *mut c_void {
        self.0.as_mut_ptr().cast()
    }
}

struct DbusApi {
    _lib: Library,
    error_init: unsafe extern "C" fn(*mut c_void),
    error_free: unsafe extern "C" fn(*mut c_void),
    error_is_set: unsafe extern "C" fn(*const c_void) -> c_uint,
    bus_get_private: unsafe extern "C" fn(c_int, *mut c_void) -> *mut c_void,
    bus_get_unique_name: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    bus_add_match: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_void),
    connection_set_exit_on_disconnect: unsafe extern "C" fn(*mut c_void, c_uint),
    connection_close: unsafe extern "C" fn(*mut c_void),
    connection_unref: unsafe extern "C" fn(*mut c_void),
    connection_send_with_reply_and_block:
        unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, *mut c_void) -> *mut c_void,
    connection_read_write: unsafe extern "C" fn(*mut c_void, c_int) -> c_uint,
    connection_pop_message: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    message_new_method_call: unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> *mut c_void,
    message_unref: unsafe extern "C" fn(*mut c_void),
    message_is_signal: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_uint,
    message_get_path: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    message_iter_init_append: unsafe extern "C" fn(*mut c_void, *mut c_void),
    message_iter_open_container:
        unsafe extern "C" fn(*mut c_void, c_int, *const c_char, *mut c_void) -> c_uint,
    message_iter_close_container: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_uint,
    message_iter_append_basic: unsafe extern "C" fn(*mut c_void, c_int, *const c_void) -> c_uint,
    message_iter_init: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_uint,
    message_iter_get_arg_type: unsafe extern "C" fn(*mut c_void) -> c_int,
    message_iter_recurse: unsafe extern "C" fn(*mut c_void, *mut c_void),
    message_iter_get_basic: unsafe extern "C" fn(*mut c_void, *mut c_void),
    message_iter_next: unsafe extern "C" fn(*mut c_void) -> c_uint,
}

// SAFETY: `DbusApi` holds immutable function pointers and a process-lifetime `Library`.
unsafe impl Send for DbusApi {}
unsafe impl Sync for DbusApi {}

static DBUS_API: OnceLock<Option<DbusApi>> = OnceLock::new();
static TOKEN_SEQ: AtomicU64 = AtomicU64::new(1);

impl DbusApi {
    fn load() -> Option<&'static Self> {
        DBUS_API
            .get_or_init(|| {
                // SAFETY: Standard `libdbus-1` C ABI symbols on Linux.
                unsafe {
                    let lib = Library::new("libdbus-1.so.3").ok()?;
                    Some(Self {
                        error_init: *lib.get(b"dbus_error_init\0").ok()?,
                        error_free: *lib.get(b"dbus_error_free\0").ok()?,
                        error_is_set: *lib.get(b"dbus_error_is_set\0").ok()?,
                        bus_get_private: *lib.get(b"dbus_bus_get_private\0").ok()?,
                        bus_get_unique_name: *lib.get(b"dbus_bus_get_unique_name\0").ok()?,
                        bus_add_match: *lib.get(b"dbus_bus_add_match\0").ok()?,
                        connection_set_exit_on_disconnect: *lib
                            .get(b"dbus_connection_set_exit_on_disconnect\0")
                            .ok()?,
                        connection_close: *lib.get(b"dbus_connection_close\0").ok()?,
                        connection_unref: *lib.get(b"dbus_connection_unref\0").ok()?,
                        connection_send_with_reply_and_block: *lib
                            .get(b"dbus_connection_send_with_reply_and_block\0")
                            .ok()?,
                        connection_read_write: *lib.get(b"dbus_connection_read_write\0").ok()?,
                        connection_pop_message: *lib.get(b"dbus_connection_pop_message\0").ok()?,
                        message_new_method_call: *lib
                            .get(b"dbus_message_new_method_call\0")
                            .ok()?,
                        message_unref: *lib.get(b"dbus_message_unref\0").ok()?,
                        message_is_signal: *lib.get(b"dbus_message_is_signal\0").ok()?,
                        message_get_path: *lib.get(b"dbus_message_get_path\0").ok()?,
                        message_iter_init_append: *lib
                            .get(b"dbus_message_iter_init_append\0")
                            .ok()?,
                        message_iter_open_container: *lib
                            .get(b"dbus_message_iter_open_container\0")
                            .ok()?,
                        message_iter_close_container: *lib
                            .get(b"dbus_message_iter_close_container\0")
                            .ok()?,
                        message_iter_append_basic: *lib
                            .get(b"dbus_message_iter_append_basic\0")
                            .ok()?,
                        message_iter_init: *lib.get(b"dbus_message_iter_init\0").ok()?,
                        message_iter_get_arg_type: *lib
                            .get(b"dbus_message_iter_get_arg_type\0")
                            .ok()?,
                        message_iter_recurse: *lib.get(b"dbus_message_iter_recurse\0").ok()?,
                        message_iter_get_basic: *lib.get(b"dbus_message_iter_get_basic\0").ok()?,
                        message_iter_next: *lib.get(b"dbus_message_iter_next\0").ok()?,
                        _lib: lib,
                    })
                }
            })
            .as_ref()
    }
}

#[derive(Debug)]
pub(crate) enum PortalOutcome {
    /// Keystrokes were successfully dispatched via `org.freedesktop.portal.RemoteDesktop`.
    Injected,
    /// The user denied or cancelled the desktop permission prompt (do NOT fall back to XTest,
    /// which would pop up a second duplicate dialog).
    Denied(String),
    /// The desktop compositor does not expose a working `RemoteDesktop` portal; caller may fall
    /// back to `wtype` / `ydotool` / X11 `XTest`.
    Unavailable,
}

struct PortalSession {
    conn: *mut c_void,
    session_path: CString,
}

// SAFETY: Access is serialized behind `PORTAL_STATE`'s `Mutex`, and `conn` is a private
// `DBusConnection` never shared with any other thread.
unsafe impl Send for PortalSession {}

struct PortalState {
    session: Option<PortalSession>,
    /// Set to `true` if `org.freedesktop.portal.RemoteDesktop` is absent/unsupported in this session.
    portal_unavailable: bool,
}

static PORTAL_STATE: OnceLock<Mutex<PortalState>> = OnceLock::new();

fn portal_state() -> &'static Mutex<PortalState> {
    PORTAL_STATE.get_or_init(|| {
        Mutex::new(PortalState {
            session: None,
            portal_unavailable: false,
        })
    })
}

fn next_token(prefix: &str) -> String {
    let seq = TOKEN_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("eqho_{prefix}_{}_{seq}", std::process::id())
}

fn restore_token_path() -> Option<PathBuf> {
    if let Some(cfg) = std::env::var_os("EQHO_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(cfg).join("wayland-remote-token"));
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join("Eqho").join("wayland-remote-token"));
    }
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("Eqho")
                .join("wayland-remote-token")
        })
}

fn load_restore_token() -> Option<String> {
    let path = restore_token_path()?;
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn save_restore_token(token: &str) {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return;
    }
    let Some(path) = restore_token_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("tmp");
    if let Ok(mut file) = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        && file.write_all(trimmed.as_bytes()).is_ok()
    {
        let _ = fs::rename(tmp, path);
    }
}

fn clear_restore_token() {
    if let Some(path) = restore_token_path() {
        let _ = fs::remove_file(path);
    }
}

enum DictVal<'a> {
    Str(&'a str),
    U32(u32),
}

unsafe fn append_options_dict(
    api: &DbusApi,
    parent_iter: &mut RawDbusIter,
    entries: &[(&str, DictVal<'_>)],
) -> bool {
    // SAFETY: Caller supplies a valid open `DBusMessageIter`.
    unsafe {
        let mut arr = RawDbusIter::new();
        if (api.message_iter_open_container)(
            parent_iter.as_mut_ptr(),
            DBUS_TYPE_ARRAY,
            c"{sv}".as_ptr(),
            arr.as_mut_ptr(),
        ) == 0
        {
            return false;
        }

        let mut ok = true;
        for (key, val) in entries {
            let Ok(ckey) = CString::new(*key) else {
                ok = false;
                break;
            };
            let mut entry = RawDbusIter::new();
            if (api.message_iter_open_container)(
                arr.as_mut_ptr(),
                DBUS_TYPE_DICT_ENTRY,
                null(),
                entry.as_mut_ptr(),
            ) == 0
            {
                ok = false;
                break;
            }
            let key_ptr = ckey.as_ptr();
            if (api.message_iter_append_basic)(
                entry.as_mut_ptr(),
                DBUS_TYPE_STRING,
                (&raw const key_ptr).cast(),
            ) == 0
            {
                ok = false;
            }
            if ok {
                let mut var = RawDbusIter::new();
                match val {
                    DictVal::Str(s) => {
                        if let Ok(cs) = CString::new(*s) {
                            let s_ptr = cs.as_ptr();
                            if (api.message_iter_open_container)(
                                entry.as_mut_ptr(),
                                DBUS_TYPE_VARIANT,
                                c"s".as_ptr(),
                                var.as_mut_ptr(),
                            ) != 0
                            {
                                if (api.message_iter_append_basic)(
                                    var.as_mut_ptr(),
                                    DBUS_TYPE_STRING,
                                    (&raw const s_ptr).cast(),
                                ) == 0
                                {
                                    ok = false;
                                }
                                let _ = (api.message_iter_close_container)(
                                    entry.as_mut_ptr(),
                                    var.as_mut_ptr(),
                                );
                            } else {
                                ok = false;
                            }
                        } else {
                            ok = false;
                        }
                    }
                    DictVal::U32(u) => {
                        if (api.message_iter_open_container)(
                            entry.as_mut_ptr(),
                            DBUS_TYPE_VARIANT,
                            c"u".as_ptr(),
                            var.as_mut_ptr(),
                        ) != 0
                        {
                            if (api.message_iter_append_basic)(
                                var.as_mut_ptr(),
                                DBUS_TYPE_UINT32,
                                (u as *const u32).cast(),
                            ) == 0
                            {
                                ok = false;
                            }
                            let _ = (api.message_iter_close_container)(
                                entry.as_mut_ptr(),
                                var.as_mut_ptr(),
                            );
                        } else {
                            ok = false;
                        }
                    }
                }
            }
            let _ = (api.message_iter_close_container)(arr.as_mut_ptr(), entry.as_mut_ptr());
            if !ok {
                break;
            }
        }

        let _ = (api.message_iter_close_container)(parent_iter.as_mut_ptr(), arr.as_mut_ptr());
        ok
    }
}

struct PortalResponse {
    code: u32,
    session_handle: Option<String>,
    restore_token: Option<String>,
    devices: Option<u32>,
}

unsafe fn parse_portal_response(api: &DbusApi, msg: *mut c_void) -> Option<PortalResponse> {
    // SAFETY: `msg` is a validated `org.freedesktop.portal.Request::Response` signal.
    unsafe {
        let mut iter = RawDbusIter::new();
        if (api.message_iter_init)(msg, iter.as_mut_ptr()) == 0 {
            return None;
        }
        if (api.message_iter_get_arg_type)(iter.as_mut_ptr()) != DBUS_TYPE_UINT32 {
            return None;
        }
        let mut code: u32 = 2;
        (api.message_iter_get_basic)(iter.as_mut_ptr(), (&raw mut code).cast());

        let mut session_handle = None;
        let mut restore_token = None;
        let mut devices = None;

        if (api.message_iter_next)(iter.as_mut_ptr()) != 0
            && (api.message_iter_get_arg_type)(iter.as_mut_ptr()) == DBUS_TYPE_ARRAY
        {
            let mut arr = RawDbusIter::new();
            (api.message_iter_recurse)(iter.as_mut_ptr(), arr.as_mut_ptr());
            while (api.message_iter_get_arg_type)(arr.as_mut_ptr()) == DBUS_TYPE_DICT_ENTRY {
                let mut entry = RawDbusIter::new();
                (api.message_iter_recurse)(arr.as_mut_ptr(), entry.as_mut_ptr());
                if (api.message_iter_get_arg_type)(entry.as_mut_ptr()) == DBUS_TYPE_STRING {
                    let mut key_ptr: *const c_char = null();
                    (api.message_iter_get_basic)(entry.as_mut_ptr(), (&raw mut key_ptr).cast());
                    let key = if key_ptr.is_null() {
                        ""
                    } else {
                        CStr::from_ptr(key_ptr).to_str().unwrap_or("")
                    };
                    if (api.message_iter_next)(entry.as_mut_ptr()) != 0
                        && (api.message_iter_get_arg_type)(entry.as_mut_ptr()) == DBUS_TYPE_VARIANT
                    {
                        let mut var = RawDbusIter::new();
                        (api.message_iter_recurse)(entry.as_mut_ptr(), var.as_mut_ptr());
                        let vtype = (api.message_iter_get_arg_type)(var.as_mut_ptr());
                        match (key, vtype) {
                            ("session_handle", DBUS_TYPE_STRING | DBUS_TYPE_OBJECT_PATH) => {
                                let mut vp: *const c_char = null();
                                (api.message_iter_get_basic)(var.as_mut_ptr(), (&raw mut vp).cast());
                                if !vp.is_null() {
                                    session_handle =
                                        Some(CStr::from_ptr(vp).to_string_lossy().into_owned());
                                }
                            }
                            ("restore_token", DBUS_TYPE_STRING) => {
                                let mut vp: *const c_char = null();
                                (api.message_iter_get_basic)(var.as_mut_ptr(), (&raw mut vp).cast());
                                if !vp.is_null() {
                                    restore_token =
                                        Some(CStr::from_ptr(vp).to_string_lossy().into_owned());
                                }
                            }
                            ("devices", DBUS_TYPE_UINT32) => {
                                let mut vu: u32 = 0;
                                (api.message_iter_get_basic)(var.as_mut_ptr(), (&raw mut vu).cast());
                                devices = Some(vu);
                            }
                            _ => {}
                        }
                    }
                }
                if (api.message_iter_next)(arr.as_mut_ptr()) == 0 {
                    break;
                }
            }
        }

        Some(PortalResponse {
            code,
            session_handle,
            restore_token,
            devices,
        })
    }
}

unsafe fn read_reply_object_path(api: &DbusApi, reply: *mut c_void) -> Option<String> {
    // SAFETY: Caller supplies a valid D-Bus method reply message.
    unsafe {
        let mut iter = RawDbusIter::new();
        if (api.message_iter_init)(reply, iter.as_mut_ptr()) == 0 {
            return None;
        }
        let arg_t = (api.message_iter_get_arg_type)(iter.as_mut_ptr());
        if arg_t != DBUS_TYPE_OBJECT_PATH && arg_t != DBUS_TYPE_STRING {
            return None;
        }
        let mut ptr: *const c_char = null();
        (api.message_iter_get_basic)(iter.as_mut_ptr(), (&raw mut ptr).cast());
        if ptr.is_null() {
            None
        } else {
            Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
        }
    }
}

unsafe fn wait_portal_response(
    api: &DbusApi,
    conn: *mut c_void,
    expected_path: &str,
    timeout: Duration,
) -> Option<PortalResponse> {
    // SAFETY: `conn` is a valid private `DBusConnection`.
    unsafe {
        let deadline = Instant::now() + timeout;
        loop {
            loop {
                let msg = (api.connection_pop_message)(conn);
                if msg.is_null() {
                    break;
                }
                let is_resp = (api.message_is_signal)(
                    msg,
                    PORTAL_REQ_IFACE.as_ptr(),
                    c"Response".as_ptr(),
                ) != 0;
                if is_resp {
                    let path_ptr = (api.message_get_path)(msg);
                    let path_matches = !path_ptr.is_null()
                        && CStr::from_ptr(path_ptr).to_str().ok() == Some(expected_path);
                    if path_matches {
                        let parsed = parse_portal_response(api, msg);
                        (api.message_unref)(msg);
                        return parsed;
                    }
                }
                (api.message_unref)(msg);
            }

            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let wait_ms = (deadline - now).as_millis().clamp(1, 100) as c_int;
            if (api.connection_read_write)(conn, wait_ms) == 0 {
                return None;
            }
        }
    }
}

unsafe fn close_portal_session(api: &DbusApi, session: PortalSession) {
    // SAFETY: Closes a portal session cleanly on its private D-Bus connection.
    unsafe {
        let msg = (api.message_new_method_call)(
            PORTAL_BUS_NAME.as_ptr(),
            session.session_path.as_ptr(),
            PORTAL_SESS_IFACE.as_ptr(),
            c"Close".as_ptr(),
        );
        if !msg.is_null() {
            let mut err = RawDbusError::new();
            (api.error_init)(err.as_mut_ptr());
            let reply = (api.connection_send_with_reply_and_block)(
                session.conn,
                msg,
                500,
                err.as_mut_ptr(),
            );
            if !reply.is_null() {
                (api.message_unref)(reply);
            }
            (api.error_free)(err.as_mut_ptr());
            (api.message_unref)(msg);
        }
        (api.connection_close)(session.conn);
        (api.connection_unref)(session.conn);
    }
}

enum OpenSessionResult {
    Ready {
        session: PortalSession,
        interactive_prompt_shown: bool,
    },
    Denied(String),
    Unavailable,
}

unsafe fn open_portal_session(api: &DbusApi) -> OpenSessionResult {
    // SAFETY: Performs standard `org.freedesktop.portal.RemoteDesktop` v2 handshake on a private
    // session-bus connection.
    unsafe {
        let mut err = RawDbusError::new();
        (api.error_init)(err.as_mut_ptr());
        let conn = (api.bus_get_private)(DBUS_BUS_SESSION, err.as_mut_ptr());
        let conn_err = (api.error_is_set)(err.as_mut_ptr()) != 0;
        (api.error_free)(err.as_mut_ptr());
        if conn.is_null() || conn_err {
            if !conn.is_null() {
                (api.connection_close)(conn);
                (api.connection_unref)(conn);
            }
            return OpenSessionResult::Unavailable;
        }
        (api.connection_set_exit_on_disconnect)(conn, 0);

        let uniq_ptr = (api.bus_get_unique_name)(conn);
        if uniq_ptr.is_null() {
            (api.connection_close)(conn);
            (api.connection_unref)(conn);
            return OpenSessionResult::Unavailable;
        }
        let sender = dbus_unique_name_to_portal_sender(&CStr::from_ptr(uniq_ptr).to_string_lossy());

        // Subscribe to portal `Request::Response` signals BEFORE sending any requests so signals
        // can never be missed in the window between call return and match installation.
        let mut match_err = RawDbusError::new();
        (api.error_init)(match_err.as_mut_ptr());
        let match_rule = c"type='signal',sender='org.freedesktop.portal.Desktop',interface='org.freedesktop.portal.Request',member='Response'";
        (api.bus_add_match)(conn, match_rule.as_ptr(), match_err.as_mut_ptr());
        (api.error_free)(match_err.as_mut_ptr());

        // 1. CreateSession
        let cs_token = next_token("cs");
        let sess_token = next_token("rd");
        let expected_cs_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{cs_token}");
        let cs_msg = (api.message_new_method_call)(
            PORTAL_BUS_NAME.as_ptr(),
            PORTAL_OBJECT_PATH.as_ptr(),
            PORTAL_RD_IFACE.as_ptr(),
            c"CreateSession".as_ptr(),
        );
        if cs_msg.is_null() {
            (api.connection_close)(conn);
            (api.connection_unref)(conn);
            return OpenSessionResult::Unavailable;
        }
        let mut iter = RawDbusIter::new();
        (api.message_iter_init_append)(cs_msg, iter.as_mut_ptr());
        let ok = append_options_dict(
            api,
            &mut iter,
            &[
                ("handle_token", DictVal::Str(&cs_token)),
                ("session_handle_token", DictVal::Str(&sess_token)),
            ],
        );
        if !ok {
            (api.message_unref)(cs_msg);
            (api.connection_close)(conn);
            (api.connection_unref)(conn);
            return OpenSessionResult::Unavailable;
        }

        let mut call_err = RawDbusError::new();
        (api.error_init)(call_err.as_mut_ptr());
        let cs_reply = (api.connection_send_with_reply_and_block)(
            conn,
            cs_msg,
            5_000,
            call_err.as_mut_ptr(),
        );
        (api.message_unref)(cs_msg);
        let cs_failed = cs_reply.is_null() || (api.error_is_set)(call_err.as_mut_ptr()) != 0;
        (api.error_free)(call_err.as_mut_ptr());
        if cs_failed {
            if !cs_reply.is_null() {
                (api.message_unref)(cs_reply);
            }
            (api.connection_close)(conn);
            (api.connection_unref)(conn);
            return OpenSessionResult::Unavailable;
        }
        let cs_handle =
            read_reply_object_path(api, cs_reply).unwrap_or_else(|| expected_cs_path.clone());
        (api.message_unref)(cs_reply);

        let Some(cs_resp) = wait_portal_response(api, conn, &cs_handle, Duration::from_secs(5))
        else {
            (api.connection_close)(conn);
            (api.connection_unref)(conn);
            return OpenSessionResult::Unavailable;
        };
        let Some(session_handle_str) = cs_resp.session_handle.filter(|_| cs_resp.code == 0) else {
            (api.connection_close)(conn);
            (api.connection_unref)(conn);
            return OpenSessionResult::Unavailable;
        };
        let Ok(session_path) = CString::new(session_handle_str) else {
            (api.connection_close)(conn);
            (api.connection_unref)(conn);
            return OpenSessionResult::Unavailable;
        };

        // 2. SelectDevices (KEYBOARD only, persist_mode = 2, optional saved restore_token)
        let saved_token = load_restore_token();
        let had_restore_token = saved_token.is_some();
        let sd_token = next_token("sd");
        let expected_sd_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{sd_token}");
        let sd_msg = (api.message_new_method_call)(
            PORTAL_BUS_NAME.as_ptr(),
            PORTAL_OBJECT_PATH.as_ptr(),
            PORTAL_RD_IFACE.as_ptr(),
            c"SelectDevices".as_ptr(),
        );
        if sd_msg.is_null() {
            close_portal_session(api, PortalSession { conn, session_path });
            return OpenSessionResult::Unavailable;
        }
        let mut sd_iter = RawDbusIter::new();
        (api.message_iter_init_append)(sd_msg, sd_iter.as_mut_ptr());
        let sess_ptr = session_path.as_ptr();
        let _ = (api.message_iter_append_basic)(
            sd_iter.as_mut_ptr(),
            DBUS_TYPE_OBJECT_PATH,
            (&raw const sess_ptr).cast(),
        );
        let mut sd_opts = vec![
            ("handle_token", DictVal::Str(&sd_token)),
            ("types", DictVal::U32(DEVICE_KEYBOARD)),
            ("persist_mode", DictVal::U32(PERSIST_UNTIL_REVOKED)),
        ];
        if let Some(tok) = saved_token.as_deref() {
            sd_opts.push(("restore_token", DictVal::Str(tok)));
        }
        let sd_ok = append_options_dict(api, &mut sd_iter, &sd_opts);
        if !sd_ok {
            (api.message_unref)(sd_msg);
            close_portal_session(api, PortalSession { conn, session_path });
            return OpenSessionResult::Unavailable;
        }

        let mut sd_err = RawDbusError::new();
        (api.error_init)(sd_err.as_mut_ptr());
        let sd_reply = (api.connection_send_with_reply_and_block)(
            conn,
            sd_msg,
            5_000,
            sd_err.as_mut_ptr(),
        );
        (api.message_unref)(sd_msg);
        let sd_failed = sd_reply.is_null() || (api.error_is_set)(sd_err.as_mut_ptr()) != 0;
        (api.error_free)(sd_err.as_mut_ptr());
        if sd_failed {
            if !sd_reply.is_null() {
                (api.message_unref)(sd_reply);
            }
            close_portal_session(api, PortalSession { conn, session_path });
            return OpenSessionResult::Unavailable;
        }
        let sd_handle =
            read_reply_object_path(api, sd_reply).unwrap_or_else(|| expected_sd_path.clone());
        (api.message_unref)(sd_reply);

        let Some(sd_resp) = wait_portal_response(api, conn, &sd_handle, Duration::from_secs(5))
        else {
            close_portal_session(api, PortalSession { conn, session_path });
            return OpenSessionResult::Unavailable;
        };
        if sd_resp.code != 0 {
            close_portal_session(api, PortalSession { conn, session_path });
            return OpenSessionResult::Unavailable;
        }

        // 3. Start (silent when restore_token is valid; shows one-time GNOME dialog otherwise)
        let start_t0 = Instant::now();
        let st_token = next_token("st");
        let expected_st_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{st_token}");
        let st_msg = (api.message_new_method_call)(
            PORTAL_BUS_NAME.as_ptr(),
            PORTAL_OBJECT_PATH.as_ptr(),
            PORTAL_RD_IFACE.as_ptr(),
            c"Start".as_ptr(),
        );
        if st_msg.is_null() {
            close_portal_session(api, PortalSession { conn, session_path });
            return OpenSessionResult::Unavailable;
        }
        let mut st_iter = RawDbusIter::new();
        (api.message_iter_init_append)(st_msg, st_iter.as_mut_ptr());
        let sess_ptr2 = session_path.as_ptr();
        let parent_ptr = c"".as_ptr();
        let _ = (api.message_iter_append_basic)(
            st_iter.as_mut_ptr(),
            DBUS_TYPE_OBJECT_PATH,
            (&raw const sess_ptr2).cast(),
        );
        let _ = (api.message_iter_append_basic)(
            st_iter.as_mut_ptr(),
            DBUS_TYPE_STRING,
            (&raw const parent_ptr).cast(),
        );
        let st_ok = append_options_dict(
            api,
            &mut st_iter,
            &[("handle_token", DictVal::Str(&st_token))],
        );
        if !st_ok {
            (api.message_unref)(st_msg);
            close_portal_session(api, PortalSession { conn, session_path });
            return OpenSessionResult::Unavailable;
        }

        let mut st_err = RawDbusError::new();
        (api.error_init)(st_err.as_mut_ptr());
        let st_reply = (api.connection_send_with_reply_and_block)(
            conn,
            st_msg,
            5_000,
            st_err.as_mut_ptr(),
        );
        (api.message_unref)(st_msg);
        let st_failed = st_reply.is_null() || (api.error_is_set)(st_err.as_mut_ptr()) != 0;
        (api.error_free)(st_err.as_mut_ptr());
        if st_failed {
            if !st_reply.is_null() {
                (api.message_unref)(st_reply);
            }
            close_portal_session(api, PortalSession { conn, session_path });
            return OpenSessionResult::Unavailable;
        }
        let st_handle =
            read_reply_object_path(api, st_reply).unwrap_or_else(|| expected_st_path.clone());
        (api.message_unref)(st_reply);

        let Some(st_resp) = wait_portal_response(api, conn, &st_handle, Duration::from_secs(60))
        else {
            close_portal_session(api, PortalSession { conn, session_path });
            return OpenSessionResult::Denied(
                "Remote Interaction permission timed out — allow it once to enable auto-paste."
                    .to_owned(),
            );
        };
        let elapsed = start_t0.elapsed();

        if st_resp.code != 0 {
            if had_restore_token {
                clear_restore_token();
            }
            close_portal_session(api, PortalSession { conn, session_path });
            return OpenSessionResult::Denied(
                "Remote Interaction permission wasn't granted — click Copy or allow Remote Interaction to auto-paste."
                    .to_owned(),
            );
        }

        if let Some(devices) = st_resp.devices
            && (devices & DEVICE_KEYBOARD) == 0
        {
            close_portal_session(api, PortalSession { conn, session_path });
            return OpenSessionResult::Denied(
                "Keyboard access wasn't enabled in Remote Interaction — click Copy or allow keyboard sharing."
                    .to_owned(),
            );
        }

        if let Some(new_token) = st_resp.restore_token.as_deref() {
            save_restore_token(new_token);
        }

        let interactive_prompt_shown = !had_restore_token || elapsed >= Duration::from_millis(350);
        OpenSessionResult::Ready {
            session: PortalSession { conn, session_path },
            interactive_prompt_shown,
        }
    }
}

unsafe fn notify_keycode(
    api: &DbusApi,
    session: &PortalSession,
    keycode: i32,
    state: u32,
) -> bool {
    // SAFETY: Sends `NotifyKeyboardKeycode` on an active `RemoteDesktop` portal session.
    unsafe {
        let msg = (api.message_new_method_call)(
            PORTAL_BUS_NAME.as_ptr(),
            PORTAL_OBJECT_PATH.as_ptr(),
            PORTAL_RD_IFACE.as_ptr(),
            c"NotifyKeyboardKeycode".as_ptr(),
        );
        if msg.is_null() {
            return false;
        }
        let mut iter = RawDbusIter::new();
        (api.message_iter_init_append)(msg, iter.as_mut_ptr());
        let sess_ptr = session.session_path.as_ptr();
        let _ = (api.message_iter_append_basic)(
            iter.as_mut_ptr(),
            DBUS_TYPE_OBJECT_PATH,
            (&raw const sess_ptr).cast(),
        );
        let ok_dict = append_options_dict(api, &mut iter, &[]);
        let _ = (api.message_iter_append_basic)(
            iter.as_mut_ptr(),
            DBUS_TYPE_INT32,
            (&raw const keycode).cast(),
        );
        let _ = (api.message_iter_append_basic)(
            iter.as_mut_ptr(),
            DBUS_TYPE_UINT32,
            (&raw const state).cast(),
        );
        if !ok_dict {
            (api.message_unref)(msg);
            return false;
        }
        let mut err = RawDbusError::new();
        (api.error_init)(err.as_mut_ptr());
        let reply =
            (api.connection_send_with_reply_and_block)(session.conn, msg, 2_000, err.as_mut_ptr());
        (api.message_unref)(msg);
        let ok = !reply.is_null() && (api.error_is_set)(err.as_mut_ptr()) == 0;
        if !reply.is_null() {
            (api.message_unref)(reply);
        }
        (api.error_free)(err.as_mut_ptr());
        ok
    }
}

unsafe fn notify_keysym(api: &DbusApi, session: &PortalSession, keysym: i32, state: u32) -> bool {
    // SAFETY: Sends `NotifyKeyboardKeysym` on an active `RemoteDesktop` portal session.
    unsafe {
        let msg = (api.message_new_method_call)(
            PORTAL_BUS_NAME.as_ptr(),
            PORTAL_OBJECT_PATH.as_ptr(),
            PORTAL_RD_IFACE.as_ptr(),
            c"NotifyKeyboardKeysym".as_ptr(),
        );
        if msg.is_null() {
            return false;
        }
        let mut iter = RawDbusIter::new();
        (api.message_iter_init_append)(msg, iter.as_mut_ptr());
        let sess_ptr = session.session_path.as_ptr();
        let _ = (api.message_iter_append_basic)(
            iter.as_mut_ptr(),
            DBUS_TYPE_OBJECT_PATH,
            (&raw const sess_ptr).cast(),
        );
        let ok_dict = append_options_dict(api, &mut iter, &[]);
        let _ = (api.message_iter_append_basic)(
            iter.as_mut_ptr(),
            DBUS_TYPE_INT32,
            (&raw const keysym).cast(),
        );
        let _ = (api.message_iter_append_basic)(
            iter.as_mut_ptr(),
            DBUS_TYPE_UINT32,
            (&raw const state).cast(),
        );
        if !ok_dict {
            (api.message_unref)(msg);
            return false;
        }
        let mut err = RawDbusError::new();
        (api.error_init)(err.as_mut_ptr());
        let reply =
            (api.connection_send_with_reply_and_block)(session.conn, msg, 2_000, err.as_mut_ptr());
        (api.message_unref)(msg);
        let ok = !reply.is_null() && (api.error_is_set)(err.as_mut_ptr()) == 0;
        if !reply.is_null() {
            (api.message_unref)(reply);
        }
        (api.error_free)(err.as_mut_ptr());
        ok
    }
}

fn with_active_session(
    op: impl Fn(&DbusApi, &PortalSession) -> bool,
) -> PortalOutcome {
    let Some(api) = DbusApi::load() else {
        return PortalOutcome::Unavailable;
    };
    let Ok(mut state) = portal_state().lock() else {
        return PortalOutcome::Unavailable;
    };
    if state.portal_unavailable {
        return PortalOutcome::Unavailable;
    }

    // Attempt with existing session first; if the session was revoked/closed by the compositor,
    // discard it and try opening a fresh one once.
    if let Some(session) = state.session.as_ref() {
        if op(api, session) {
            return PortalOutcome::Injected;
        }
        if let Some(stale) = state.session.take() {
            // SAFETY: Closing dead portal session cleanly.
            unsafe { close_portal_session(api, stale) };
        }
    }

    // SAFETY: Opens a new persistent `RemoteDesktop` portal session on a private D-Bus connection.
    match unsafe { open_portal_session(api) } {
        OpenSessionResult::Ready {
            session,
            interactive_prompt_shown,
        } => {
            if interactive_prompt_shown {
                // Give GNOME/Mutter a moment to dismiss the modal permission dialog and restore
                // focus to the user's target editor before delivering the keystrokes.
                thread::sleep(Duration::from_millis(260));
            }
            let ok = op(api, &session);
            if ok {
                state.session = Some(session);
                PortalOutcome::Injected
            } else {
                // SAFETY: Closing session that rejected injection.
                unsafe { close_portal_session(api, session) };
                PortalOutcome::Denied(
                    "Wayland desktop rejected synthetic keyboard input — click Copy to paste manually."
                        .to_owned(),
                )
            }
        }
        OpenSessionResult::Denied(msg) => PortalOutcome::Denied(msg),
        OpenSessionResult::Unavailable => {
            state.portal_unavailable = true;
            PortalOutcome::Unavailable
        }
    }
}

/// Send `Ctrl+V` through the persistent Wayland `RemoteDesktop` portal.
pub(crate) fn send_paste_shortcut() -> PortalOutcome {
    with_active_session(|api, session| {
        // SAFETY: Valid active `RemoteDesktop` session and evdev keycodes.
        unsafe {
            let ok_down = notify_keycode(api, session, EVDEV_KEY_LEFTCTRL, 1)
                && notify_keycode(api, session, EVDEV_KEY_V, 1);
            if ok_down {
                thread::sleep(Duration::from_millis(15));
            }
            let ok_up = notify_keycode(api, session, EVDEV_KEY_V, 0)
                && notify_keycode(api, session, EVDEV_KEY_LEFTCTRL, 0);
            ok_down && ok_up
        }
    })
}

/// Send `count` Backspace presses through the persistent Wayland `RemoteDesktop` portal.
pub(crate) fn send_backspaces(count: usize) -> PortalOutcome {
    with_active_session(|api, session| {
        // SAFETY: Valid active `RemoteDesktop` session and evdev backspace keycode.
        unsafe {
            for _ in 0..count {
                if !notify_keycode(api, session, EVDEV_KEY_BACKSPACE, 1)
                    || !notify_keycode(api, session, EVDEV_KEY_BACKSPACE, 0)
                {
                    return false;
                }
            }
            true
        }
    })
}

/// Type arbitrary Unicode text via `RemoteDesktop.NotifyKeyboardKeysym` (works even in terminals
/// when clipboard auto-paste is disabled).
pub(crate) fn type_unicode_text(text: &str) -> PortalOutcome {
    with_active_session(|api, session| {
        // SAFETY: Valid active `RemoteDesktop` session and XKB Unicode keysyms.
        unsafe {
            for ch in text.chars() {
                if let Some(ks) = char_to_xkb_keysym(ch)
                    && (!notify_keysym(api, session, ks, 1) || !notify_keysym(api, session, ks, 0))
                {
                    return false;
                }
            }
            true
        }
    })
}
