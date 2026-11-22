//! Win32 keyboard input simulation via `GetAsyncKeyState` and `SendInput` (`EQ-MODS`).

use std::mem::size_of;
use std::thread;
use std::time::Duration;

use eqho_platform::{InputInjector, ModifierKey, PlatformError};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_LCONTROL,
    VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_V,
};

/// Maximum characters per `SendInput` batch in Unicode typing mode so Electron/IDE message queues
/// never overflow or drop characters on long dictations.
const TYPING_BATCH_CHARS: usize = 16;
const TYPING_BATCH_DELAY: Duration = Duration::from_millis(2);

/// Windows keyboard input injector.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinInputInjector;

const ALL_MODIFIERS: [(ModifierKey, VIRTUAL_KEY); 8] = [
    (ModifierKey::CtrlLeft, VK_LCONTROL),
    (ModifierKey::CtrlRight, VK_RCONTROL),
    (ModifierKey::ShiftLeft, VK_LSHIFT),
    (ModifierKey::ShiftRight, VK_RSHIFT),
    (ModifierKey::AltLeft, VK_LMENU),
    (ModifierKey::AltRight, VK_RMENU),
    (ModifierKey::WinLeft, VK_LWIN),
    (ModifierKey::WinRight, VK_RWIN),
];

const fn modifier_to_vk(key: ModifierKey) -> VIRTUAL_KEY {
    match key {
        ModifierKey::CtrlLeft => VK_LCONTROL,
        ModifierKey::CtrlRight => VK_RCONTROL,
        ModifierKey::ShiftLeft => VK_LSHIFT,
        ModifierKey::ShiftRight => VK_RSHIFT,
        ModifierKey::AltLeft => VK_LMENU,
        ModifierKey::AltRight => VK_RMENU,
        ModifierKey::WinLeft => VK_LWIN,
        ModifierKey::WinRight => VK_RWIN,
    }
}

fn vk_input(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn unicode_input(unit: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_inputs(inputs: &[INPUT]) -> Result<(), PlatformError> {
    if inputs.is_empty() {
        return Ok(());
    }
    let cb_size = i32::try_from(size_of::<INPUT>())
        .map_err(|_| PlatformError("INPUT size overflow".to_owned()))?;
    // SAFETY: `inputs` is a valid slice of `INPUT` structs and `cb_size == size_of::<INPUT>()`.
    let sent = unsafe { SendInput(inputs, cb_size) };
    if sent == 0 {
        return Err(PlatformError("SendInput sent 0 events".to_owned()));
    }
    Ok(())
}

impl InputInjector for WinInputInjector {
    fn held_modifiers(&self) -> Option<Vec<ModifierKey>> {
        let mut held = Vec::new();
        for (modifier, vk) in ALL_MODIFIERS {
            // SAFETY: `GetAsyncKeyState` queries the async key state for a valid `VIRTUAL_KEY`.
            let state = unsafe { GetAsyncKeyState(i32::from(vk.0)) };
            if (state as u16 & 0x8000) != 0 {
                held.push(modifier);
            }
        }
        Some(held)
    }

    fn release_modifiers(&self, mods: &[ModifierKey]) -> Result<(), PlatformError> {
        let inputs: Vec<INPUT> = mods
            .iter()
            .map(|&m| vk_input(modifier_to_vk(m), KEYEVENTF_KEYUP))
            .collect();
        send_inputs(&inputs)
    }

    fn send_paste_shortcut(&self) -> Result<(), PlatformError> {
        let inputs = [
            vk_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
            vk_input(VK_V, KEYBD_EVENT_FLAGS(0)),
            vk_input(VK_V, KEYEVENTF_KEYUP),
            vk_input(VK_CONTROL, KEYEVENTF_KEYUP),
        ];
        send_inputs(&inputs)
    }

    fn type_unicode_text(&self, text: &str) -> Result<(), PlatformError> {
        if text.is_empty() {
            return Ok(());
        }
        let mut batch = Vec::with_capacity(TYPING_BATCH_CHARS * 2);
        let mut chars_in_batch = 0usize;
        for ch in text.chars() {
            if ch == '\r' {
                continue;
            }
            if ch == '\n' {
                batch.push(vk_input(VK_RETURN, KEYBD_EVENT_FLAGS(0)));
                batch.push(vk_input(VK_RETURN, KEYEVENTF_KEYUP));
            } else {
                let mut utf16 = [0u16; 2];
                for &mut unit in ch.encode_utf16(&mut utf16) {
                    batch.push(unicode_input(unit, KEYEVENTF_UNICODE));
                    batch.push(unicode_input(unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            }
            chars_in_batch += 1;
            if chars_in_batch >= TYPING_BATCH_CHARS {
                send_inputs(&batch)?;
                batch.clear();
                chars_in_batch = 0;
                thread::sleep(TYPING_BATCH_DELAY);
            }
        }
        if !batch.is_empty() {
            send_inputs(&batch)?;
        }
        Ok(())
    }

    fn send_backspaces(&self, count: usize) -> Result<(), PlatformError> {
        if count == 0 {
            return Ok(());
        }
        let mut inputs = Vec::with_capacity(count * 2);
        for _ in 0..count {
            inputs.push(vk_input(VK_BACK, KEYBD_EVENT_FLAGS(0)));
            inputs.push(vk_input(VK_BACK, KEYEVENTF_KEYUP));
        }
        send_inputs(&inputs)
    }
}
