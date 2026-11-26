//! Pure-Rust helpers for `eqho-platform-macos`, compiled and unit-tested on all targets.

use eqho_platform::{GpuAdapter, PlatformError};

pub const CG_FLAG_SHIFT: u64 = 0x0002_0000;
pub const CG_FLAG_CONTROL: u64 = 0x0004_0000;
pub const CG_FLAG_ALTERNATE: u64 = 0x0008_0000; // Option / Alt
pub const CG_FLAG_COMMAND: u64 = 0x0010_0000; // Command / Win

/// Modifier bitmask for macOS `CGEventTap` global hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MacHotkeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
}

impl MacHotkeyModifiers {
    /// Returns `true` if `cg_flags` satisfies all required modifiers.
    #[must_use]
    pub const fn satisfied_by_cg_flags(self, cg_flags: u64) -> bool {
        (!self.ctrl || (cg_flags & CG_FLAG_CONTROL) != 0)
            && (!self.alt || (cg_flags & CG_FLAG_ALTERNATE) != 0)
            && (!self.shift || (cg_flags & CG_FLAG_SHIFT) != 0)
            && (!self.cmd || (cg_flags & CG_FLAG_COMMAND) != 0)
    }
}

/// Parsed macOS hotkey: Carbon/CoreGraphics virtual keycode (`kVK_*`) + modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParsedMacHotkey {
    pub keycode: u16,
    pub modifiers: MacHotkeyModifiers,
}

/// Parse a hotkey string like `"alt+q"` (Option+Q), `"cmd+shift+space"`, or `"f9"`.
pub fn parse_hotkey_combo(raw: &str) -> Result<ParsedMacHotkey, PlatformError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(PlatformError("empty hotkey combination".to_owned()));
    }
    let mut modifiers = MacHotkeyModifiers::default();
    let mut trigger: Option<u16> = None;

    for part in trimmed.split('+') {
        let token = part
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .to_ascii_lowercase();
        if token.is_empty() {
            return Err(PlatformError(format!("invalid empty token in hotkey: {raw:?}")));
        }
        match token.as_str() {
            "ctrl" | "control" | "ctrl_l" | "ctrl_r" | "lctrl" | "rctrl" => modifiers.ctrl = true,
            "alt" | "alt_l" | "alt_r" | "lalt" | "ralt" | "option" | "opt" => modifiers.alt = true,
            "shift" | "shift_l" | "shift_r" | "lshift" | "rshift" => modifiers.shift = true,
            "cmd" | "command" | "win" | "windows" | "super" | "meta" | "lwin" | "rwin" => {
                modifiers.cmd = true;
            }
            other => {
                if trigger.is_some() {
                    return Err(PlatformError(format!(
                        "multiple trigger keys in hotkey: {raw:?}"
                    )));
                }
                trigger = Some(parse_mac_keycode(other).ok_or_else(|| {
                    PlatformError(format!("unknown macOS key {other:?} in hotkey {raw:?}"))
                })?);
            }
        }
    }

    let keycode =
        trigger.ok_or_else(|| PlatformError(format!("hotkey has no trigger key: {raw:?}")))?;
    Ok(ParsedMacHotkey { keycode, modifiers })
}

/// Map normalized key names to macOS ANSI virtual keycodes (`Events.h` `kVK_*`).
fn parse_mac_keycode(token: &str) -> Option<u16> {
    match token {
        "a" => Some(0x00),
        "s" => Some(0x01),
        "d" => Some(0x02),
        "f" => Some(0x03),
        "h" => Some(0x04),
        "g" => Some(0x05),
        "z" => Some(0x06),
        "x" => Some(0x07),
        "c" => Some(0x08),
        "v" => Some(0x09),
        "b" => Some(0x0B),
        "q" => Some(0x0C),
        "w" => Some(0x0D),
        "e" => Some(0x0E),
        "r" => Some(0x0F),
        "y" => Some(0x10),
        "t" => Some(0x11),
        "1" => Some(0x12),
        "2" => Some(0x13),
        "3" => Some(0x14),
        "4" => Some(0x15),
        "6" => Some(0x16),
        "5" => Some(0x17),
        "equal" | "equals" | "=" => Some(0x18),
        "9" => Some(0x19),
        "7" => Some(0x1A),
        "minus" | "-" => Some(0x1B),
        "8" => Some(0x1C),
        "0" => Some(0x1D),
        "o" => Some(0x1F),
        "u" => Some(0x20),
        "i" => Some(0x22),
        "p" => Some(0x23),
        "enter" | "return" => Some(0x24),
        "l" => Some(0x25),
        "j" => Some(0x26),
        "k" => Some(0x28),
        "semicolon" | ";" => Some(0x29),
        "comma" | "," => Some(0x2B),
        "slash" | "/" => Some(0x2C),
        "n" => Some(0x2D),
        "m" => Some(0x2E),
        "period" | "dot" | "." => Some(0x2F),
        "tab" => Some(0x30),
        "space" | "spacebar" => Some(0x31),
        "grave" | "backtick" | "`" => Some(0x32),
        "backspace" | "back" => Some(0x33),
        "esc" | "escape" => Some(0x35),
        "delete" | "del" => Some(0x75),
        "f1" => Some(0x7A),
        "f2" => Some(0x78),
        "f3" => Some(0x63),
        "f4" => Some(0x76),
        "f5" => Some(0x60),
        "f6" => Some(0x61),
        "f7" => Some(0x62),
        "f8" => Some(0x64),
        "f9" => Some(0x65),
        "f10" => Some(0x6D),
        "f11" => Some(0x67),
        "f12" => Some(0x6F),
        "f13" => Some(0x69),
        "f14" => Some(0x6B),
        "f15" => Some(0x71),
        "f16" => Some(0x6A),
        "f17" => Some(0x40),
        "f18" => Some(0x4F),
        "f19" => Some(0x50),
        "f20" => Some(0x5A),
        "home" => Some(0x73),
        "end" => Some(0x77),
        "pageup" | "page_up" | "pgup" => Some(0x74),
        "pagedown" | "page_down" | "pgdn" => Some(0x79),
        "left" => Some(0x7B),
        "right" => Some(0x7C),
        "down" => Some(0x7D),
        "up" => Some(0x7E),
        _ => None,
    }
}

/// Build a `GpuAdapter` descriptor for Apple Silicon unified Metal GPUs (allocated ~75% of
/// unified memory as recommended working set by Metal).
#[must_use]
pub fn classify_apple_gpu(cpu_brand: &str, total_ram_bytes: u64, is_arm64: bool) -> Option<GpuAdapter> {
    let trimmed = cpu_brand.trim();
    if is_arm64 || trimmed.to_ascii_lowercase().contains("apple m") {
        let name = if trimmed.is_empty() {
            "Apple Silicon GPU".to_owned()
        } else {
            trimmed.to_owned()
        };
        // Metal recommends ~75% of unified memory for GPU working set on Apple Silicon.
        let vram = (total_ram_bytes / 4).saturating_mul(3);
        Some(GpuAdapter {
            name,
            vendor_id: 0x106B, // Apple PCI vendor ID
            dedicated_video_memory_bytes: vram,
        })
    } else {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_macos_hotkeys_and_cg_flags() {
        let opt_q = parse_hotkey_combo("alt+q").unwrap();
        assert_eq!(opt_q.keycode, 0x0C);
        assert!(opt_q.modifiers.satisfied_by_cg_flags(CG_FLAG_ALTERNATE));
        assert!(!opt_q.modifiers.satisfied_by_cg_flags(CG_FLAG_COMMAND));

        let cmd_space = parse_hotkey_combo("cmd+shift+space").unwrap();
        assert_eq!(cmd_space.keycode, 0x31);
        assert!(cmd_space.modifiers.satisfied_by_cg_flags(CG_FLAG_COMMAND | CG_FLAG_SHIFT));

        assert!(parse_hotkey_combo("option+f12").is_ok());
        assert!(parse_hotkey_combo("").is_err());
    }

    #[test]
    fn classifies_apple_silicon_metal_gpu() {
        let m3 = classify_apple_gpu("Apple M3 Pro", 18 * 1024 * 1024 * 1024, true).unwrap();
        assert_eq!(m3.name, "Apple M3 Pro");
        assert_eq!(m3.vendor_id, 0x106B);
        assert!(m3.dedicated_video_memory_bytes > 10 * 1024 * 1024 * 1024);
        assert!(classify_apple_gpu("Intel(R) Core(TM) i7", 16 * 1024 * 1024 * 1024, false).is_none());
    }
}
