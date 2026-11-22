//! Pure-Rust helpers for `eqho-platform-win`, compiled and unit-tested on all targets.

use eqho_platform::{GpuAdapter, PlatformError};

/// Modifier key bitmask for a parsed global hotkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct HotkeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl HotkeyModifiers {
    /// Returns a compact 4-bit mask (`ctrl=1, alt=2, shift=4, win=8`).
    #[must_use]
    pub const fn bits(self) -> u8 {
        (self.ctrl as u8)
            | ((self.alt as u8) << 1)
            | ((self.shift as u8) << 2)
            | ((self.win as u8) << 3)
    }

    /// Returns `true` if `held` satisfies all modifiers required by `self`.
    #[must_use]
    pub const fn satisfied_by(self, held: Self) -> bool {
        (!self.ctrl || held.ctrl)
            && (!self.alt || held.alt)
            && (!self.shift || held.shift)
            && (!self.win || held.win)
    }
}

/// Parsed hotkey combination: Windows Virtual Key code plus required modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParsedHotkey {
    pub vk: u16,
    pub modifiers: HotkeyModifiers,
}

/// Parses a hotkey string like `"alt+q"`, `"ctrl+shift+f9"`, `"win+space"`, or `"f12"`.
///
/// Rejects empty strings, empty segments, multiple trigger keys, and modifier-only combos.
pub fn parse_hotkey_combo(raw: &str) -> Result<ParsedHotkey, PlatformError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(PlatformError("empty hotkey combination".to_owned()));
    }

    let mut modifiers = HotkeyModifiers::default();
    let mut trigger_vk: Option<u16> = None;

    for part in trimmed.split('+') {
        let token = part.trim().to_ascii_lowercase();
        if token.is_empty() {
            return Err(PlatformError(format!("invalid empty token in hotkey: {raw:?}")));
        }

        match token.as_str() {
            "ctrl" | "control" | "ctrl_l" | "ctrl_r" | "lctrl" | "rctrl" => {
                modifiers.ctrl = true;
            }
            "alt" | "alt_l" | "alt_r" | "alt_gr" | "lalt" | "ralt" | "menu" | "option" => {
                modifiers.alt = true;
            }
            "shift" | "shift_l" | "shift_r" | "lshift" | "rshift" => {
                modifiers.shift = true;
            }
            "win" | "windows" | "super" | "cmd" | "cmd_l" | "cmd_r" | "meta" | "lwin"
            | "rwin" => {
                modifiers.win = true;
            }
            other => {
                if trigger_vk.is_some() {
                    return Err(PlatformError(format!(
                        "multiple trigger keys in hotkey: {raw:?}"
                    )));
                }
                trigger_vk = Some(parse_trigger_vk(other).ok_or_else(|| {
                    PlatformError(format!("unknown key {other:?} in hotkey {raw:?}"))
                })?);
            }
        }
    }

    let vk = trigger_vk
        .ok_or_else(|| PlatformError(format!("hotkey has no trigger key: {raw:?}")))?;

    Ok(ParsedHotkey { vk, modifiers })
}

fn parse_trigger_vk(token: &str) -> Option<u16> {
    if token.len() == 1 {
        let b = token.as_bytes()[0];
        if b.is_ascii_lowercase() {
            return Some(u16::from(b.to_ascii_uppercase()));
        }
        if b.is_ascii_digit() {
            return Some(u16::from(b));
        }
    }

    if let Some(num) = token.strip_prefix('f')
        && let Ok(n) = num.parse::<u16>()
        && (1..=24).contains(&n)
    {
        return Some(0x70 + (n - 1));
    }

    if let Some(num) = token
        .strip_prefix("numpad")
        .or_else(|| token.strip_prefix("num"))
        && let Ok(n) = num.parse::<u16>()
        && n <= 9
        && num.len() == 1
    {
        return Some(0x60 + n);
    }

    match token {
        "space" | "spacebar" => Some(0x20),
        "enter" | "return" => Some(0x0D),
        "esc" | "escape" => Some(0x1B),
        "tab" => Some(0x09),
        "backspace" | "back" => Some(0x08),
        "delete" | "del" => Some(0x2E),
        "insert" | "ins" => Some(0x2D),
        "home" => Some(0x24),
        "end" => Some(0x23),
        "pageup" | "page_up" | "pgup" | "prior" => Some(0x21),
        "pagedown" | "page_down" | "pgdn" | "next" => Some(0x22),
        "up" | "arrow_up" => Some(0x26),
        "down" | "arrow_down" => Some(0x28),
        "left" | "arrow_left" => Some(0x25),
        "right" | "arrow_right" => Some(0x27),
        "capslock" | "caps_lock" | "caps" => Some(0x14),
        "numlock" | "num_lock" => Some(0x90),
        "scrolllock" | "scroll_lock" => Some(0x91),
        "pause" | "break" => Some(0x13),
        "printscreen" | "print_screen" | "prtsc" | "snapshot" => Some(0x2C),
        "apps" | "context_menu" => Some(0x5D),
        ";" | "semicolon" => Some(0xBA),
        "=" | "equal" | "equals" | "plus" => Some(0xBB),
        "," | "comma" => Some(0xBC),
        "-" | "minus" => Some(0xBD),
        "." | "period" | "dot" => Some(0xBE),
        "/" | "slash" => Some(0xBF),
        "`" | "grave" | "backtick" | "tilde" => Some(0xC0),
        "[" | "lbracket" => Some(0xDB),
        "\\" | "backslash" => Some(0xDC),
        "]" | "rbracket" => Some(0xDD),
        "'" | "quote" | "apostrophe" => Some(0xDE),
        _ => None,
    }
}

/// Classifies Windows microphone consent registry values matching `src/oskit/windows.py:353-367`.
///
/// - `Some(true)` if any of `(user, non_packaged, machine)` is `"Deny"`.
/// - `Some(false)` if both `user` and `non_packaged` are `"Allow"`.
/// - `None` otherwise (unreadable or partial).
#[must_use]
pub fn classify_mic_consent(
    user: Option<&str>,
    non_packaged: Option<&str>,
    machine: Option<&str>,
) -> Option<bool> {
    if [user, non_packaged, machine].contains(&Some("Deny")) {
        return Some(true);
    }
    if user == Some("Allow") && non_packaged == Some("Allow") {
        return Some(false);
    }
    None
}

/// Returns `true` if the DXGI adapter is a software/basic-render adapter (`DXGI_ADAPTER_FLAG_SOFTWARE`
/// bit `0x2` or Microsoft Basic Render Driver vendor `0x1414`).
#[must_use]
pub const fn is_software_adapter(vendor_id: u32, flags: u32) -> bool {
    (flags & 2) != 0 || vendor_id == 0x1414
}

/// Returns `true` if any hardware adapter has at least `min_vram_bytes` of dedicated VRAM.
#[must_use]
pub fn has_discrete_gpu(adapters: &[GpuAdapter], min_vram_bytes: u64) -> bool {
    adapters.iter().any(|adapter| {
        !is_software_adapter(adapter.vendor_id, 0)
            && adapter.dedicated_video_memory_bytes >= min_vram_bytes
    })
}

/// Linearly resamples mono `f32` audio from `src_rate` Hz to `dst_rate` Hz.
#[must_use]
pub fn linear_resample_mono(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if input.is_empty() || src_rate == 0 || dst_rate == 0 {
        return Vec::new();
    }
    if src_rate == dst_rate {
        return input.to_vec();
    }
    let out_len = ((input.len() as f64 * f64::from(dst_rate)) / f64::from(src_rate)).round();
    let out_len = (out_len as usize).max(usize::from(input.len() == 1));
    if out_len == 0 {
        return Vec::new();
    }
    if input.len() == 1 {
        return vec![input[0]; out_len];
    }

    let mut out = Vec::with_capacity(out_len);
    let last_idx = input.len() - 1;
    let ratio = f64::from(src_rate) / f64::from(dst_rate);
    for i in 0..out_len {
        let src_pos = (i as f64) * ratio;
        let idx = (src_pos.floor() as usize).min(last_idx);
        let frac = (src_pos - idx as f64) as f32;
        let next = if idx < last_idx {
            input[idx + 1]
        } else {
            input[idx]
        };
        out.push(input[idx] + (next - input[idx]) * frac);
    }
    out
}

/// Downmixes interleaved `f32` PCM with `channels` channels to mono `f32`.
#[must_use]
pub fn downmix_f32_to_mono(input: &[f32], channels: usize) -> Vec<f32> {
    if channels == 0 || input.is_empty() {
        return Vec::new();
    }
    if channels == 1 {
        return input.to_vec();
    }
    let inv = 1.0 / (channels as f32);
    input
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() * inv)
        .collect()
}

/// Downmixes interleaved `i16` PCM with `channels` channels to mono `f32` in `[-1.0, 1.0]`.
#[must_use]
pub fn downmix_i16_to_mono(input: &[i16], channels: usize) -> Vec<f32> {
    if channels == 0 || input.is_empty() {
        return Vec::new();
    }
    let scale = 1.0 / (32768.0 * channels as f32);
    if channels == 1 {
        return input.iter().map(|&s| f32::from(s) * scale).collect();
    }
    input
        .chunks_exact(channels)
        .map(|frame| {
            let sum: f32 = frame.iter().map(|&s| f32::from(s)).sum();
            sum * scale
        })
        .collect()
}

/// Encodes `text` as a NUL-terminated UTF-16 buffer.
#[must_use]
pub fn utf16_with_nul(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Decodes a UTF-16 slice up to the first NUL (`0`) code unit, replacing invalid surrogates.
#[must_use]
pub fn from_utf16_nul(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_hotkey_valid_combos() {
        let alt_q = parse_hotkey_combo("alt+q").unwrap();
        assert_eq!(alt_q.vk, u16::from(b'Q'));
        assert!(alt_q.modifiers.alt);
        assert!(!alt_q.modifiers.ctrl);
        assert!(!alt_q.modifiers.shift);
        assert!(!alt_q.modifiers.win);

        let cs_f9 = parse_hotkey_combo("  Control + SHIFT + F9 ").unwrap();
        assert_eq!(cs_f9.vk, 0x78); // VK_F9
        assert!(cs_f9.modifiers.ctrl);
        assert!(cs_f9.modifiers.shift);
        assert!(!cs_f9.modifiers.alt);

        let win_space = parse_hotkey_combo("win+space").unwrap();
        assert_eq!(win_space.vk, 0x20);
        assert!(win_space.modifiers.win);

        let cmd_ret = parse_hotkey_combo("super+return").unwrap();
        assert_eq!(cmd_ret.vk, 0x0D);
        assert!(cmd_ret.modifiers.win);

        let f12 = parse_hotkey_combo("f12").unwrap();
        assert_eq!(f12.vk, 0x7B);
        assert_eq!(f12.modifiers.bits(), 0);

        let esc = parse_hotkey_combo("ctrl+esc").unwrap();
        assert_eq!(esc.vk, 0x1B);
        assert!(esc.modifiers.ctrl);
    }

    #[test]
    fn parse_hotkey_rejects_invalid() {
        assert!(parse_hotkey_combo("").is_err());
        assert!(parse_hotkey_combo("   ").is_err());
        assert!(parse_hotkey_combo("ctrl+alt").is_err());
        assert!(parse_hotkey_combo("win").is_err());
        assert!(parse_hotkey_combo("alt+").is_err());
        assert!(parse_hotkey_combo("+q").is_err());
        assert!(parse_hotkey_combo("a+b").is_err());
        assert!(parse_hotkey_combo("alt+nonexistent_key").is_err());
    }

    #[test]
    fn mic_consent_classification() {
        assert_eq!(
            classify_mic_consent(Some("Allow"), Some("Allow"), Some("Allow")),
            Some(false)
        );
        assert_eq!(
            classify_mic_consent(Some("Allow"), Some("Allow"), None),
            Some(false)
        );
        assert_eq!(
            classify_mic_consent(Some("Deny"), Some("Allow"), Some("Allow")),
            Some(true)
        );
        assert_eq!(
            classify_mic_consent(Some("Allow"), Some("Deny"), Some("Allow")),
            Some(true)
        );
        assert_eq!(
            classify_mic_consent(Some("Allow"), Some("Allow"), Some("Deny")),
            Some(true)
        );
        assert_eq!(classify_mic_consent(Some("Allow"), None, None), None);
        assert_eq!(classify_mic_consent(None, None, None), None);
    }

    #[test]
    fn gpu_helpers() {
        assert!(is_software_adapter(0x1414, 0));
        assert!(is_software_adapter(0x10DE, 2));
        assert!(!is_software_adapter(0x10DE, 0));

        let min_2gb = 2 * 1024 * 1024 * 1024;
        let igpu = GpuAdapter {
            name: "Intel UHD".to_owned(),
            vendor_id: 0x8086,
            dedicated_video_memory_bytes: 128 * 1024 * 1024,
        };
        let dgpu = GpuAdapter {
            name: "NVIDIA RTX 3060".to_owned(),
            vendor_id: 0x10DE,
            dedicated_video_memory_bytes: 12 * 1024 * 1024 * 1024,
        };
        assert!(!has_discrete_gpu(std::slice::from_ref(&igpu), min_2gb));
        assert!(has_discrete_gpu(&[igpu, dgpu], min_2gb));
    }

    #[test]
    fn audio_resample_and_downmix() {
        assert!(linear_resample_mono(&[], 48000, 16000).is_empty());
        assert_eq!(linear_resample_mono(&[0.5, -0.5], 16000, 16000), vec![0.5, -0.5]);

        let down = linear_resample_mono(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0], 48000, 16000);
        assert_eq!(down.len(), 2);
        assert!((down[0] - 0.0).abs() < 1e-6);
        assert!((down[1] - 3.0).abs() < 1e-6);

        let mono_f32 = downmix_f32_to_mono(&[1.0, -1.0, 0.5, 0.5], 2);
        assert_eq!(mono_f32, vec![0.0, 0.5]);

        let mono_i16 = downmix_i16_to_mono(&[16384, -16384, 32767, 32767], 2);
        assert!((mono_i16[0] - 0.0).abs() < 1e-5);
        assert!((mono_i16[1] - (32767.0 / 32768.0)).abs() < 1e-5);
    }

    #[test]
    fn utf16_roundtrip() {
        let s = "Eqho 🚀";
        let buf = utf16_with_nul(s);
        assert_eq!(buf.last().copied(), Some(0));
        assert_eq!(from_utf16_nul(&buf), s);

        let with_garbage = [b'A' as u16, b'B' as u16, 0, b'C' as u16];
        assert_eq!(from_utf16_nul(&with_garbage), "AB");
    }
}
