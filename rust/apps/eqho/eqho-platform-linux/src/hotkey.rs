//! Global Linux hotkey listener:
//! - On GNOME Wayland: automatically registers & maintains a native GNOME Custom Shortcut
//!   (`org.gnome.settings-daemon.plugins.media-keys`) invoking `<current_exe> --toggle` so
//!   `Alt+Q` works globally across every Wayland and XWayland app out of the box with zero
//!   manual Ubuntu Settings configuration, and cleans up on unregister/quit.
//! - On X11 (and non-GNOME Wayland with XWayland): native `XGrabKey` supporting both Toggle
//!   and Hold-to-Talk modes (with CapsLock & NumLock permutations).

use std::ffi::{c_int, c_uint, c_ulong, c_void};
use std::process::{Command, Stdio};
use std::ptr::null;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_platform::{
    HotkeyCallback, HotkeyEvent, HotkeyMode, HotkeyRegistration, HotkeyService, HotkeySpec,
    PlatformError,
};

use crate::pure::{
    add_gsettings_keybinding_path, build_gnome_toggle_command, hotkey_to_gtk_accelerator,
    is_wayland_session, parse_hotkey_combo, remove_gsettings_keybinding_path,
};
use crate::x11_ffi::{
    GRAB_MODE_ASYNC, KEY_PRESS, KEY_RELEASE, LOCK_MASK, MOD2_MASK, X11Api, XEvent,
};

const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(400);
const GNOME_MEDIA_KEYS_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const GNOME_CUSTOM_BINDING_SCHEMA: &str =
    "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const GNOME_EQHO_SLOT: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/eqho/";

static X11_GRAB_ERROR: AtomicBool = AtomicBool::new(false);
static GNOME_SHORTCUT_GEN: AtomicU64 = AtomicU64::new(0);
static GNOME_SHORTCUT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn gnome_lock() -> &'static Mutex<()> {
    GNOME_SHORTCUT_LOCK.get_or_init(|| Mutex::new(()))
}

unsafe extern "C" fn grab_error_handler(_dpy: *mut c_void, _err: *mut c_void) -> c_int {
    X11_GRAB_ERROR.store(true, Ordering::Release);
    0
}

fn is_wayland() -> bool {
    let wl = std::env::var("WAYLAND_DISPLAY").ok();
    let st = std::env::var("XDG_SESSION_TYPE").ok();
    is_wayland_session(wl.as_deref(), st.as_deref())
}

fn gsettings_get_custom_keybindings() -> Option<String> {
    let out = Command::new("gsettings")
        .args(["get", GNOME_MEDIA_KEYS_SCHEMA, "custom-keybindings"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn gsettings_set(schema: &str, key: &str, value: &str) -> bool {
    Command::new("gsettings")
        .args(["set", schema, key, value])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

struct GnomeWaylandRegistration {
    gen_id: u64,
}

impl HotkeyRegistration for GnomeWaylandRegistration {}

impl Drop for GnomeWaylandRegistration {
    fn drop(&mut self) {
        let _guard = gnome_lock().lock().unwrap_or_else(|e| e.into_inner());
        // Only remove the GNOME shortcut if no newer registration has superseded us (guards
        // against `*hotkey_reg = Some(new_reg)` dropping the old registration after setting new).
        if GNOME_SHORTCUT_GEN.load(Ordering::Acquire) != self.gen_id {
            return;
        }
        if let Some(current) = gsettings_get_custom_keybindings() {
            let updated = remove_gsettings_keybinding_path(&current, GNOME_EQHO_SLOT);
            let _ = gsettings_set(GNOME_MEDIA_KEYS_SCHEMA, "custom-keybindings", &updated);
        }
    }
}

fn try_register_gnome_wayland(spec: &HotkeySpec) -> Option<Result<Box<dyn HotkeyRegistration>, PlatformError>> {
    if !is_wayland() {
        return None;
    }
    let current = gsettings_get_custom_keybindings()?;
    let accel = match hotkey_to_gtk_accelerator(&spec.raw) {
        Ok(a) => a,
        Err(err) => return Some(Err(err)),
    };
    let exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => return None,
    };
    let cfg_env = std::env::var("EQHO_CONFIG_DIR").ok();
    let cmd = build_gnome_toggle_command(&exe, cfg_env.as_deref());
    let binding_schema = format!("{GNOME_CUSTOM_BINDING_SCHEMA}:{GNOME_EQHO_SLOT}");

    let _guard = gnome_lock().lock().unwrap_or_else(|e| e.into_inner());
    let updated_list = add_gsettings_keybinding_path(&current, GNOME_EQHO_SLOT);

    let ok = gsettings_set(&binding_schema, "name", "Eqho Dictation")
        && gsettings_set(&binding_schema, "command", &cmd)
        && gsettings_set(&binding_schema, "binding", &accel)
        && gsettings_set(GNOME_MEDIA_KEYS_SCHEMA, "custom-keybindings", &updated_list);

    if ok {
        let gen_id = GNOME_SHORTCUT_GEN.fetch_add(1, Ordering::AcqRel) + 1;
        Some(Ok(Box::new(GnomeWaylandRegistration { gen_id })))
    } else {
        None
    }
}

struct LinuxHotkeyRegistration {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl HotkeyRegistration for LinuxHotkeyRegistration {}

impl Drop for LinuxHotkeyRegistration {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Global Linux hotkey service (automatic GNOME Wayland `gsettings` shortcut + X11 `XGrabKey`).
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxHotkeyService;

impl HotkeyService for LinuxHotkeyService {
    fn register(
        &self,
        spec: &HotkeySpec,
        callback: HotkeyCallback,
    ) -> Result<Box<dyn HotkeyRegistration>, PlatformError> {
        // On GNOME Wayland, XWayland's `XGrabKey` cannot intercept keystrokes while native
        // Wayland windows have focus. Automatically register a GNOME Custom Shortcut pointing at
        // `<current_exe> --toggle` so the hotkey works globally out of the box.
        if let Some(gnome_res) = try_register_gnome_wayland(spec) {
            return gnome_res;
        }

        let parsed = parse_hotkey_combo(&spec.raw)?;
        let mode = spec.mode;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = Arc::clone(&stop);
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), PlatformError>>(1);

        let handle = thread::Builder::new()
            .name("eqho-hotkey-x11".to_owned())
            .spawn(move || {
                let Some(api) = X11Api::load() else {
                    let _ = ready_tx.send(Err(PlatformError(
                        "X11 display unavailable for global hotkey (on pure Wayland, bind `Eqho --toggle` in desktop shortcuts)".to_owned(),
                    )));
                    return;
                };
                // SAFETY: Dedicated X11 connection owned exclusively by this hotkey thread.
                unsafe {
                    let dpy = (api.open_display)(null());
                    if dpy.is_null() {
                        let _ = ready_tx.send(Err(PlatformError(
                            "XOpenDisplay failed for global hotkey".to_owned(),
                        )));
                        return;
                    }
                    let root = (api.default_root_window)(dpy);
                    let keycode = (api.keysym_to_keycode)(dpy, c_ulong::from(parsed.keysym));
                    if keycode == 0 {
                        (api.close_display)(dpy);
                        let _ = ready_tx.send(Err(PlatformError(format!(
                            "no X11 keycode for keysym 0x{:x}",
                            parsed.keysym
                        ))));
                        return;
                    }

                    let base_mask = parsed.modifiers.x11_mask();
                    let masks: [c_uint; 4] = [
                        base_mask,
                        base_mask | LOCK_MASK,
                        base_mask | MOD2_MASK,
                        base_mask | LOCK_MASK | MOD2_MASK,
                    ];

                    X11_GRAB_ERROR.store(false, Ordering::Release);
                    let prev_handler = (api.set_error_handler)(Some(grab_error_handler));
                    for &m in &masks {
                        (api.grab_key)(
                            dpy,
                            c_int::from(keycode),
                            m,
                            root,
                            0,
                            GRAB_MODE_ASYNC,
                            GRAB_MODE_ASYNC,
                        );
                    }
                    (api.sync)(dpy, 0);
                    let grab_failed = X11_GRAB_ERROR.load(Ordering::Acquire);
                    (api.set_error_handler)(prev_handler);

                    if grab_failed {
                        for &m in &masks {
                            (api.ungrab_key)(dpy, c_int::from(keycode), m, root);
                        }
                        (api.close_display)(dpy);
                        let _ = ready_tx.send(Err(PlatformError(
                            "hotkey is already grabbed by another application or window manager"
                                .to_owned(),
                        )));
                        return;
                    }

                    let _ = ready_tx.send(Ok(()));

                    let mut last_toggle: Option<Instant> = None;
                    let mut hold_active = false;
                    let mut ev: XEvent = std::mem::zeroed();

                    while !stop_bg.load(Ordering::Acquire) {
                        if (api.pending)(dpy) == 0 {
                            thread::sleep(Duration::from_millis(12));
                            continue;
                        }
                        (api.next_event)(dpy, &mut ev);
                        if ev.r#type == KEY_PRESS && ev.xkey.keycode == c_uint::from(keycode) {
                            match mode {
                                HotkeyMode::Toggle => {
                                    let now = Instant::now();
                                    let debounced = last_toggle.is_some_and(|p| {
                                        now.duration_since(p) < TOGGLE_DEBOUNCE
                                    });
                                    if !debounced {
                                        last_toggle = Some(now);
                                        callback(HotkeyEvent::Toggle);
                                    }
                                }
                                HotkeyMode::Hold => {
                                    if !hold_active {
                                        hold_active = true;
                                        callback(HotkeyEvent::HoldPressed);
                                    }
                                }
                            }
                        } else if ev.r#type == KEY_RELEASE
                            && ev.xkey.keycode == c_uint::from(keycode)
                            && mode == HotkeyMode::Hold
                            && hold_active
                        {
                            // Filter synthetic X11 key auto-repeat pairs (Release immediately
                            // followed by Press with identical timestamp and keycode).
                            let mut is_repeat = false;
                            if (api.pending)(dpy) > 0 {
                                let mut next: XEvent = std::mem::zeroed();
                                (api.peek_event)(dpy, &mut next);
                                if next.r#type == KEY_PRESS
                                    && next.xkey.keycode == ev.xkey.keycode
                                    && next.xkey.time == ev.xkey.time
                                {
                                    is_repeat = true;
                                }
                            }
                            if !is_repeat {
                                hold_active = false;
                                callback(HotkeyEvent::HoldReleased);
                            }
                        }
                    }

                    for &m in &masks {
                        (api.ungrab_key)(dpy, c_int::from(keycode), m, root);
                    }
                    (api.flush)(dpy);
                    (api.close_display)(dpy);
                }
            })
            .map_err(|e| PlatformError(format!("failed to spawn X11 hotkey thread: {e}")))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Box::new(LinuxHotkeyRegistration {
                stop,
                thread: Some(handle),
            })),
            Ok(Err(err)) => {
                let _ = handle.join();
                Err(err)
            }
            Err(_) => {
                let _ = handle.join();
                Err(PlatformError("X11 hotkey thread exited unexpectedly".to_owned()))
            }
        }
    }
}
