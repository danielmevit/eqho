//! Global X11 hotkey listener (`XGrabKey`) supporting Toggle and Hold-to-Talk modes.

use std::ffi::{c_int, c_uint, c_ulong, c_void};
use std::ptr::null;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_platform::{
    HotkeyCallback, HotkeyEvent, HotkeyMode, HotkeyRegistration, HotkeyService, HotkeySpec,
    PlatformError,
};

use crate::pure::parse_hotkey_combo;
use crate::x11_ffi::{
    GRAB_MODE_ASYNC, KEY_PRESS, KEY_RELEASE, LOCK_MASK, MOD2_MASK, X11Api, XEvent,
};

const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(400);

static X11_GRAB_ERROR: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn grab_error_handler(_dpy: *mut c_void, _err: *mut c_void) -> c_int {
    X11_GRAB_ERROR.store(true, Ordering::Release);
    0
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

/// X11 global hotkey service via `XGrabKey` (handles CapsLock & NumLock permutations cleanly).
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxHotkeyService;

impl HotkeyService for LinuxHotkeyService {
    fn register(
        &self,
        spec: &HotkeySpec,
        callback: HotkeyCallback,
    ) -> Result<Box<dyn HotkeyRegistration>, PlatformError> {
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
