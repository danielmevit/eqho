//! Global Win32 `WH_KEYBOARD_LL` hotkey listener supporting `Toggle` and `Hold` modes.

use std::cell::RefCell;
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_platform::{
    HotkeyCallback, HotkeyEvent, HotkeyMode, HotkeyRegistration, HotkeyService, HotkeySpec,
    PlatformError,
};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
    PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::pure::{HotkeyModifiers, ParsedHotkey, parse_hotkey_combo};

const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(400);

type SharedHotkeyCallback = Arc<dyn Fn(HotkeyEvent) + Send + Sync>;

struct HookState {
    spec: ParsedHotkey,
    mode: HotkeyMode,
    callback: SharedHotkeyCallback,
    held_mods: HotkeyModifiers,
    last_toggle: Option<Instant>,
    hold_active: bool,
}

thread_local! {
    static HOOK_STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

fn update_modifier(held: &mut HotkeyModifiers, vk: u16, is_down: bool) -> bool {
    if vk == VK_CONTROL.0 || vk == VK_LCONTROL.0 || vk == VK_RCONTROL.0 {
        held.ctrl = is_down;
        return true;
    }
    if vk == VK_MENU.0 || vk == VK_LMENU.0 || vk == VK_RMENU.0 {
        held.alt = is_down;
        return true;
    }
    if vk == VK_SHIFT.0 || vk == VK_LSHIFT.0 || vk == VK_RSHIFT.0 {
        held.shift = is_down;
        return true;
    }
    if vk == VK_LWIN.0 || vk == VK_RWIN.0 {
        held.win = is_down;
        return true;
    }
    false
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
            let mut emit: Option<(SharedHotkeyCallback, HotkeyEvent)> = None;
            HOOK_STATE.with(|cell| {
                let mut borrow = cell.borrow_mut();
                let Some(state) = borrow.as_mut() else {
                    return;
                };
                if update_modifier(&mut state.held_mods, vk, is_down) {
                    return;
                }
                if vk != state.spec.vk {
                    return;
                }

                match state.mode {
                    HotkeyMode::Toggle => {
                        if is_down && state.spec.modifiers.satisfied_by(state.held_mods) {
                            let now = Instant::now();
                            let debounced = state
                                .last_toggle
                                .is_some_and(|prev| now.duration_since(prev) < TOGGLE_DEBOUNCE);
                            if !debounced {
                                state.last_toggle = Some(now);
                                emit = Some((Arc::clone(&state.callback), HotkeyEvent::Toggle));
                            }
                        }
                    }
                    HotkeyMode::Hold => {
                        if is_down
                            && !state.hold_active
                            && state.spec.modifiers.satisfied_by(state.held_mods)
                        {
                            state.hold_active = true;
                            emit = Some((Arc::clone(&state.callback), HotkeyEvent::HoldPressed));
                        } else if is_up && state.hold_active {
                            state.hold_active = false;
                            emit = Some((Arc::clone(&state.callback), HotkeyEvent::HoldReleased));
                        }
                    }
                }
            });
            if let Some((cb, event)) = emit {
                cb(event);
            }
        }
    }

    // SAFETY: Pass all events down the hook chain per Win32 `LowLevelKeyboardProc` contract.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Active `WH_KEYBOARD_LL` hotkey registration; dropping posts `WM_QUIT`, joins, and unhooks.
#[derive(Debug)]
pub struct WinHotkeyRegistration {
    thread_id: u32,
    join_handle: Option<JoinHandle<()>>,
}

impl HotkeyRegistration for WinHotkeyRegistration {}

impl Drop for WinHotkeyRegistration {
    fn drop(&mut self) {
        // SAFETY: `self.thread_id` is a valid Win32 thread ID running a `GetMessageW` loop.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(handle) = self.join_handle.take() {
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
        let callback: Arc<dyn Fn(HotkeyEvent) + Send + Sync> = Arc::from(callback);
        let (tx, rx) = mpsc::sync_channel::<Result<u32, PlatformError>>(1);

        let join_handle = thread::Builder::new()
            .name("eqho-hotkey".to_owned())
            .spawn(move || {
                HOOK_STATE.with(|cell| {
                    *cell.borrow_mut() = Some(HookState {
                        spec: parsed,
                        mode,
                        callback,
                        held_mods: HotkeyModifiers::default(),
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

                // SAFETY: `GetCurrentThreadId` returns the OS thread ID of this message-loop thread.
                let tid = unsafe { GetCurrentThreadId() };
                let _ = tx.send(Ok(tid));

                let mut msg = MSG::default();
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
            })
            .map_err(|e| PlatformError(format!("failed to spawn hotkey thread: {e}")))?;

        let thread_id = rx
            .recv()
            .map_err(|_| PlatformError("hotkey thread exited before reporting status".to_owned()))??;

        Ok(Box::new(WinHotkeyRegistration {
            thread_id,
            join_handle: Some(join_handle),
        }))
    }
}
