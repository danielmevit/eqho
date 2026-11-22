//! Global Win32 `WH_KEYBOARD_LL` hotkey listener supporting `Toggle` and `Hold` modes.

use std::cell::RefCell;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_platform::{
    HotkeyCallback, HotkeyEvent, HotkeyMode, HotkeyRegistration, HotkeyService, HotkeySpec,
    PlatformError,
};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE,
    PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_USER,
};

use crate::pure::{HotkeyModifiers, ParsedHotkey, is_async_key_down, parse_hotkey_combo};

const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(400);

struct HookState {
    spec: ParsedHotkey,
    mode: HotkeyMode,
    event_tx: mpsc::Sender<HotkeyEvent>,
    last_toggle: Option<Instant>,
    hold_active: bool,
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

fn live_modifiers() -> HotkeyModifiers {
    HotkeyModifiers {
        ctrl: any_vk_down(&[VK_CONTROL, VK_LCONTROL, VK_RCONTROL]),
        alt: any_vk_down(&[VK_MENU, VK_LMENU, VK_RMENU]),
        shift: any_vk_down(&[VK_SHIFT, VK_LSHIFT, VK_RSHIFT]),
        win: any_vk_down(&[VK_LWIN, VK_RWIN]),
    }
}

unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == i32::try_from(HC_ACTION).unwrap_or(0) {
        // SAFETY: For `HC_ACTION`, Win32 passes a valid `KBDLLHOOKSTRUCT` pointer in `lparam`.
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
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
                if vk != state.spec.vk {
                    return;
                }

                match state.mode {
                    HotkeyMode::Toggle => {
                        if is_down && state.spec.modifiers.satisfied_by(live_modifiers()) {
                            let now = Instant::now();
                            let debounced = state
                                .last_toggle
                                .is_some_and(|prev| now.duration_since(prev) < TOGGLE_DEBOUNCE);
                            if !debounced {
                                state.last_toggle = Some(now);
                                // Non-blocking send to the dispatcher thread keeps hook latency <1 ms.
                                let _ = state.event_tx.send(HotkeyEvent::Toggle);
                            }
                        }
                    }
                    HotkeyMode::Hold => {
                        if is_down
                            && !state.hold_active
                            && state.spec.modifiers.satisfied_by(live_modifiers())
                        {
                            state.hold_active = true;
                            let _ = state.event_tx.send(HotkeyEvent::HoldPressed);
                        } else if is_up && state.hold_active {
                            state.hold_active = false;
                            let _ = state.event_tx.send(HotkeyEvent::HoldReleased);
                        }
                    }
                }
            });
        }
    }

    // SAFETY: Pass all events down the hook chain per Win32 `LowLevelKeyboardProc` contract.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Active `WH_KEYBOARD_LL` hotkey registration; dropping posts `WM_QUIT`, joins, and unhooks.
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
        // Joining `hook_handle` guarantees `HOOK_STATE` (and its `event_tx`) has been dropped,
        // so the dispatcher thread exits `recv()` cleanly.
        if let Some(handle) = self.dispatch_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Global Win32 hotkey service using a dedicated `WH_KEYBOARD_LL` message-loop thread.
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
                HOOK_STATE.with(|cell| {
                    *cell.borrow_mut() = Some(HookState {
                        spec: parsed,
                        mode,
                        event_tx,
                        last_toggle: None,
                        hold_active: false,
                    });
                });

                // SAFETY: `low_level_keyboard_proc` is a valid `HOOKPROC` for `WH_KEYBOARD_LL`;
                // `hmod` is `None` and `dwThreadId` is `0` for a global low-level hook.
                let hook = unsafe {
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), None, 0)
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

                let mut msg = MSG::default();
                // SAFETY: Force Windows to allocate a message queue on this thread before sending `tid`,
                // preventing `PostThreadMessageW(WM_QUIT)` from failing with `ERROR_INVALID_THREAD_ID` on immediate drop.
                let _ = unsafe { PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE) };

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
