//! macOS implementations of the frozen [`damt_platform`] contracts.
//!
//! Pure helpers in [`pure`] compile and run unit tests on every target; live macOS
//! implementations are gated behind `#[cfg(target_os = "macos")]` and exposed via
//! [`MacPlatform`].

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(missing_debug_implementations)]

pub mod pure;

pub use pure::{
    build_launch_agent_plist, escape_applescript_string, launch_agent_plist_path,
    parse_apple_interface_style, parse_reduce_motion,
};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::{MacActiveScreen, MacAutostart, MacMotion, MacNotifier, MacPlatform, MacTheme};
