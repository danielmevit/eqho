//! Pure-Rust helpers for `eqho-platform-linux`, compiled and unit-tested on all targets.

use eqho_platform::PlatformError;

/// Modifier bitmask for Linux X11 global hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LinuxHotkeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

impl LinuxHotkeyModifiers {
    /// Convert to standard X11 modifier mask (`ControlMask=4`, `ShiftMask=1`, `Mod1Mask=8`, `Mod4Mask=64`).
    #[must_use]
    pub const fn x11_mask(self) -> u32 {
        let mut m = 0u32;
        if self.shift {
            m |= 1 << 0; // ShiftMask
        }
        if self.ctrl {
            m |= 1 << 2; // ControlMask
        }
        if self.alt {
            m |= 1 << 3; // Mod1Mask
        }
        if self.super_key {
            m |= 1 << 6; // Mod4Mask
        }
        m
    }
}

/// Parsed Linux hotkey combination: X11 KeySym plus required modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParsedLinuxHotkey {
    pub keysym: u32,
    pub modifiers: LinuxHotkeyModifiers,
}

/// Parse a hotkey string like `"alt+q"`, `"ctrl+shift+f9"`, or `"win+space"` into an X11 KeySym.
pub fn parse_hotkey_combo(raw: &str) -> Result<ParsedLinuxHotkey, PlatformError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(PlatformError("empty hotkey combination".to_owned()));
    }
    let mut modifiers = LinuxHotkeyModifiers::default();
    let mut trigger: Option<u32> = None;

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
            "alt" | "alt_l" | "alt_r" | "alt_gr" | "lalt" | "ralt" | "menu" | "option" => {
                modifiers.alt = true;
            }
            "shift" | "shift_l" | "shift_r" | "lshift" | "rshift" => modifiers.shift = true,
            "win" | "windows" | "super" | "cmd" | "cmd_l" | "cmd_r" | "meta" | "lwin"
            | "rwin" => modifiers.super_key = true,
            other => {
                if trigger.is_some() {
                    return Err(PlatformError(format!(
                        "multiple trigger keys in hotkey: {raw:?}"
                    )));
                }
                trigger = Some(parse_x11_keysym(other).ok_or_else(|| {
                    PlatformError(format!("unknown key {other:?} in hotkey {raw:?}"))
                })?);
            }
        }
    }

    let keysym =
        trigger.ok_or_else(|| PlatformError(format!("hotkey has no trigger key: {raw:?}")))?;
    Ok(ParsedLinuxHotkey { keysym, modifiers })
}

fn parse_x11_keysym(token: &str) -> Option<u32> {
    if token.len() == 1 {
        let b = token.as_bytes()[0];
        if b.is_ascii_lowercase() || b.is_ascii_digit() {
            return Some(u32::from(b));
        }
    }
    if let Some(num) = token.strip_prefix('f')
        && let Ok(n) = num.parse::<u32>()
        && (1..=24).contains(&n)
    {
        // XK_F1 = 0xFFBE
        return Some(0xFFBE + (n - 1));
    }
    match token {
        "space" | "spacebar" => Some(0x0020),
        "enter" | "return" => Some(0xFF0D),
        "esc" | "escape" => Some(0xFF1B),
        "tab" => Some(0xFF09),
        "backspace" | "back" => Some(0xFF08),
        "delete" | "del" => Some(0xFFFF),
        "insert" | "ins" => Some(0xFF63),
        "home" => Some(0xFF50),
        "end" => Some(0xFF57),
        "pageup" | "page_up" | "pgup" => Some(0xFF55),
        "pagedown" | "page_down" | "pgdn" => Some(0xFF56),
        "left" => Some(0xFF51),
        "up" => Some(0xFF52),
        "right" => Some(0xFF53),
        "down" => Some(0xFF54),
        "minus" | "-" => Some(0x002D),
        "equal" | "equals" | "=" => Some(0x003D),
        "comma" | "," => Some(0x002C),
        "period" | "dot" | "." => Some(0x002E),
        "slash" | "/" => Some(0x002F),
        "semicolon" | ";" => Some(0x003B),
        "grave" | "backtick" | "`" => Some(0x0060),
        _ => None,
    }
}

/// Parse available physical RAM in MiB from Linux `/proc/meminfo`.
#[must_use]
pub fn parse_proc_meminfo_available_mb(contents: &str) -> Option<u64> {
    let mut mem_avail_kb: Option<u64> = None;
    let mut mem_free_kb = 0u64;
    let mut buffers_kb = 0u64;
    let mut cached_kb = 0u64;
    let mut saw_fallback = false;

    for line in contents.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let val_kb = rest
            .split_whitespace()
            .next()
            .and_then(|n| n.parse::<u64>().ok());
        let Some(kb) = val_kb else {
            continue;
        };
        match key.trim() {
            "MemAvailable" => mem_avail_kb = Some(kb),
            "MemFree" => {
                mem_free_kb = kb;
                saw_fallback = true;
            }
            "Buffers" => buffers_kb = kb,
            "Cached" => cached_kb = kb,
            _ => {}
        }
    }

    let kb = mem_avail_kb.or_else(|| {
        if saw_fallback {
            Some(mem_free_kb.saturating_add(buffers_kb).saturating_add(cached_kb))
        } else {
            None
        }
    })?;
    Some(kb / 1024)
}

/// Parse `wpctl get-volume @DEFAULT_AUDIO_SINK@` output (e.g. `"Volume: 0.65"` or `"Volume: 0.40 [MUTED]"`).
#[must_use]
pub fn parse_wpctl_volume(output: &str) -> Option<f32> {
    for tok in output.split_whitespace() {
        if let Ok(v) = tok.parse::<f32>()
            && v.is_finite()
            && v >= 0.0
        {
            return Some(v.clamp(0.0, 1.5));
        }
    }
    None
}

/// Parse `pactl get-sink-volume @DEFAULT_SINK@` output (e.g. `"Volume: front-left: 49152 /  75% / -7.50 dB"`).
#[must_use]
pub fn parse_pactl_volume(output: &str) -> Option<f32> {
    for tok in output.split_whitespace() {
        if let Some(pct_str) = tok.strip_suffix('%')
            && let Ok(pct) = pct_str.parse::<f32>()
            && pct.is_finite()
            && pct >= 0.0
        {
            return Some((pct / 100.0).clamp(0.0, 1.5));
        }
    }
    None
}

/// Parse `pactl list sources` output into `(pulse_source_name, friendly_description)` pairs,
/// automatically excluding `.monitor` output-loopback sources so users only see real microphones.
#[must_use]
pub fn parse_pactl_sources(output: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut cur_name: Option<String> = None;
    let mut cur_desc: Option<String> = None;

    let flush = |out: &mut Vec<(String, String)>,
                 name: &mut Option<String>,
                 desc: &mut Option<String>| {
        if let Some(n) = name.take() {
            let d = desc.take().unwrap_or_else(|| n.clone());
            if !n.ends_with(".monitor") && !d.to_ascii_lowercase().starts_with("monitor of ") {
                out.push((n, d));
            }
        } else {
            *desc = None;
        }
    };

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Source #") {
            flush(&mut out, &mut cur_name, &mut cur_desc);
        } else if let Some(val) = trimmed.strip_prefix("Name:") {
            cur_name = Some(val.trim().to_owned());
        } else if let Some(val) = trimmed.strip_prefix("Description:") {
            cur_desc = Some(val.trim().to_owned());
        }
    }
    flush(&mut out, &mut cur_name, &mut cur_desc);
    out
}

/// Return `true` when running inside a Wayland desktop session.
#[must_use]
pub fn is_wayland_session(wayland_display: Option<&str>, xdg_session_type: Option<&str>) -> bool {
    wayland_display.is_some_and(|s| !s.trim().is_empty())
        || xdg_session_type.is_some_and(|s| s.trim().eq_ignore_ascii_case("wayland"))
}

/// Return `true` for CPU/software Vulkan renderers (`VK_PHYSICAL_DEVICE_TYPE_CPU == 4`, llvmpipe, etc.).
#[must_use]
pub fn is_software_vulkan_device(device_type: u32, name: &str) -> bool {
    if device_type == 4 {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    lower.contains("llvmpipe")
        || lower.contains("lavapipe")
        || lower.contains("swiftshader")
        || lower.contains("softpipe")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_hotkeys_to_x11_keysyms() {
        let alt_q = parse_hotkey_combo("alt+q").unwrap();
        assert_eq!(alt_q.keysym, u32::from(b'q'));
        assert!(alt_q.modifiers.alt && !alt_q.modifiers.ctrl);
        assert_eq!(alt_q.modifiers.x11_mask(), 8);

        let f9 = parse_hotkey_combo("ctrl+shift+f9").unwrap();
        assert_eq!(f9.keysym, 0xFFBE + 8);
        assert!(f9.modifiers.ctrl && f9.modifiers.shift);

        assert!(parse_hotkey_combo("").is_err());
        assert!(parse_hotkey_combo("alt+").is_err());
        assert!(parse_hotkey_combo("alt+ctrl").is_err());
    }

    #[test]
    fn parses_proc_meminfo_available() {
        let sample = "\
MemTotal:       16384000 kB
MemFree:         2048000 kB
MemAvailable:    8192000 kB
Buffers:          512000 kB
";
        assert_eq!(parse_proc_meminfo_available_mb(sample), Some(8000));
        let fallback = "MemFree: 1024000 kB\nBuffers: 512000 kB\nCached: 512000 kB\n";
        assert_eq!(parse_proc_meminfo_available_mb(fallback), Some(2000));
    }

    #[test]
    fn parses_wpctl_and_pactl_volumes() {
        assert!((parse_wpctl_volume("Volume: 0.75\n").unwrap() - 0.75).abs() < 1e-5);
        assert!((parse_wpctl_volume("Volume: 0.40 [MUTED]").unwrap() - 0.40).abs() < 1e-5);
        assert!(
            (parse_pactl_volume("Volume: front-left: 49152 /  75% / -7.50 dB").unwrap() - 0.75)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn parses_pactl_sources_filtering_monitors() {
        let raw = "\
Source #0
\tName: alsa_output.pci.analog-stereo.monitor
\tDescription: Monitor of Built-in Audio Analog Stereo
Source #1
\tName: alsa_input.usb-Blue_Yeti-00.mono-fallback
\tDescription: Blue Yeti Stereo Microphone
";
        let list = parse_pactl_sources(raw);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "alsa_input.usb-Blue_Yeti-00.mono-fallback");
        assert_eq!(list[0].1, "Blue Yeti Stereo Microphone");
    }

    #[test]
    fn detects_software_vulkan_and_wayland() {
        assert!(is_software_vulkan_device(4, "llvmpipe (LLVM 17)"));
        assert!(is_software_vulkan_device(0, "SwiftShader Device"));
        assert!(!is_software_vulkan_device(2, "NVIDIA GeForce RTX 3060"));
        assert!(is_wayland_session(Some("wayland-0"), None));
        assert!(is_wayland_session(None, Some("wayland")));
        assert!(!is_wayland_session(None, Some("x11")));
    }
}
