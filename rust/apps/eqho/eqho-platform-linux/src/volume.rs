//! Master output volume ducking via PipeWire (`wpctl`) or PulseAudio (`pactl`).

use std::process::{Command, Stdio};

use eqho_platform::VolumeControl;

use crate::pure::{parse_pactl_volume, parse_wpctl_volume};

/// PipeWire / PulseAudio master sink volume control.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxVolumeControl;

impl LinuxVolumeControl {
    fn has_tool(bin: &str, probe_arg: &str) -> bool {
        Command::new(bin)
            .arg(probe_arg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

impl VolumeControl for LinuxVolumeControl {
    fn is_supported(&self) -> bool {
        // WirePlumber's `wpctl` does not support `--version` (exits non-zero); `--help` exits 0.
        Self::has_tool("wpctl", "--help") || Self::has_tool("pactl", "--version")
    }

    fn get_volume(&self) -> Option<f32> {
        if let Ok(out) = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            && out.status.success()
            && let Some(v) = parse_wpctl_volume(&String::from_utf8_lossy(&out.stdout))
        {
            return Some(v.clamp(0.0, 1.0));
        }
        if let Ok(out) = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            && out.status.success()
            && let Some(v) = parse_pactl_volume(&String::from_utf8_lossy(&out.stdout))
        {
            return Some(v.clamp(0.0, 1.0));
        }
        None
    }

    fn set_volume(&self, level: f32) -> bool {
        let clamped = level.clamp(0.0, 1.0);
        let wp_arg = format!("{clamped:.2}");
        if Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &wp_arg])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            return true;
        }
        let pct_arg = format!("{}%", (clamped * 100.0).round() as u32);
        Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &pct_arg])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn set_mute(&self, muted: bool) -> bool {
        let wp_arg = if muted { "1" } else { "0" };
        if Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", wp_arg])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            return true;
        }
        let pa_arg = if muted { "1" } else { "0" };
        Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", pa_arg])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_control_detects_wpctl_or_pactl_when_installed() {
        let ctrl = LinuxVolumeControl;
        if LinuxVolumeControl::has_tool("wpctl", "--help")
            || LinuxVolumeControl::has_tool("pactl", "--version")
        {
            assert!(ctrl.is_supported());
        }
    }
}
