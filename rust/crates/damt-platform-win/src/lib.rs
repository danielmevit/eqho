//! Windows implementations of the frozen [`damt_platform`] contracts.
//!
//! Everything in [`pure`] is plain math and mapping, compiled and tested on every target. The Win32
//! implementations live behind `#[cfg(windows)]`; each one never panics, returning the contract's documented
//! "unknown" value on failure.

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod pure;

pub use pure::{
    AutostartCommand, AutostartCommandError, AutostartKind, AutostartPathSelector, ExStyle,
    FocusStateMapper, IdleMath, MotionMath, MsixVersionError, RawStoreLicense, RectMath,
    RegistryValue, SessionEventMapper, StartupTaskToggleState, StoreLicenseKind,
    StoreLicenseMapper, StorePurchaseOutcome, TaskbarThemeMath, ToastXml, aumid_registry_subkey,
    format_msix_version, game_bar_exe_path, is_game_bar_exe_value_name, notify_icon_path,
};

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{
    WinActiveScreen, WinAutostart, WinFocus, WinForeground, WinIdle, WinMotion, WinNotifier,
    WinPlatform, WinSessionEvents, WinTaskbarTheme, is_packaged_app,
};

#[cfg(all(windows, feature = "store"))]
pub use windows::{query_app_license, query_app_license_async, request_purchase_async};
