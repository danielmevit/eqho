//! Wayland (`wl-copy`/`wl-paste`) and X11 (`xclip`/`xsel`) clipboard service.

use std::io::Write;
use std::process::{Command, Stdio};

use eqho_platform::{ClipboardService, PlatformError};

use crate::pure::is_wayland_session;

/// Cross-session Linux clipboard supporting Wayland and X11 without daemon hang traps.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxClipboardService;

impl LinuxClipboardService {
    fn is_wayland() -> bool {
        let wl = std::env::var("WAYLAND_DISPLAY").ok();
        let st = std::env::var("XDG_SESSION_TYPE").ok();
        is_wayland_session(wl.as_deref(), st.as_deref())
    }

    fn run_read(bin: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(bin)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn run_write(bin: &str, args: &[&str], text: &str) -> Result<(), PlatformError> {
        // Redirecting stdout & stderr to null is critical for `xclip`/`wl-copy`, which fork
        // a background selection owner that would otherwise hold stdout open and block `.output()`.
        let mut child = Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| PlatformError(format!("{bin} failed to start: {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| PlatformError(format!("{bin} stdin write failed: {e}")))?;
        }
        let status = child
            .wait()
            .map_err(|e| PlatformError(format!("{bin} wait failed: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError(format!("{bin} exited with {status}")))
        }
    }
}

impl ClipboardService for LinuxClipboardService {
    fn get_text(&self) -> Option<String> {
        if Self::is_wayland()
            && let Some(t) = Self::run_read("wl-paste", &["--no-newline", "--type", "text/plain"])
        {
            return Some(t);
        }
        Self::run_read("xclip", &["-selection", "clipboard", "-out"])
            .or_else(|| Self::run_read("xsel", &["--clipboard", "--output"]))
            .or_else(|| Self::run_read("wl-paste", &["--no-newline"]))
    }

    fn set_text(&self, text: &str, _private: bool) -> Result<(), PlatformError> {
        if Self::is_wayland()
            && Self::run_write("wl-copy", &["--type", "text/plain;charset=utf-8"], text).is_ok()
        {
            return Ok(());
        }
        if Self::run_write("xclip", &["-selection", "clipboard", "-in"], text).is_ok() {
            return Ok(());
        }
        if Self::run_write("xsel", &["--clipboard", "--input"], text).is_ok() {
            return Ok(());
        }
        Self::run_write("wl-copy", &["--type", "text/plain;charset=utf-8"], text)
    }
}
