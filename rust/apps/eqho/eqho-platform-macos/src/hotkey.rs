//! macOS global hotkey listener (`CGEventTapCreate`) supporting Toggle and Hold-to-Talk.
//!
//! Swallows matched hotkey chords by returning `NULL` from the event tap callback so `Option+Q`
//! never types a stray character into focused text fields (including Onboarding Step 6).

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_platform::{
    HotkeyCallback, HotkeyEvent, HotkeyMode, HotkeyRegistration, HotkeyService, HotkeySpec,
    PlatformError,
};

use crate::pure::{ParsedMacHotkey, parse_hotkey_combo};

const K_CG_SESSION_EVENT_TAP: u32 = 1;
const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const K_CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;

const K_CG_EVENT_KEY_DOWN: u32 = 10;
const K_CG_EVENT_KEY_UP: u32 = 11;
const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
const K_CG_KEYBOARD_EVENT_AUTOREPEAT: u32 = 8;

const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(400);

type CGEventTapCallBack =
    unsafe extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetIntegerValueField(event: *const c_void, field: u32) -> i64;
    fn CGEventGetFlags(event: *const c_void) -> u64;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: *const c_void;
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: *mut c_void,
        order: isize,
    ) -> *mut c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRun();
    fn CFRunLoopStop(rl: *mut c_void);
    fn CFRetain(cf: *const c_void) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

struct TapContext {
    spec: ParsedMacHotkey,
    mode: HotkeyMode,
    tap_port: *mut c_void,
    event_tx: mpsc::Sender<HotkeyEvent>,
    last_toggle: Option<Instant>,
    hold_active: bool,
    suppressed_kc: Option<u16>,
}

unsafe extern "C" fn event_tap_proc(
    _proxy: *mut c_void,
    event_type: u32,
    event: *mut c_void,
    user_info: *mut c_void,
) -> *mut c_void {
    if user_info.is_null() {
        return event;
    }
    // SAFETY: `user_info` points to the heap-boxed `TapContext` alive for the runloop lifetime.
    let ctx = unsafe { &mut *(user_info as *mut TapContext) };

    if event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT {
        if !ctx.tap_port.is_null() {
            // SAFETY: Re-enabling our active CGEventTap if macOS timed it out.
            unsafe { CGEventTapEnable(ctx.tap_port, true) };
        }
        return event;
    }

    if event.is_null() || (event_type != K_CG_EVENT_KEY_DOWN && event_type != K_CG_EVENT_KEY_UP) {
        return event;
    }

    // SAFETY: Reading keycode, autorepeat, and modifier flags from a valid `CGEventRef`.
    let (kc, is_repeat, flags) = unsafe {
        let kc = CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) as u16;
        let rep = CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_AUTOREPEAT) != 0;
        let flags = CGEventGetFlags(event);
        (kc, rep, flags)
    };

    if kc != ctx.spec.keycode {
        return event;
    }

    let is_down = event_type == K_CG_EVENT_KEY_DOWN;
    match ctx.mode {
        HotkeyMode::Toggle => {
            if is_down && ctx.spec.modifiers.satisfied_by_cg_flags(flags) {
                ctx.suppressed_kc = Some(kc);
                if !is_repeat {
                    let now = Instant::now();
                    let debounced = ctx
                        .last_toggle
                        .is_some_and(|prev| now.duration_since(prev) < TOGGLE_DEBOUNCE);
                    if !debounced {
                        ctx.last_toggle = Some(now);
                        let _ = ctx.event_tx.send(HotkeyEvent::Toggle);
                    }
                }
                return null_mut(); // Swallow hotkey cleanly
            } else if !is_down && ctx.suppressed_kc == Some(kc) {
                ctx.suppressed_kc = None;
                return null_mut();
            }
        }
        HotkeyMode::Hold => {
            if is_down && ctx.spec.modifiers.satisfied_by_cg_flags(flags) {
                ctx.suppressed_kc = Some(kc);
                if !ctx.hold_active {
                    ctx.hold_active = true;
                    let _ = ctx.event_tx.send(HotkeyEvent::HoldPressed);
                }
                return null_mut();
            } else if !is_down && (ctx.hold_active || ctx.suppressed_kc == Some(kc)) {
                ctx.suppressed_kc = None;
                if ctx.hold_active {
                    ctx.hold_active = false;
                    let _ = ctx.event_tx.send(HotkeyEvent::HoldReleased);
                }
                return null_mut();
            }
        }
    }

    event
}

struct MacHotkeyRegistration {
    runloop_raw: usize,
    tap_thread: Option<JoinHandle<()>>,
    dispatch_thread: Option<JoinHandle<()>>,
}

impl HotkeyRegistration for MacHotkeyRegistration {}

impl Drop for MacHotkeyRegistration {
    fn drop(&mut self) {
        if self.runloop_raw != 0 {
            let rl = self.runloop_raw as *mut c_void;
            // SAFETY: `rl` was retained for this registration and `CFRunLoopStop` is thread-safe.
            unsafe {
                CFRunLoopStop(rl);
                CFRelease(rl.cast());
            }
            self.runloop_raw = 0;
        }
        if let Some(h) = self.tap_thread.take() {
            let _ = h.join();
        }
        if let Some(h) = self.dispatch_thread.take() {
            let _ = h.join();
        }
    }
}

/// macOS CoreGraphics global hotkey service.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacHotkeyService;

impl HotkeyService for MacHotkeyService {
    fn register(
        &self,
        spec: &HotkeySpec,
        callback: HotkeyCallback,
    ) -> Result<Box<dyn HotkeyRegistration>, PlatformError> {
        let parsed = parse_hotkey_combo(&spec.raw)?;
        let mode = spec.mode;

        let (event_tx, event_rx) = mpsc::channel::<HotkeyEvent>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<usize, PlatformError>>(1);

        let dispatch_thread = thread::Builder::new()
            .name("eqho-hotkey-dispatch".to_owned())
            .spawn(move || {
                while let Ok(ev) = event_rx.recv() {
                    callback(ev);
                }
            })
            .map_err(|e| PlatformError(format!("failed to spawn hotkey dispatch thread: {e}")))?;

        let tap_thread = thread::Builder::new()
            .name("eqho-hotkey-macos".to_owned())
            .spawn(move || {
                let mut ctx = Box::new(TapContext {
                    spec: parsed,
                    mode,
                    tap_port: null_mut(),
                    event_tx,
                    last_toggle: None,
                    hold_active: false,
                    suppressed_kc: None,
                });
                let mask = (1u64 << K_CG_EVENT_KEY_DOWN) | (1u64 << K_CG_EVENT_KEY_UP);
                // SAFETY: Installing a CoreGraphics session event tap on this thread's CFRunLoop.
                unsafe {
                    let tap = CGEventTapCreate(
                        K_CG_SESSION_EVENT_TAP,
                        K_CG_HEAD_INSERT_EVENT_TAP,
                        K_CG_EVENT_TAP_OPTION_DEFAULT,
                        mask,
                        event_tap_proc,
                        (&mut *ctx as *mut TapContext).cast(),
                    );
                    if tap.is_null() {
                        let _ = ready_tx.send(Err(PlatformError(
                            "CGEventTapCreate failed (grant Accessibility / Input Monitoring in System Settings -> Privacy & Security)".to_owned(),
                        )));
                        return;
                    }
                    ctx.tap_port = tap;
                    let src = CFMachPortCreateRunLoopSource(null_mut(), tap, 0);
                    if src.is_null() {
                        CFRelease(tap.cast());
                        let _ = ready_tx.send(Err(PlatformError(
                            "CFMachPortCreateRunLoopSource failed".to_owned(),
                        )));
                        return;
                    }
                    let rl = CFRunLoopGetCurrent();
                    CFRetain(rl.cast());
                    CFRunLoopAddSource(rl, src, kCFRunLoopCommonModes);
                    CGEventTapEnable(tap, true);

                    let _ = ready_tx.send(Ok(rl as usize));
                    CFRunLoopRun();

                    CGEventTapEnable(tap, false);
                    CFRelease(src.cast());
                    CFRelease(tap.cast());
                }
                drop(ctx);
            })
            .map_err(|e| PlatformError(format!("failed to spawn macOS hotkey thread: {e}")))?;

        match ready_rx.recv() {
            Ok(Ok(runloop_raw)) => Ok(Box::new(MacHotkeyRegistration {
                runloop_raw,
                tap_thread: Some(tap_thread),
                dispatch_thread: Some(dispatch_thread),
            })),
            Ok(Err(err)) => {
                let _ = tap_thread.join();
                let _ = dispatch_thread.join();
                Err(err)
            }
            Err(_) => {
                let _ = tap_thread.join();
                let _ = dispatch_thread.join();
                Err(PlatformError("macOS hotkey thread exited unexpectedly".to_owned()))
            }
        }
    }
}
