//! Linux implementations of the frozen [`damt_platform`] contracts.
//!
//! Pure helpers in [`pure`] compile and run unit tests on every target; live Linux
//! implementations are gated behind `#[cfg(target_os = "linux")]` and exposed via
//! [`LinuxPlatform`].

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(missing_debug_implementations)]

pub mod pure;

pub use pure::{
    autostart_file_path, build_autostart_desktop_entry, parse_enable_animations,
    parse_portal_color_scheme, resolve_autostart_exec_path,
};

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{
    LinuxActiveScreen, LinuxAutostart, LinuxMotion, LinuxNotifier, LinuxPlatform, LinuxTheme,
};
