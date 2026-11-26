//! macOS keyboard injection (`Cmd+V`, Unicode typing, Backspace, modifier release) via CoreGraphics.

use std::ffi::c_void;
use std::ptr::null;

use eqho_platform::{InputInjector, ModifierKey, PlatformError};

use crate::pure::{CG_FLAG_ALTERNATE, CG_FLAG_COMMAND, CG_FLAG_CONTROL, CG_FLAG_SHIFT};

const K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION: i32 = 0;
const K_CG_HID_EVENT_TAP: u32 = 0;

const K_VK_V: u16 = 0x09;
const K_VK_DELETE: u16 = 0x33;
const K_VK_RETURN: u16 = 0x24;
const K_VK_COMMAND_L: u16 = 0x37;
const K_VK_COMMAND_R: u16 = 0x36;
const K_VK_SHIFT_L: u16 = 0x38;
const K_VK_SHIFT_R: u16 = 0x3C;
const K_VK_OPTION_L: u16 = 0x3A;
const K_VK_OPTION_R: u16 = 0x3D;
const K_VK_CONTROL_L: u16 = 0x3B;
const K_VK_CONTROL_R: u16 = 0x3E;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceFlagsState(state_id: i32) -> u64;
    fn CGEventCreateKeyboardEvent(source: *const c_void, virtual_key: u16, key_down: bool)
        -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventKeyboardSetUnicodeString(
        event: *mut c_void,
        string_length: usize,
        unicode_string: *const u16,
    );
    fn CGEventPost(tap: u32, event: *mut c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// CoreGraphics keyboard injector (`Cmd+V` paste, direct UTF-16 Unicode typing, Backspace).
#[derive(Debug, Default, Clone, Copy)]
pub struct MacInputInjector;

impl MacInputInjector {
    fn post_key(vk: u16, down: bool, flags: Option<u64>) -> Result<(), PlatformError> {
        // SAFETY: Creating, posting, and releasing a CoreGraphics keyboard event.
        unsafe {
            let ev = CGEventCreateKeyboardEvent(null(), vk, down);
            if ev.is_null() {
                return Err(PlatformError(
                    "CGEventCreateKeyboardEvent failed (grant Accessibility permission in System Settings)"
                        .to_owned(),
                ));
            }
            if let Some(f) = flags {
                CGEventSetFlags(ev, f);
            }
            CGEventPost(K_CG_HID_EVENT_TAP, ev);
            CFRelease(ev.cast());
        }
        Ok(())
    }

    fn modifier_vk(m: ModifierKey) -> u16 {
        match m {
            ModifierKey::CtrlLeft => K_VK_CONTROL_L,
            ModifierKey::CtrlRight => K_VK_CONTROL_R,
            ModifierKey::ShiftLeft => K_VK_SHIFT_L,
            ModifierKey::ShiftRight => K_VK_SHIFT_R,
            ModifierKey::AltLeft => K_VK_OPTION_L,
            ModifierKey::AltRight => K_VK_OPTION_R,
            ModifierKey::WinLeft => K_VK_COMMAND_L,
            ModifierKey::WinRight => K_VK_COMMAND_R,
        }
    }
}

impl InputInjector for MacInputInjector {
    fn held_modifiers(&self) -> Option<Vec<ModifierKey>> {
        // SAFETY: Stateless query of combined session modifier flags.
        let flags = unsafe { CGEventSourceFlagsState(K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION) };
        let mut held = Vec::new();
        if (flags & CG_FLAG_CONTROL) != 0 {
            held.push(ModifierKey::CtrlLeft);
        }
        if (flags & CG_FLAG_ALTERNATE) != 0 {
            held.push(ModifierKey::AltLeft);
        }
        if (flags & CG_FLAG_SHIFT) != 0 {
            held.push(ModifierKey::ShiftLeft);
        }
        if (flags & CG_FLAG_COMMAND) != 0 {
            held.push(ModifierKey::WinLeft);
        }
        Some(held)
    }

    fn release_modifiers(&self, mods: &[ModifierKey]) -> Result<(), PlatformError> {
        for &m in mods {
            let _ = Self::post_key(Self::modifier_vk(m), false, Some(0));
        }
        Ok(())
    }

    fn send_paste_shortcut(&self) -> Result<(), PlatformError> {
        Self::post_key(K_VK_V, true, Some(CG_FLAG_COMMAND))?;
        Self::post_key(K_VK_V, false, Some(CG_FLAG_COMMAND))?;
        Ok(())
    }

    fn type_unicode_text(&self, text: &str) -> Result<(), PlatformError> {
        if text.is_empty() {
            return Ok(());
        }
        for part in text.split('\n') {
            if !part.is_empty() {
                let utf16: Vec<u16> = part.encode_utf16().collect();
                // CoreGraphics recommends chunks <= 20 UTF-16 code units per event.
                for chunk in utf16.chunks(20) {
                    // SAFETY: Posting synthesized Unicode keydown/keyup events.
                    unsafe {
                        let down = CGEventCreateKeyboardEvent(null(), 0, true);
                        let up = CGEventCreateKeyboardEvent(null(), 0, false);
                        if down.is_null() || up.is_null() {
                            if !down.is_null() {
                                CFRelease(down.cast());
                            }
                            if !up.is_null() {
                                CFRelease(up.cast());
                            }
                            return Err(PlatformError(
                                "CGEventCreateKeyboardEvent failed for Unicode typing".to_owned(),
                            ));
                        }
                        CGEventKeyboardSetUnicodeString(down, chunk.len(), chunk.as_ptr());
                        CGEventKeyboardSetUnicodeString(up, chunk.len(), chunk.as_ptr());
                        CGEventPost(K_CG_HID_EVENT_TAP, down);
                        CGEventPost(K_CG_HID_EVENT_TAP, up);
                        CFRelease(down.cast());
                        CFRelease(up.cast());
                    }
                }
            }
        }
        let newline_count = text.chars().filter(|&c| c == '\n').count();
        if newline_count > 0 && text.ends_with('\n') {
            Self::post_key(K_VK_RETURN, true, Some(0))?;
            Self::post_key(K_VK_RETURN, false, Some(0))?;
        }
        Ok(())
    }

    fn send_backspaces(&self, count: usize) -> Result<(), PlatformError> {
        for _ in 0..count {
            Self::post_key(K_VK_DELETE, true, Some(0))?;
            Self::post_key(K_VK_DELETE, false, Some(0))?;
        }
        Ok(())
    }
}
