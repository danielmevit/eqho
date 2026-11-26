//! Persistent Wayland keyboard + clipboard portal backend (`org.freedesktop.portal.RemoteDesktop`
//! v2 + `org.freedesktop.portal.Clipboard` v1) over dynamically loaded `libdbus-1.so.3` (zero
//! external CLI tools, zero new Cargo dependencies).
//!
//! Runs a single dedicated `eqho-portal-session` background worker thread that owns a private
//! D-Bus session-bus connection exclusively so it can:
//! - Request keyboard + clipboard access once (`persist_mode = 2` + saved `restore_token`),
//!   eliminating repeated GNOME "Remote Interaction" prompts across restarts and reboots.
//! - Continuously answer `org.freedesktop.portal.Clipboard::SelectionTransfer` signals via
//!   `SelectionWrite` + `SelectionWriteDone`, bypassing Mutter 50's unfocused XWayland clipboard
//!   block so native Wayland apps receive clipboard paste cleanly.
//! - Automatically fall back to direct `RemoteDesktop.NotifyKeyboardKeysym` typing when portal
//!   clipboard access is not enabled (e.g. if the user didn't toggle "Allow Clipboard Access" or
//!   the compositor doesn't implement the Clipboard portal), so auto-paste still inserts text
//!   directly at the cursor.
//! - Return `PortalOutcome::Denied` when the user cancels permission, allowing Eqho to keep the
//!   text and immediately surface the floating Recovery Card.

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::ptr::null;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use libloading::Library;

use crate::pure::{char_to_xkb_keysym, dbus_unique_name_to_portal_sender};

const DBUS_BUS_SESSION: c_int = 0;
const DBUS_TYPE_BOOLEAN: c_int = b'b' as c_int;
const DBUS_TYPE_INT32: c_int = b'i' as c_int;
const DBUS_TYPE_UINT32: c_int = b'u' as c_int;
const DBUS_TYPE_STRING: c_int = b's' as c_int;
const DBUS_TYPE_OBJECT_PATH: c_int = b'o' as c_int;
const DBUS_TYPE_ARRAY: c_int = b'a' as c_int;
const DBUS_TYPE_VARIANT: c_int = b'v' as c_int;
const DBUS_TYPE_DICT_ENTRY: c_int = b'e' as c_int;
const DBUS_TYPE_UNIX_FD: c_int = b'h' as c_int;

const PORTAL_BUS_NAME: &CStr = c"org.freedesktop.portal.Desktop";
const PORTAL_OBJECT_PATH: &CStr = c"/org/freedesktop/portal/desktop";
const PORTAL_RD_IFACE: &CStr = c"org.freedesktop.portal.RemoteDesktop";
const PORTAL_CLIP_IFACE: &CStr = c"org.freedesktop.portal.Clipboard";
const PORTAL_REQ_IFACE: &CStr = c"org.freedesktop.portal.Request";
const PORTAL_SESS_IFACE: &CStr = c"org.freedesktop.portal.Session";

const DEVICE_KEYBOARD: u32 = 1;
const PERSIST_UNTIL_REVOKED: u32 = 2;

/// Linux `evdev` keycodes (from `<linux/input-event-codes.h>`).
pub(crate) const EVDEV_KEY_BACKSPACE: i32 = 14;
pub(crate) const EVDEV_KEY_LEFTCTRL: i32 = 29;
pub(crate) const EVDEV_KEY_V: i32 = 47;

const MAX_CLIPBOARD_READ_BYTES: u64 = 1024 * 1024;

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
static PORTAL_CLIPBOARD_ACTIVE: AtomicBool = AtomicBool::new(false);

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
    /// Text or keystrokes were successfully delivered via the Wayland desktop portal.
    Injected,
    /// The user denied or cancelled the desktop permission prompt (do NOT fall back to XTest,
    /// which would pop up a second duplicate dialog).
    Denied(String),
    /// The desktop compositor does not expose a working `RemoteDesktop` portal; caller may fall
    /// back to `wtype` / `ydotool` / X11 `XTest`.
    Unavailable,
}

enum PortalCmd {
    TrySetClipboard {
        payload: Vec<u8>,
        ack: mpsc::SyncSender<bool>,
    },
    TryReadClipboard {
        ack: mpsc::SyncSender<Option<String>>,
    },
    PasteOrType {
        text: Option<String>,
        ack: mpsc::SyncSender<PortalOutcome>,
    },
    TypeUnicode {
        text: String,
        ack: mpsc::SyncSender<PortalOutcome>,
    },
    SendBackspaces {
        count: usize,
        ack: mpsc::SyncSender<PortalOutcome>,
    },
}

struct PortalClient {
    tx: mpsc::Sender<PortalCmd>,
}

static PORTAL_CLIENT: OnceLock<Option<PortalClient>> = OnceLock::new();

fn ensure_portal_client() -> Option<&'static PortalClient> {
    PORTAL_CLIENT
        .get_or_init(|| {
            let api = DbusApi::load()?;
            let (tx, rx) = mpsc::channel::<PortalCmd>();
            thread::Builder::new()
                .name("eqho-portal-session".to_owned())
                .spawn(move || run_portal_worker(api, rx))
                .ok()?;
            Some(PortalClient { tx })
        })
        .as_ref()
}

struct ActivePortalSession {
    conn: *mut c_void,
    session_path: CString,
    clipboard_enabled: bool,
    owns_selection: bool,
    clipboard_bytes: Vec<u8>,
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
    StrArray(&'a [&'a str]),
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
                    DictVal::StrArray(items) => {
                        if (api.message_iter_open_container)(
                            entry.as_mut_ptr(),
                            DBUS_TYPE_VARIANT,
                            c"as".as_ptr(),
                            var.as_mut_ptr(),
                        ) != 0
                        {
                            let mut str_arr = RawDbusIter::new();
                            if (api.message_iter_open_container)(
                                var.as_mut_ptr(),
                                DBUS_TYPE_ARRAY,
                                c"s".as_ptr(),
                                str_arr.as_mut_ptr(),
                            ) != 0
                            {
                                for item in *items {
                                    if let Ok(ci) = CString::new(*item) {
                                        let p = ci.as_ptr();
                                        if (api.message_iter_append_basic)(
                                            str_arr.as_mut_ptr(),
                                            DBUS_TYPE_STRING,
                                            (&raw const p).cast(),
                                        ) == 0
                                        {
                                            ok = false;
                                            break;
                                        }
                                    }
                                }
                                let _ = (api.message_iter_close_container)(
                                    var.as_mut_ptr(),
                                    str_arr.as_mut_ptr(),
                                );
                            } else {
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
    clipboard_enabled: bool,
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
        let mut clipboard_enabled = false;

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
                            ("clipboard_enabled", DBUS_TYPE_BOOLEAN) => {
                                // libdbus `dbus_bool_t` is 32-bit `uint32_t`.
                                let mut vb: u32 = 0;
                                (api.message_iter_get_basic)(var.as_mut_ptr(), (&raw mut vb).cast());
                                clipboard_enabled = vb != 0;
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
            clipboard_enabled,
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

unsafe fn read_reply_unix_fd(api: &DbusApi, reply: *mut c_void) -> Option<c_int> {
    // SAFETY: Caller supplies a valid D-Bus method reply returning `(h fd)`.
    unsafe {
        let mut iter = RawDbusIter::new();
        if (api.message_iter_init)(reply, iter.as_mut_ptr()) == 0 {
            return None;
        }
        if (api.message_iter_get_arg_type)(iter.as_mut_ptr()) != DBUS_TYPE_UNIX_FD {
            return None;
        }
        let mut fd: c_int = -1;
        (api.message_iter_get_basic)(iter.as_mut_ptr(), (&raw mut fd).cast());
        if fd >= 0 { Some(fd) } else { None }
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

unsafe fn close_portal_session(api: &DbusApi, session: ActivePortalSession) {
    PORTAL_CLIPBOARD_ACTIVE.store(false, Ordering::Release);
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

unsafe fn request_portal_clipboard(api: &DbusApi, conn: *mut c_void, session_path: &CStr) {
    // SAFETY: Best-effort `org.freedesktop.portal.Clipboard.RequestClipboard(session, {})` before
    // `RemoteDesktop.Start`. Ignored cleanly if the compositor does not expose Clipboard portal.
    unsafe {
        let msg = (api.message_new_method_call)(
            PORTAL_BUS_NAME.as_ptr(),
            PORTAL_OBJECT_PATH.as_ptr(),
            PORTAL_CLIP_IFACE.as_ptr(),
            c"RequestClipboard".as_ptr(),
        );
        if msg.is_null() {
            return;
        }
        let mut iter = RawDbusIter::new();
        (api.message_iter_init_append)(msg, iter.as_mut_ptr());
        let sess_ptr = session_path.as_ptr();
        let _ = (api.message_iter_append_basic)(
            iter.as_mut_ptr(),
            DBUS_TYPE_OBJECT_PATH,
            (&raw const sess_ptr).cast(),
        );
        let ok_dict = append_options_dict(api, &mut iter, &[]);
        if ok_dict {
            let mut err = RawDbusError::new();
            (api.error_init)(err.as_mut_ptr());
            let reply =
                (api.connection_send_with_reply_and_block)(conn, msg, 3_000, err.as_mut_ptr());
            if !reply.is_null() {
                (api.message_unref)(reply);
            }
            (api.error_free)(err.as_mut_ptr());
        }
        (api.message_unref)(msg);
    }
}

unsafe fn portal_set_selection(api: &DbusApi, session: &mut ActivePortalSession, payload: Vec<u8>) -> bool {
    if !session.clipboard_enabled {
        return false;
    }
    session.clipboard_bytes = payload;
    // SAFETY: Calls `org.freedesktop.portal.Clipboard.SetSelection(session, {mime_types})`.
    unsafe {
        let msg = (api.message_new_method_call)(
            PORTAL_BUS_NAME.as_ptr(),
            PORTAL_OBJECT_PATH.as_ptr(),
            PORTAL_CLIP_IFACE.as_ptr(),
            c"SetSelection".as_ptr(),
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
        let mimes = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];
        let ok_dict = append_options_dict(api, &mut iter, &[("mime_types", DictVal::StrArray(&mimes))]);
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
        if ok {
            session.owns_selection = true;
        }
        ok
    }
}

unsafe fn portal_read_selection(api: &DbusApi, session: &ActivePortalSession) -> Option<String> {
    if !session.clipboard_enabled {
        return None;
    }
    // Avoid self-deadlock when our own portal session currently owns the Wayland clipboard.
    if session.owns_selection {
        return Some(String::from_utf8_lossy(&session.clipboard_bytes).into_owned());
    }
    // SAFETY: Calls `org.freedesktop.portal.Clipboard.SelectionRead(session, mime_type) -> (h fd)`.
    unsafe {
        for mime in [c"text/plain;charset=utf-8", c"UTF8_STRING", c"text/plain"] {
            let msg = (api.message_new_method_call)(
                PORTAL_BUS_NAME.as_ptr(),
                PORTAL_OBJECT_PATH.as_ptr(),
                PORTAL_CLIP_IFACE.as_ptr(),
                c"SelectionRead".as_ptr(),
            );
            if msg.is_null() {
                return None;
            }
            let mut iter = RawDbusIter::new();
            (api.message_iter_init_append)(msg, iter.as_mut_ptr());
            let sess_ptr = session.session_path.as_ptr();
            let mime_ptr = mime.as_ptr();
            let _ = (api.message_iter_append_basic)(
                iter.as_mut_ptr(),
                DBUS_TYPE_OBJECT_PATH,
                (&raw const sess_ptr).cast(),
            );
            let _ = (api.message_iter_append_basic)(
                iter.as_mut_ptr(),
                DBUS_TYPE_STRING,
                (&raw const mime_ptr).cast(),
            );
            let mut err = RawDbusError::new();
            (api.error_init)(err.as_mut_ptr());
            let reply = (api.connection_send_with_reply_and_block)(
                session.conn,
                msg,
                1_000,
                err.as_mut_ptr(),
            );
            (api.message_unref)(msg);
            let ok = !reply.is_null() && (api.error_is_set)(err.as_mut_ptr()) == 0;
            (api.error_free)(err.as_mut_ptr());
            if !ok {
                if !reply.is_null() {
                    (api.message_unref)(reply);
                }
                continue;
            }
            let fd_opt = read_reply_unix_fd(api, reply);
            (api.message_unref)(reply);
            if let Some(fd) = fd_opt {
                let file = File::from_raw_fd(fd);
                let mut buf = Vec::new();
                if file
                    .take(MAX_CLIPBOARD_READ_BYTES)
                    .read_to_end(&mut buf)
                    .is_ok()
                    && !buf.is_empty()
                {
                    return Some(String::from_utf8_lossy(&buf).into_owned());
                }
            }
        }
        None
    }
}

unsafe fn handle_selection_transfer(api: &DbusApi, session: &ActivePortalSession, serial: u32) {
    // SAFETY: Answers `SelectionTransfer` via `SelectionWrite` -> write UTF-8 -> `SelectionWriteDone`.
    unsafe {
        let mut success_val: u32 = 0;

        let write_msg = (api.message_new_method_call)(
            PORTAL_BUS_NAME.as_ptr(),
            PORTAL_OBJECT_PATH.as_ptr(),
            PORTAL_CLIP_IFACE.as_ptr(),
            c"SelectionWrite".as_ptr(),
        );
        if !write_msg.is_null() {
            let mut iter = RawDbusIter::new();
            (api.message_iter_init_append)(write_msg, iter.as_mut_ptr());
            let sess_ptr = session.session_path.as_ptr();
            let _ = (api.message_iter_append_basic)(
                iter.as_mut_ptr(),
                DBUS_TYPE_OBJECT_PATH,
                (&raw const sess_ptr).cast(),
            );
            let _ = (api.message_iter_append_basic)(
                iter.as_mut_ptr(),
                DBUS_TYPE_UINT32,
                (&raw const serial).cast(),
            );
            let mut err = RawDbusError::new();
            (api.error_init)(err.as_mut_ptr());
            let reply = (api.connection_send_with_reply_and_block)(
                session.conn,
                write_msg,
                1_500,
                err.as_mut_ptr(),
            );
            (api.message_unref)(write_msg);
            let ok = !reply.is_null() && (api.error_is_set)(err.as_mut_ptr()) == 0;
            (api.error_free)(err.as_mut_ptr());
            if ok {
                let fd_opt = read_reply_unix_fd(api, reply);
                (api.message_unref)(reply);
                if let Some(fd) = fd_opt {
                    let mut file = File::from_raw_fd(fd);
                    if file.write_all(&session.clipboard_bytes).is_ok() {
                        success_val = 1;
                    }
                    // Dropping `file` closes the pipe FD so the pasting app receives EOF.
                }
            } else if !reply.is_null() {
                (api.message_unref)(reply);
            }
        }

        let done_msg = (api.message_new_method_call)(
            PORTAL_BUS_NAME.as_ptr(),
            PORTAL_OBJECT_PATH.as_ptr(),
            PORTAL_CLIP_IFACE.as_ptr(),
            c"SelectionWriteDone".as_ptr(),
        );
        if !done_msg.is_null() {
            let mut iter = RawDbusIter::new();
            (api.message_iter_init_append)(done_msg, iter.as_mut_ptr());
            let sess_ptr = session.session_path.as_ptr();
            let _ = (api.message_iter_append_basic)(
                iter.as_mut_ptr(),
                DBUS_TYPE_OBJECT_PATH,
                (&raw const sess_ptr).cast(),
            );
            let _ = (api.message_iter_append_basic)(
                iter.as_mut_ptr(),
                DBUS_TYPE_UINT32,
                (&raw const serial).cast(),
            );
            let _ = (api.message_iter_append_basic)(
                iter.as_mut_ptr(),
                DBUS_TYPE_BOOLEAN,
                (&raw const success_val).cast(),
            );
            let mut err = RawDbusError::new();
            (api.error_init)(err.as_mut_ptr());
            let reply = (api.connection_send_with_reply_and_block)(
                session.conn,
                done_msg,
                1_500,
                err.as_mut_ptr(),
            );
            (api.message_unref)(done_msg);
            if !reply.is_null() {
                (api.message_unref)(reply);
            }
            (api.error_free)(err.as_mut_ptr());
        }
    }
}

/// Drain pending `org.freedesktop.portal.Clipboard` signals (`SelectionTransfer` and
/// `SelectionOwnerChanged`) on the active portal session.
unsafe fn pump_portal_signals(api: &DbusApi, session: &mut ActivePortalSession, wait_ms: c_int) {
    // SAFETY: `session.conn` is a valid private `DBusConnection` owned by this worker thread.
    unsafe {
        let _ = (api.connection_read_write)(session.conn, wait_ms);
        loop {
            let msg = (api.connection_pop_message)(session.conn);
            if msg.is_null() {
                break;
            }
            if (api.message_is_signal)(msg, PORTAL_CLIP_IFACE.as_ptr(), c"SelectionTransfer".as_ptr())
                != 0
            {
                // Args: (o session_handle, s mime_type, u serial)
                let mut iter = RawDbusIter::new();
                if (api.message_iter_init)(msg, iter.as_mut_ptr()) != 0
                    && (api.message_iter_get_arg_type)(iter.as_mut_ptr()) == DBUS_TYPE_OBJECT_PATH
                {
                    let mut sp: *const c_char = null();
                    (api.message_iter_get_basic)(iter.as_mut_ptr(), (&raw mut sp).cast());
                    let matches =
                        !sp.is_null() && CStr::from_ptr(sp) == session.session_path.as_c_str();
                    if matches
                        && (api.message_iter_next)(iter.as_mut_ptr()) != 0
                        && (api.message_iter_next)(iter.as_mut_ptr()) != 0
                        && (api.message_iter_get_arg_type)(iter.as_mut_ptr()) == DBUS_TYPE_UINT32
                    {
                        let mut serial: u32 = 0;
                        (api.message_iter_get_basic)(iter.as_mut_ptr(), (&raw mut serial).cast());
                        handle_selection_transfer(api, session, serial);
                    }
                }
            } else if (api.message_is_signal)(
                msg,
                PORTAL_CLIP_IFACE.as_ptr(),
                c"SelectionOwnerChanged".as_ptr(),
            ) != 0
            {
                // Args: (o session_handle, a{sv} options)
                let mut iter = RawDbusIter::new();
                if (api.message_iter_init)(msg, iter.as_mut_ptr()) != 0
                    && (api.message_iter_next)(iter.as_mut_ptr()) != 0
                    && (api.message_iter_get_arg_type)(iter.as_mut_ptr()) == DBUS_TYPE_ARRAY
                {
                    let mut arr = RawDbusIter::new();
                    (api.message_iter_recurse)(iter.as_mut_ptr(), arr.as_mut_ptr());
                    while (api.message_iter_get_arg_type)(arr.as_mut_ptr()) == DBUS_TYPE_DICT_ENTRY
                    {
                        let mut entry = RawDbusIter::new();
                        (api.message_iter_recurse)(arr.as_mut_ptr(), entry.as_mut_ptr());
                        if (api.message_iter_get_arg_type)(entry.as_mut_ptr()) == DBUS_TYPE_STRING {
                            let mut kp: *const c_char = null();
                            (api.message_iter_get_basic)(entry.as_mut_ptr(), (&raw mut kp).cast());
                            let key = if kp.is_null() {
                                ""
                            } else {
                                CStr::from_ptr(kp).to_str().unwrap_or("")
                            };
                            if key == "session_is_owner"
                                && (api.message_iter_next)(entry.as_mut_ptr()) != 0
                                && (api.message_iter_get_arg_type)(entry.as_mut_ptr())
                                    == DBUS_TYPE_VARIANT
                            {
                                let mut var = RawDbusIter::new();
                                (api.message_iter_recurse)(entry.as_mut_ptr(), var.as_mut_ptr());
                                if (api.message_iter_get_arg_type)(var.as_mut_ptr())
                                    == DBUS_TYPE_BOOLEAN
                                {
                                    let mut vb: u32 = 0;
                                    (api.message_iter_get_basic)(
                                        var.as_mut_ptr(),
                                        (&raw mut vb).cast(),
                                    );
                                    session.owns_selection = vb != 0;
                                }
                            }
                        }
                        if (api.message_iter_next)(arr.as_mut_ptr()) == 0 {
                            break;
                        }
                    }
                }
            }
            (api.message_unref)(msg);
        }
    }
}

enum OpenSessionResult {
    Ready {
        session: ActivePortalSession,
        interactive_prompt_shown: bool,
    },
    Denied(String),
    Unavailable,
}

unsafe fn open_portal_session(api: &DbusApi) -> OpenSessionResult {
    // SAFETY: Performs standard `org.freedesktop.portal.RemoteDesktop` v2 +
    // `org.freedesktop.portal.Clipboard` v1 handshake on a private session-bus connection.
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

        for rule in [
            c"type='signal',sender='org.freedesktop.portal.Desktop',interface='org.freedesktop.portal.Request',member='Response'",
            c"type='signal',sender='org.freedesktop.portal.Desktop',interface='org.freedesktop.portal.Clipboard',member='SelectionTransfer'",
            c"type='signal',sender='org.freedesktop.portal.Desktop',interface='org.freedesktop.portal.Clipboard',member='SelectionOwnerChanged'",
        ] {
            let mut merr = RawDbusError::new();
            (api.error_init)(merr.as_mut_ptr());
            (api.bus_add_match)(conn, rule.as_ptr(), merr.as_mut_ptr());
            (api.error_free)(merr.as_mut_ptr());
        }

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
        if !append_options_dict(
            api,
            &mut iter,
            &[
                ("handle_token", DictVal::Str(&cs_token)),
                ("session_handle_token", DictVal::Str(&sess_token)),
            ],
        ) {
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

        let make_bare = |sp: CString| ActivePortalSession {
            conn,
            session_path: sp,
            clipboard_enabled: false,
            owns_selection: false,
            clipboard_bytes: Vec::new(),
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
            close_portal_session(api, make_bare(session_path));
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
        if !append_options_dict(api, &mut sd_iter, &sd_opts) {
            (api.message_unref)(sd_msg);
            close_portal_session(api, make_bare(session_path));
            return OpenSessionResult::Unavailable;
        }
        let mut sd_err = RawDbusError::new();
        (api.error_init)(sd_err.as_mut_ptr());
        let sd_reply =
            (api.connection_send_with_reply_and_block)(conn, sd_msg, 5_000, sd_err.as_mut_ptr());
        (api.message_unref)(sd_msg);
        let sd_failed = sd_reply.is_null() || (api.error_is_set)(sd_err.as_mut_ptr()) != 0;
        (api.error_free)(sd_err.as_mut_ptr());
        if sd_failed {
            if !sd_reply.is_null() {
                (api.message_unref)(sd_reply);
            }
            close_portal_session(api, make_bare(session_path));
            return OpenSessionResult::Unavailable;
        }
        let sd_handle =
            read_reply_object_path(api, sd_reply).unwrap_or_else(|| expected_sd_path.clone());
        (api.message_unref)(sd_reply);

        let Some(sd_resp) = wait_portal_response(api, conn, &sd_handle, Duration::from_secs(5))
        else {
            close_portal_session(api, make_bare(session_path));
            return OpenSessionResult::Unavailable;
        };
        if sd_resp.code != 0 {
            close_portal_session(api, make_bare(session_path));
            return OpenSessionResult::Unavailable;
        }

        // 3. RequestClipboard (attaches native Wayland clipboard to this same RemoteDesktop session)
        request_portal_clipboard(api, conn, &session_path);

        // 4. Start (silent when restore_token is valid; shows one-time GNOME dialog otherwise)
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
            close_portal_session(api, make_bare(session_path));
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
        if !append_options_dict(
            api,
            &mut st_iter,
            &[("handle_token", DictVal::Str(&st_token))],
        ) {
            (api.message_unref)(st_msg);
            close_portal_session(api, make_bare(session_path));
            return OpenSessionResult::Unavailable;
        }
        let mut st_err = RawDbusError::new();
        (api.error_init)(st_err.as_mut_ptr());
        let st_reply =
            (api.connection_send_with_reply_and_block)(conn, st_msg, 5_000, st_err.as_mut_ptr());
        (api.message_unref)(st_msg);
        let st_failed = st_reply.is_null() || (api.error_is_set)(st_err.as_mut_ptr()) != 0;
        (api.error_free)(st_err.as_mut_ptr());
        if st_failed {
            if !st_reply.is_null() {
                (api.message_unref)(st_reply);
            }
            close_portal_session(api, make_bare(session_path));
            return OpenSessionResult::Unavailable;
        }
        let st_handle =
            read_reply_object_path(api, st_reply).unwrap_or_else(|| expected_st_path.clone());
        (api.message_unref)(st_reply);

        let Some(st_resp) = wait_portal_response(api, conn, &st_handle, Duration::from_secs(60))
        else {
            close_portal_session(api, make_bare(session_path));
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
            close_portal_session(api, make_bare(session_path));
            return OpenSessionResult::Denied(
                "Remote Interaction permission wasn't granted — click Copy or allow Remote Interaction to auto-paste."
                    .to_owned(),
            );
        }

        if let Some(devices) = st_resp.devices
            && (devices & DEVICE_KEYBOARD) == 0
        {
            close_portal_session(api, make_bare(session_path));
            return OpenSessionResult::Denied(
                "Keyboard access wasn't enabled in Remote Interaction — click Copy or allow keyboard sharing."
                    .to_owned(),
            );
        }

        if let Some(new_token) = st_resp.restore_token.as_deref() {
            save_restore_token(new_token);
        }

        PORTAL_CLIPBOARD_ACTIVE.store(st_resp.clipboard_enabled, Ordering::Release);
        let interactive_prompt_shown = !had_restore_token || elapsed >= Duration::from_millis(350);
        OpenSessionResult::Ready {
            session: ActivePortalSession {
                conn,
                session_path,
                clipboard_enabled: st_resp.clipboard_enabled,
                owns_selection: false,
                clipboard_bytes: Vec::new(),
            },
            interactive_prompt_shown,
        }
    }
}

unsafe fn notify_keycode(
    api: &DbusApi,
    session: &ActivePortalSession,
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

unsafe fn notify_keysym(
    api: &DbusApi,
    session: &ActivePortalSession,
    keysym: i32,
    state: u32,
) -> bool {
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

unsafe fn do_type_keysyms(api: &DbusApi, session: &ActivePortalSession, text: &str) -> bool {
    // SAFETY: Streams XKB/Unicode keysyms on an active `RemoteDesktop` session.
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
}

unsafe fn do_ctrl_v_with_pump(api: &DbusApi, session: &mut ActivePortalSession) -> bool {
    // SAFETY: Emits evdev Ctrl+V while continuously serving any Wayland Clipboard `SelectionTransfer`.
    unsafe {
        let ok_down = notify_keycode(api, session, EVDEV_KEY_LEFTCTRL, 1)
            && notify_keycode(api, session, EVDEV_KEY_V, 1);
        if ok_down {
            pump_portal_signals(api, session, 15);
        }
        let ok_up = notify_keycode(api, session, EVDEV_KEY_V, 0)
            && notify_keycode(api, session, EVDEV_KEY_LEFTCTRL, 0);
        if ok_down && ok_up && session.clipboard_enabled {
            // Serve the pasting app's clipboard read immediately while focus is on the target app.
            let deadline = Instant::now() + Duration::from_millis(60);
            while Instant::now() < deadline {
                pump_portal_signals(api, session, 5);
            }
        }
        ok_down && ok_up
    }
}

unsafe fn run_with_session(
    api: &DbusApi,
    slot: &mut Option<ActivePortalSession>,
    portal_unavailable: &mut bool,
    op: impl Fn(&DbusApi, &mut ActivePortalSession) -> bool,
) -> PortalOutcome {
    if *portal_unavailable {
        return PortalOutcome::Unavailable;
    }

    if let Some(sess) = slot.as_mut() {
        if op(api, sess) {
            return PortalOutcome::Injected;
        }
        if let Some(stale) = slot.take() {
            // SAFETY: Closes expired/revoked portal session before retrying once.
            unsafe { close_portal_session(api, stale) };
        }
    }

    // SAFETY: Opens a fresh persistent `RemoteDesktop` + `Clipboard` portal session.
    match unsafe { open_portal_session(api) } {
        OpenSessionResult::Ready {
            mut session,
            interactive_prompt_shown,
        } => {
            if interactive_prompt_shown {
                // Give GNOME/Mutter time to dismiss the modal dialog and return focus to the editor.
                thread::sleep(Duration::from_millis(260));
            }
            if op(api, &mut session) {
                *slot = Some(session);
                PortalOutcome::Injected
            } else {
                // SAFETY: Closes session that rejected injection.
                unsafe { close_portal_session(api, session) };
                PortalOutcome::Denied(
                    "Wayland desktop rejected synthetic keyboard input — click Copy to paste manually."
                        .to_owned(),
                )
            }
        }
        OpenSessionResult::Denied(msg) => PortalOutcome::Denied(msg),
        OpenSessionResult::Unavailable => {
            *portal_unavailable = true;
            PortalOutcome::Unavailable
        }
    }
}

fn run_portal_worker(api: &'static DbusApi, rx: mpsc::Receiver<PortalCmd>) {
    let mut session: Option<ActivePortalSession> = None;
    let mut portal_unavailable = false;

    loop {
        let cmd = if let Some(sess) = session.as_mut() {
            if sess.clipboard_enabled {
                // SAFETY: Continuously answers `SelectionTransfer` signals while we own clipboard.
                unsafe { pump_portal_signals(api, sess, 8) };
            }
            match rx.recv_timeout(Duration::from_millis(8)) {
                Ok(c) => c,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(c) => c,
                Err(_) => break,
            }
        };

        match cmd {
            PortalCmd::TrySetClipboard { payload, ack } => {
                let ok = if let Some(sess) = session.as_mut() {
                    // SAFETY: Updates native Wayland clipboard via active `Clipboard` portal session.
                    unsafe { portal_set_selection(api, sess, payload) }
                } else {
                    false
                };
                let _ = ack.send(ok);
            }
            PortalCmd::TryReadClipboard { ack } => {
                let res = if let Some(sess) = session.as_ref() {
                    // SAFETY: Reads native Wayland clipboard via active `Clipboard` portal session.
                    unsafe { portal_read_selection(api, sess) }
                } else {
                    None
                };
                let _ = ack.send(res);
            }
            PortalCmd::PasteOrType { text, ack } => {
                // SAFETY: Dispatches native Wayland clipboard+Ctrl+V (when clipboard enabled) or
                // direct `NotifyKeyboardKeysym` typing (when clipboard switch is off / unavailable).
                let outcome = unsafe {
                    run_with_session(api, &mut session, &mut portal_unavailable, |api, sess| {
                        match text.as_deref() {
                            Some(payload) if sess.clipboard_enabled => {
                                if portal_set_selection(api, sess, payload.as_bytes().to_vec()) {
                                    pump_portal_signals(api, sess, 20);
                                    do_ctrl_v_with_pump(api, sess)
                                } else {
                                    do_type_keysyms(api, sess, payload)
                                }
                            }
                            Some(payload) => do_type_keysyms(api, sess, payload),
                            None => do_ctrl_v_with_pump(api, sess),
                        }
                    })
                };
                let _ = ack.send(outcome);
            }
            PortalCmd::TypeUnicode { text, ack } => {
                // SAFETY: Streams Unicode keysyms directly into the focused Wayland window.
                let outcome = unsafe {
                    run_with_session(api, &mut session, &mut portal_unavailable, |api, sess| {
                        do_type_keysyms(api, sess, &text)
                    })
                };
                let _ = ack.send(outcome);
            }
            PortalCmd::SendBackspaces { count, ack } => {
                // SAFETY: Sends Backspace keycodes through the active `RemoteDesktop` session.
                let outcome = unsafe {
                    run_with_session(api, &mut session, &mut portal_unavailable, |api, sess| {
                        for _ in 0..count {
                            if !notify_keycode(api, sess, EVDEV_KEY_BACKSPACE, 1)
                                || !notify_keycode(api, sess, EVDEV_KEY_BACKSPACE, 0)
                            {
                                return false;
                            }
                        }
                        true
                    })
                };
                let _ = ack.send(outcome);
            }
        }
    }

    if let Some(sess) = session.take() {
        // SAFETY: Clean session teardown on exit.
        unsafe { close_portal_session(api, sess) };
    }
}

/// Return `true` when an active Wayland `Clipboard` portal session is currently open.
pub(crate) fn is_portal_clipboard_active() -> bool {
    PORTAL_CLIPBOARD_ACTIVE.load(Ordering::Acquire)
}

/// Non-prompting write to the native Wayland clipboard if a `Clipboard` portal session is open.
pub(crate) fn try_set_clipboard_text(text: &str) -> bool {
    if !is_portal_clipboard_active() {
        return false;
    }
    let Some(client) = ensure_portal_client() else {
        return false;
    };
    let (ack_tx, ack_rx) = mpsc::sync_channel::<bool>(1);
    if client
        .tx
        .send(PortalCmd::TrySetClipboard {
            payload: text.as_bytes().to_vec(),
            ack: ack_tx,
        })
        .is_err()
    {
        return false;
    }
    ack_rx.recv_timeout(Duration::from_secs(2)).unwrap_or(false)
}

/// Non-prompting read from the native Wayland clipboard if a `Clipboard` portal session is open.
pub(crate) fn try_read_clipboard_text() -> Option<String> {
    if !is_portal_clipboard_active() {
        return None;
    }
    let client = ensure_portal_client()?;
    let (ack_tx, ack_rx) = mpsc::sync_channel::<Option<String>>(1);
    client
        .tx
        .send(PortalCmd::TryReadClipboard { ack: ack_tx })
        .ok()?;
    ack_rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
}

/// Paste `pending_text` via native Wayland `Clipboard` portal + `Ctrl+V` when clipboard access is
/// granted, or type `pending_text` directly via `NotifyKeyboardKeysym` when clipboard access is
/// disabled/unavailable.
pub(crate) fn send_paste_or_type(pending_text: Option<String>) -> PortalOutcome {
    let Some(client) = ensure_portal_client() else {
        return PortalOutcome::Unavailable;
    };
    let (ack_tx, ack_rx) = mpsc::sync_channel::<PortalOutcome>(1);
    if client
        .tx
        .send(PortalCmd::PasteOrType {
            text: pending_text,
            ack: ack_tx,
        })
        .is_err()
    {
        return PortalOutcome::Unavailable;
    }
    ack_rx
        .recv_timeout(Duration::from_secs(65))
        .unwrap_or(PortalOutcome::Unavailable)
}

/// Send `count` Backspace presses through the persistent Wayland `RemoteDesktop` portal.
pub(crate) fn send_backspaces(count: usize) -> PortalOutcome {
    let Some(client) = ensure_portal_client() else {
        return PortalOutcome::Unavailable;
    };
    let (ack_tx, ack_rx) = mpsc::sync_channel::<PortalOutcome>(1);
    if client
        .tx
        .send(PortalCmd::SendBackspaces { count, ack: ack_tx })
        .is_err()
    {
        return PortalOutcome::Unavailable;
    }
    ack_rx
        .recv_timeout(Duration::from_secs(65))
        .unwrap_or(PortalOutcome::Unavailable)
}

/// Type arbitrary Unicode text via `RemoteDesktop.NotifyKeyboardKeysym`.
pub(crate) fn type_unicode_text(text: &str) -> PortalOutcome {
    let Some(client) = ensure_portal_client() else {
        return PortalOutcome::Unavailable;
    };
    let (ack_tx, ack_rx) = mpsc::sync_channel::<PortalOutcome>(1);
    if client
        .tx
        .send(PortalCmd::TypeUnicode {
            text: text.to_owned(),
            ack: ack_tx,
        })
        .is_err()
    {
        return PortalOutcome::Unavailable;
    }
    ack_rx
        .recv_timeout(Duration::from_secs(65))
        .unwrap_or(PortalOutcome::Unavailable)
}
