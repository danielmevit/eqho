//! Keyboard paste, backspace, modifier release, and Unicode typing on Linux (X11 XTest + Wayland tools).

use std::ffi::{c_char, c_uint, c_ulong, c_void};
use std::process::{Command, Stdio};
use std::ptr::null;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use eqho_platform::{InputInjector, ModifierKey, PlatformError};

use crate::pure::is_wayland_session;
use crate::x11_ffi::{CURRENT_TIME, X11Api};

/// Persistent X11 display connection used exclusively for `XTestFakeKeyEvent`.
///
/// Keeping one connection open across dictations is critical on GNOME/Mutter Wayland: XWayland
/// grants EIS/Remote-Interaction permission per X11 client connection, so opening and closing a
/// fresh `Display*` on every dictation would re-trigger the desktop permission popup every time.
struct PersistentXTestDisplay(*mut c_void);

// SAFETY: Access is serialized through `XTEST_DISPLAY`'s `Mutex`, so only one thread uses the
// `Display*` at a time.
unsafe impl Send for PersistentXTestDisplay {}

static XTEST_DISPLAY: OnceLock<Mutex<Option<PersistentXTestDisplay>>> = OnceLock::new();

fn with_xtest_display<R>(
    api: &'static X11Api,
    f: impl FnOnce(*mut c_void) -> R,
) -> Option<R> {
    let lock = XTEST_DISPLAY.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().ok()?;
    if guard.is_none() {
        // SAFETY: Opens a dedicated X11 display connection serialized by `XTEST_DISPLAY`.
        let dpy = unsafe { (api.open_display)(null()) };
        if dpy.is_null() {
            return None;
        }
        *guard = Some(PersistentXTestDisplay(dpy));
    }
    let dpy = guard.as_ref()?.0;
    Some(f(dpy))
}

const XK_SHIFT_L: c_ulong = 0xFFE1;
const XK_SHIFT_R: c_ulong = 0xFFE2;
const XK_CONTROL_L: c_ulong = 0xFFE3;
const XK_CONTROL_R: c_ulong = 0xFFE4;
const XK_ALT_L: c_ulong = 0xFFE9;
const XK_ALT_R: c_ulong = 0xFFEA;
const XK_SUPER_L: c_ulong = 0xFFEB;
const XK_SUPER_R: c_ulong = 0xFFEC;
const XK_BACKSPACE: c_ulong = 0xFF08;
const XK_RETURN: c_ulong = 0xFF0D;
const XK_TAB: c_ulong = 0xFF09;

/// Linux input injector: native X11 XTest (zero external tools) with Wayland `wtype`/`ydotool` support.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxInputInjector;

impl LinuxInputInjector {
    fn is_wayland() -> bool {
        let wl = std::env::var("WAYLAND_DISPLAY").ok();
        let st = std::env::var("XDG_SESSION_TYPE").ok();
        is_wayland_session(wl.as_deref(), st.as_deref())
    }

    fn modifier_keysym(m: ModifierKey) -> c_ulong {
        match m {
            ModifierKey::CtrlLeft => XK_CONTROL_L,
            ModifierKey::CtrlRight => XK_CONTROL_R,
            ModifierKey::ShiftLeft => XK_SHIFT_L,
            ModifierKey::ShiftRight => XK_SHIFT_R,
            ModifierKey::AltLeft => XK_ALT_L,
            ModifierKey::AltRight => XK_ALT_R,
            ModifierKey::WinLeft => XK_SUPER_L,
            ModifierKey::WinRight => XK_SUPER_R,
        }
    }

    fn run_ok(bin: &str, args: &[&str]) -> bool {
        Command::new(bin)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    unsafe fn xtest_tap_keycode(
        api: &X11Api,
        fake: unsafe extern "C" fn(*mut std::ffi::c_void, c_uint, i32, c_ulong) -> i32,
        dpy: *mut std::ffi::c_void,
        kc: u8,
        shift: bool,
    ) {
        if kc == 0 {
            return;
        }
        // SAFETY: Valid X11 display and `XTestFakeKeyEvent` function pointer.
        unsafe {
            let shift_kc = if shift {
                (api.keysym_to_keycode)(dpy, XK_SHIFT_L)
            } else {
                0
            };
            if shift_kc != 0 {
                fake(dpy, u32::from(shift_kc), 1, CURRENT_TIME);
            }
            fake(dpy, u32::from(kc), 1, CURRENT_TIME);
            fake(dpy, u32::from(kc), 0, CURRENT_TIME);
            if shift_kc != 0 {
                fake(dpy, u32::from(shift_kc), 0, CURRENT_TIME);
            }
        }
    }
}

impl InputInjector for LinuxInputInjector {
    fn held_modifiers(&self) -> Option<Vec<ModifierKey>> {
        let api = X11Api::load()?;
        // SAFETY: Queries the live 32-byte X11 keymap bitmask.
        unsafe {
            let dpy = (api.open_display)(null());
            if dpy.is_null() {
                return None;
            }
            let mut keymap = [0 as c_char; 32];
            (api.query_keymap)(dpy, keymap.as_mut_ptr());
            let mut held = Vec::new();
            for m in [
                ModifierKey::CtrlLeft,
                ModifierKey::CtrlRight,
                ModifierKey::ShiftLeft,
                ModifierKey::ShiftRight,
                ModifierKey::AltLeft,
                ModifierKey::AltRight,
                ModifierKey::WinLeft,
                ModifierKey::WinRight,
            ] {
                let kc = (api.keysym_to_keycode)(dpy, Self::modifier_keysym(m)) as usize;
                if kc > 0 && kc < 256 {
                    let byte = keymap[kc / 8] as u8;
                    if (byte & (1 << (kc % 8))) != 0 {
                        held.push(m);
                    }
                }
            }
            (api.close_display)(dpy);
            Some(held)
        }
    }

    fn release_modifiers(&self, mods: &[ModifierKey]) -> Result<(), PlatformError> {
        if mods.is_empty() {
            return Ok(());
        }
        let Some(api) = X11Api::load() else {
            return Ok(());
        };
        let Some(fake) = api.xtest_fake_key_event else {
            return Ok(());
        };
        // SAFETY: Emitting key-up events only for explicitly held modifiers on our persistent XTest display.
        let _ = with_xtest_display(api, |dpy| unsafe {
            for &m in mods {
                let kc = (api.keysym_to_keycode)(dpy, Self::modifier_keysym(m));
                if kc != 0 {
                    fake(dpy, u32::from(kc), 0, CURRENT_TIME);
                }
            }
            (api.flush)(dpy);
        });
        Ok(())
    }

    fn send_paste_shortcut(&self) -> Result<(), PlatformError> {
        if Self::is_wayland() {
            if Self::run_ok("wtype", &["-M", "ctrl", "-k", "v", "-m", "ctrl"]) {
                return Ok(());
            }
            // Linux evdev keycodes: KEY_LEFTCTRL=29, KEY_V=47.
            if Self::run_ok("ydotool", &["key", "29:1", "47:1", "47:0", "29:0"]) {
                return Ok(());
            }
        }
        if let Some(api) = X11Api::load()
            && let Some(fake) = api.xtest_fake_key_event
        {
            // SAFETY: Synthetic `Ctrl+V` via persistent X11 XTest connection.
            let sent = with_xtest_display(api, |dpy| unsafe {
                let ctrl_kc = (api.keysym_to_keycode)(dpy, XK_CONTROL_L);
                let v_kc = (api.keysym_to_keycode)(dpy, u64::from(b'v') as c_ulong);
                if ctrl_kc != 0 && v_kc != 0 {
                    fake(dpy, u32::from(ctrl_kc), 1, CURRENT_TIME);
                    fake(dpy, u32::from(v_kc), 1, CURRENT_TIME);
                    fake(dpy, u32::from(v_kc), 0, CURRENT_TIME);
                    fake(dpy, u32::from(ctrl_kc), 0, CURRENT_TIME);
                    (api.flush)(dpy);
                    true
                } else {
                    false
                }
            });
            if sent == Some(true) {
                return Ok(());
            }
        }
        if Self::run_ok("xdotool", &["key", "--clearmodifiers", "ctrl+v"]) {
            return Ok(());
        }
        Err(PlatformError(
            "could not synthesize Ctrl+V (install xdotool on X11, or wtype/ydotool on Wayland)"
                .to_owned(),
        ))
    }

    fn type_unicode_text(&self, text: &str) -> Result<(), PlatformError> {
        if text.is_empty() {
            return Ok(());
        }
        if Self::is_wayland() {
            if Self::run_ok("wtype", &["--", text]) {
                return Ok(());
            }
            if Self::run_ok("ydotool", &["type", "--", text]) {
                return Ok(());
            }
        }
        // Prefer xdotool for non-ASCII Unicode (handles arbitrary UTF-8 cleanly on X11).
        let is_pure_ascii = text.bytes().all(|b| b.is_ascii());
        if !is_pure_ascii
            && Self::run_ok("xdotool", &["type", "--clearmodifiers", "--delay", "2", "--", text])
        {
            return Ok(());
        }
        if let Some(api) = X11Api::load()
            && let Some(fake) = api.xtest_fake_key_event
        {
            // SAFETY: Synthetic ASCII keystrokes via persistent X11 XTest connection.
            let typed = with_xtest_display(api, |dpy| unsafe {
                for ch in text.chars() {
                    let (ks, shift) = match ch {
                        '\n' | '\r' => (XK_RETURN, false),
                        '\t' => (XK_TAB, false),
                        c if c.is_ascii_uppercase() => {
                            (u64::from(c.to_ascii_lowercase() as u8) as c_ulong, true)
                        }
                        '!' => (u64::from(b'1') as c_ulong, true),
                        '@' => (u64::from(b'2') as c_ulong, true),
                        '#' => (u64::from(b'3') as c_ulong, true),
                        '$' => (u64::from(b'4') as c_ulong, true),
                        '%' => (u64::from(b'5') as c_ulong, true),
                        '^' => (u64::from(b'6') as c_ulong, true),
                        '&' => (u64::from(b'7') as c_ulong, true),
                        '*' => (u64::from(b'8') as c_ulong, true),
                        '(' => (u64::from(b'9') as c_ulong, true),
                        ')' => (u64::from(b'0') as c_ulong, true),
                        '_' => (u64::from(b'-') as c_ulong, true),
                        '+' => (u64::from(b'=') as c_ulong, true),
                        '{' => (u64::from(b'[') as c_ulong, true),
                        '}' => (u64::from(b']') as c_ulong, true),
                        '|' => (u64::from(b'\\') as c_ulong, true),
                        ':' => (u64::from(b';') as c_ulong, true),
                        '"' => (u64::from(b'\'') as c_ulong, true),
                        '<' => (u64::from(b',') as c_ulong, true),
                        '>' => (u64::from(b'.') as c_ulong, true),
                        '?' => (u64::from(b'/') as c_ulong, true),
                        '~' => (u64::from(b'`') as c_ulong, true),
                        c if c.is_ascii() => (u64::from(c as u8) as c_ulong, false),
                        _ => continue,
                    };
                    let kc = (api.keysym_to_keycode)(dpy, ks);
                    Self::xtest_tap_keycode(api, fake, dpy, kc, shift);
                }
                (api.flush)(dpy);
            });
            if typed.is_some() {
                return Ok(());
            }
        }
        if Self::run_ok("xdotool", &["type", "--clearmodifiers", "--delay", "2", "--", text]) {
            return Ok(());
        }
        Err(PlatformError(
            "could not type text on Linux (install xdotool on X11, or wtype/ydotool on Wayland)"
                .to_owned(),
        ))
    }

    fn send_backspaces(&self, count: usize) -> Result<(), PlatformError> {
        if count == 0 {
            return Ok(());
        }
        if Self::is_wayland() {
            let mut args: Vec<&str> = Vec::with_capacity(count * 2);
            for _ in 0..count {
                args.push("-k");
                args.push("BackSpace");
            }
            if Self::run_ok("wtype", &args) {
                return Ok(());
            }
        }
        if let Some(api) = X11Api::load()
            && let Some(fake) = api.xtest_fake_key_event
        {
            // SAFETY: Sending Backspace keystrokes via persistent X11 XTest connection.
            let sent = with_xtest_display(api, |dpy| unsafe {
                let kc = (api.keysym_to_keycode)(dpy, XK_BACKSPACE);
                for _ in 0..count {
                    Self::xtest_tap_keycode(api, fake, dpy, kc, false);
                }
                (api.flush)(dpy);
            });
            if sent.is_some() {
                thread::sleep(Duration::from_millis(5));
                return Ok(());
            }
        }
        Err(PlatformError("could not send Backspace keystrokes".to_owned()))
    }
}
