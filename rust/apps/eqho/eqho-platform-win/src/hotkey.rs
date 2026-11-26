//! Global Win32 hotkey listener supporting `Toggle` and `Hold` modes.
//!
//! - `HotkeyMode::Toggle` registers a kernel-level Win32 `RegisterHotKey` first (which
//!   reliably swallows `Alt+Q` before any focused text input — including Onboarding Step 6 —
//!   never hits `LowLevelHooksTimeout`, and works over elevated windows), transparently falling
//!   back to `WH_KEYBOARD_LL` if another app already owns that `RegisterHotKey` chord.
//! - `HotkeyMode::Hold` uses `WH_KEYBOARD_LL` with direct modifier keydown/keyup state tracking
//!   inside `HookState` (combined with `LLKHF_ALTDOWN` and `GetAsyncKeyState`) and ignores
//!   `LLKHF_INJECTED` events so Eqho's own `SendInput` never triggers itself.

use std::cell::RefCell;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_platform::{
    HotkeyCallback, HotkeyEvent, HotkeyMode, HotkeyRegistration, HotkeyService, HotkeySpec,
    PlatformError,
};
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    RegisterHotKey, UnregisterHotKey, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
    VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE,
    PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_USER,
};

use crate::pure::{HotkeyModifiers, ParsedHotkey, is_async_key_down, parse_hotkey_combo};

const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(400);
const HOTKEY_ID: i32 = 0x4551; // "EQ"
const LLKHF_INJECTED: u32 = 0x10;
const LLKHF_ALTDOWN: u32 = 0x20;

#[derive(Default)]
struct TrackedModifiers {
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
}

impl TrackedModifiers {
    fn update(&mut self, vk: u16, is_down: bool) {
        if vk == VK_CONTROL.0 || vk == VK_LCONTROL.0 || vk == VK_RCONTROL.0 {
            self.ctrl = is_down;
        } else if vk == VK_MENU.0 || vk == VK_LMENU.0 || vk == VK_RMENU.0 {
            self.alt = is_down;
        } else if vk == VK_SHIFT.0 || vk == VK_LSHIFT.0 || vk == VK_RSHIFT.0 {
            self.shift = is_down;
        } else if vk == VK_LWIN.0 || vk == VK_RWIN.0 {
            self.win = is_down;
        }
    }
}

struct HookState {
    spec: ParsedHotkey,
    mode: HotkeyMode,
    event_tx: mpsc::Sender<HotkeyEvent>,
    last_toggle: Option<Instant>,
    hold_active: bool,
    suppressed_vk: Option<u16>,
    tracked: TrackedModifiers,
}

thread_local! {
    static HOOK_STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

fn any_vk_down(vks: &[VIRTUAL_KEY]) -> bool {
    vks.iter().any(|&vk| {
        // SAFETY: `GetAsyncKeyState` queries the live key state for a valid `VIRTUAL_KEY`.
        let state = unsafe { GetAsyncKeyState(i32::from(vk.0)) };
        is_async_key_down(state)
    })
}

fn live_modifiers(kb_flags: u32, tracked: &TrackedModifiers) -> HotkeyModifiers {
    HotkeyModifiers {
        ctrl: tracked.ctrl || any_vk_down(&[VK_CONTROL, VK_LCONTROL, VK_RCONTROL]),
        alt: tracked.alt
            || (kb_flags & LLKHF_ALTDOWN) != 0
            || any_vk_down(&[VK_MENU, VK_LMENU, VK_RMENU]),
        shift: tracked.shift || any_vk_down(&[VK_SHIFT, VK_LSHIFT, VK_RSHIFT]),
        win: tracked.win || any_vk_down(&[VK_LWIN, VK_RWIN]),
    }
}

fn to_win32_hotkey_mods(mods: HotkeyModifiers) -> HOT_KEY_MODIFIERS {
    let mut out = MOD_NOREPEAT;
    if mods.ctrl {
        out |= MOD_CONTROL;
    }
    if mods.alt {
        out |= MOD_ALT;
    }
    if mods.shift {
        out |= MOD_SHIFT;
    }
    if mods.win {
        out |= MOD_WIN;
    }
    out
}

unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let mut suppress = false;
    if code == i32::try_from(HC_ACTION).unwrap_or(0) {
        // SAFETY: For `HC_ACTION`, Win32 passes a valid `KBDLLHOOKSTRUCT` pointer in `lparam`.
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if (kb.flags.0 & LLKHF_INJECTED) != 0 {
            // Never intercept Eqho's own synthesized `SendInput` events.
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }
        let vk = kb.vkCode as u16;
        let msg = wparam.0 as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

        if is_down || is_up {
            HOOK_STATE.with(|cell| {
                let mut borrow = cell.borrow_mut();
                let Some(state) = borrow.as_mut() else {
                    return;
                };
                state.tracked.update(vk, is_down);
                if vk != state.spec.vk {
                    return;
                }

                let mods = live_modifiers(kb.flags.0, &state.tracked);
                match state.mode {
                    HotkeyMode::Toggle => {
                        if is_down && state.spec.modifiers.satisfied_by(mods) {
                            suppress = true;
                            state.suppressed_vk = Some(vk);
                            let now = Instant::now();
                            let debounced = state
                                .last_toggle
                                .is_some_and(|prev| now.duration_since(prev) < TOGGLE_DEBOUNCE);
                            if !debounced {
                                state.last_toggle = Some(now);
                                // Non-blocking send to the dispatcher thread keeps hook latency <1 ms.
                                let _ = state.event_tx.send(HotkeyEvent::Toggle);
                            }
                        } else if is_up && state.suppressed_vk == Some(vk) {
                            state.suppressed_vk = None;
                            suppress = true;
                        }
                    }
                    HotkeyMode::Hold => {
                        if is_down && state.spec.modifiers.satisfied_by(mods) {
                            suppress = true;
                            state.suppressed_vk = Some(vk);
                            if !state.hold_active {
                                state.hold_active = true;
                                let _ = state.event_tx.send(HotkeyEvent::HoldPressed);
                            }
                        } else if is_up && (state.hold_active || state.suppressed_vk == Some(vk)) {
                            if state.suppressed_vk == Some(vk) {
                                state.suppressed_vk = None;
                                suppress = true;
                            }
                            if state.hold_active {
                                state.hold_active = false;
                                let _ = state.event_tx.send(HotkeyEvent::HoldReleased);
                            }
                        }
                    }
                }
            });
        }
    }

    if suppress {
        // Swallow the matched hotkey keydown so Alt+Q never triggers a Windows system beep
        // or types 'q' into the active text input (including Onboarding Step 6).
        return LRESULT(1);
    }

    // SAFETY: Pass all other events down the hook chain per Win32 `LowLevelKeyboardProc` contract.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Active global Win32 hotkey registration; dropping posts `WM_QUIT`, joins, and unregisters.
#[derive(Debug)]
pub struct WinHotkeyRegistration {
    thread_id: u32,
    hook_handle: Option<JoinHandle<()>>,
    dispatch_handle: Option<JoinHandle<()>>,
}

impl HotkeyRegistration for WinHotkeyRegistration {}

impl Drop for WinHotkeyRegistration {
    fn drop(&mut self) {
        // SAFETY: `self.thread_id` is a valid Win32 thread ID whose message queue was created before `register()` returned.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(handle) = self.hook_handle.take() {
            let _ = handle.join();
        }
        // Joining `hook_handle` guarantees the listener thread (and its `event_tx`) has been dropped,
        // so the dispatcher thread exits `recv()` cleanly.
        if let Some(handle) = self.dispatch_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Global Win32 hotkey service using kernel `RegisterHotKey` (for `Toggle`) with `WH_KEYBOARD_LL`
/// fallback and `WH_KEYBOARD_LL` with direct modifier tracking (for `Hold`).
#[derive(Debug, Default, Clone, Copy)]
pub struct WinHotkeyService;

impl HotkeyService for WinHotkeyService {
    fn register(
        &self,
        spec: &HotkeySpec,
        callback: HotkeyCallback,
    ) -> Result<Box<dyn HotkeyRegistration>, PlatformError> {
        let parsed = parse_hotkey_combo(&spec.raw)?;
        let mode = spec.mode;
        let (event_tx, event_rx) = mpsc::channel::<HotkeyEvent>();
        let (tx, rx) = mpsc::sync_channel::<Result<u32, PlatformError>>(1);

        let dispatch_handle = thread::Builder::new()
            .name("eqho-hotkey-dispatch".to_owned())
            .spawn(move || {
                while let Ok(event) = event_rx.recv() {
                    callback(event);
                }
            })
            .map_err(|e| PlatformError(format!("failed to spawn hotkey dispatch thread: {e}")))?;

        let hook_handle = match thread::Builder::new()
            .name("eqho-hotkey".to_owned())
            .spawn(move || {
                let mut msg = MSG::default();
                // SAFETY: Force Windows to allocate a message queue on this thread before
                // `RegisterHotKey` / returning `tid`.
                let _ = unsafe { PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE) };

                // For Toggle mode, try kernel-level `RegisterHotKey` first so Windows guarantees
                // the chord is swallowed before any focused input (including Onboarding Step 6).
                if mode == HotkeyMode::Toggle {
                    let win_mods = to_win32_hotkey_mods(parsed.modifiers);
                    // SAFETY: Thread message queue exists; `None` binds the hotkey to this thread.
                    let reg_ok = unsafe {
                        RegisterHotKey(None, HOTKEY_ID, win_mods, u32::from(parsed.vk))
                    }
                    .is_ok();
                    if reg_ok {
                        // SAFETY: `GetCurrentThreadId` returns the OS thread ID of this loop thread.
                        let tid = unsafe { GetCurrentThreadId() };
                        let _ = tx.send(Ok(tid));

                        let mut last_toggle: Option<Instant> = None;
                        // SAFETY: `msg` is a valid mutable `MSG` and `GetMessageW` runs until `WM_QUIT`.
                        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                            if msg.message == WM_HOTKEY && msg.wParam.0 == HOTKEY_ID as usize {
                                let now = Instant::now();
                                let debounced = last_toggle
                                    .is_some_and(|prev| now.duration_since(prev) < TOGGLE_DEBOUNCE);
                                if !debounced {
                                    last_toggle = Some(now);
                                    let _ = event_tx.send(HotkeyEvent::Toggle);
                                }
                            }
                            let _ = unsafe { TranslateMessage(&msg) };
                            unsafe { DispatchMessageW(&msg) };
                        }

                        // SAFETY: `HOTKEY_ID` was successfully registered on this thread above.
                        let _ = unsafe { UnregisterHotKey(None, HOTKEY_ID) };
                        return;
                    }
                }

                // Hold mode (or fallback if another app already owns `RegisterHotKey`):
                // install `WH_KEYBOARD_LL` with explicit modifier tracking.
                HOOK_STATE.with(|cell| {
                    *cell.borrow_mut() = Some(HookState {
                        spec: parsed,
                        mode,
                        event_tx,
                        last_toggle: None,
                        hold_active: false,
                        suppressed_vk: None,
                        tracked: TrackedModifiers::default(),
                    });
                });

                // SAFETY: `GetModuleHandleW(None)` retrieves the current executable's module handle,
                // and `low_level_keyboard_proc` is a valid `HOOKPROC` for global `WH_KEYBOARD_LL`.
                let hmod: Option<HINSTANCE> = unsafe { GetModuleHandleW(None) }
                    .ok()
                    .map(Into::into);
                let hook = unsafe {
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), hmod, 0)
                };
                let hook = match hook {
                    Ok(h) => h,
                    Err(e) => {
                        HOOK_STATE.with(|cell| *cell.borrow_mut() = None);
                        let _ = tx.send(Err(PlatformError(format!(
                            "SetWindowsHookExW failed: {e}"
                        ))));
                        return;
                    }
                };

                // SAFETY: `GetCurrentThreadId` returns the OS thread ID of this message-loop thread.
                let tid = unsafe { GetCurrentThreadId() };
                let _ = tx.send(Ok(tid));

                // SAFETY: `msg` is a valid mutable `MSG` and `GetMessageW` runs until `WM_QUIT`.
                while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                    // SAFETY: `msg` was populated by `GetMessageW`.
                    let _ = unsafe { TranslateMessage(&msg) };
                    // SAFETY: `msg` was populated by `GetMessageW`.
                    unsafe { DispatchMessageW(&msg) };
                }

                // SAFETY: `hook` was installed on this thread and is unhooked before exit.
                let _ = unsafe { UnhookWindowsHookEx(hook) };
                HOOK_STATE.with(|cell| *cell.borrow_mut() = None);
            }) {
            Ok(h) => h,
            Err(e) => {
                let _ = dispatch_handle.join();
                return Err(PlatformError(format!("failed to spawn hotkey thread: {e}")));
            }
        };

        let thread_id = match rx.recv() {
            Ok(Ok(tid)) => tid,
            Ok(Err(err)) => {
                let _ = hook_handle.join();
                let _ = dispatch_handle.join();
                return Err(err);
            }
            Err(_) => {
                let _ = hook_handle.join();
                let _ = dispatch_handle.join();
                return Err(PlatformError(
                    "hotkey thread exited before reporting status".to_owned(),
                ));
            }
        };

        Ok(Box::new(WinHotkeyRegistration {
            thread_id,
            hook_handle: Some(hook_handle),
            dispatch_handle: Some(dispatch_handle),
        }))
    }
}
