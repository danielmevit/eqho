//! Pure ports of the C# platform helpers: no Windows calls, so they are unit-tested on every target.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::Duration;

use damt_platform::{FocusState, ScreenRect, SessionEvent};
use thiserror::Error;

/// Wrap-safe idle-time math (C# `IdleMath`).
#[derive(Debug, Default, Clone, Copy)]
pub struct IdleMath;

impl IdleMath {
    /// Milliseconds between `now_ticks` and `last_input_ticks`, wrapping like the 32-bit tick counter.
    pub fn compute(now_ticks: u32, last_input_ticks: u32) -> Duration {
        Duration::from_millis(u64::from(now_ticks.wrapping_sub(last_input_ticks)))
    }
}

/// Mapping for the Windows "Animation effects" preference (C# `MotionMath`).
#[derive(Debug, Default, Clone, Copy)]
pub struct MotionMath;

impl MotionMath {
    /// False when the call failed (unknown means animations allowed), true only when the value is 0
    /// ("Animation effects" off).
    pub fn reduced(call_succeeded: bool, client_area_animation: i32) -> bool {
        call_succeeded && client_area_animation == 0
    }
}

/// A registry value as far as the taskbar helper cares: a DWORD or anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryValue {
    /// A `REG_DWORD` value.
    Dword(u32),
    /// Any other registry type (string, QWORD, binary, ...).
    Other,
}

/// Mapping for the Windows taskbar theme registry value (C# `TaskbarThemeMath`).
#[derive(Debug, Default, Clone, Copy)]
pub struct TaskbarThemeMath;

impl TaskbarThemeMath {
    /// Maps the `SystemUsesLightTheme` DWORD: 1 = light, 0 = dark. Missing, any other value or any
    /// non-DWORD type means unknown.
    pub fn from_registry_value(value: Option<RegistryValue>) -> Option<bool> {
        match value {
            Some(RegistryValue::Dword(1)) => Some(true),
            Some(RegistryValue::Dword(0)) => Some(false),
            _ => None,
        }
    }
}

/// Maps `SHQueryUserNotificationState` results to [`FocusState`] (C# `FocusStateMapper`).
#[derive(Debug, Default, Clone, Copy)]
pub struct FocusStateMapper;

impl FocusStateMapper {
    /// `hresult` is the call's HRESULT; negative means the query failed.
    pub fn map(hresult: i32, quns: i32) -> FocusState {
        if hresult < 0 {
            return FocusState::Unknown;
        }

        match quns {
            1 => FocusState::NotPresent,
            2 | 3 | 7 => FocusState::FullscreenApp,
            4 => FocusState::Presentation,
            5 => FocusState::Normal,
            6 => FocusState::QuietTime,
            _ => FocusState::Unknown,
        }
    }
}

/// Maps Windows session and power notifications to [`SessionEvent`] (C# `SessionEventMapper`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SessionEventMapper;

impl SessionEventMapper {
    /// `WM_WTSSESSION_CHANGE` reason: 7 = lock, 8 = unlock, 6 = logoff.
    pub fn from_session_switch(reason: i32) -> Option<SessionEvent> {
        match reason {
            7 => Some(SessionEvent::Locked),
            8 => Some(SessionEvent::Unlocked),
            6 => Some(SessionEvent::LoggingOff),
            _ => None,
        }
    }

    /// Power mode: 1 = resume, 3 = suspend.
    pub fn from_power_mode(mode: i32) -> Option<SessionEvent> {
        match mode {
            1 => Some(SessionEvent::Resumed),
            3 => Some(SessionEvent::Suspending),
            _ => None,
        }
    }
}

/// Converts Win32 `RECT` bounds to [`ScreenRect`] (C# `RectMath`).
#[derive(Debug, Default, Clone, Copy)]
pub struct RectMath;

impl RectMath {
    /// Left/top/right/bottom to `x`/`y`/`width`/`height`.
    pub fn to_screen_rect(left: i32, top: i32, right: i32, bottom: i32) -> ScreenRect {
        ScreenRect {
            x: left,
            y: top,
            width: right.wrapping_sub(left),
            height: bottom.wrapping_sub(top),
        }
    }
}

/// Why an autostart command could not be built.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum AutostartCommandError {
    /// The executable path was empty or only whitespace.
    #[error("the path must not be empty or whitespace")]
    EmptyPath,
    /// The executable path contained a quote character.
    #[error("the path must not contain quote characters")]
    ContainsQuote,
}

/// Builds the command line stored in the Windows Run key (C# `AutostartCommand`).
#[derive(Debug, Default, Clone, Copy)]
pub struct AutostartCommand;

impl AutostartCommand {
    /// Quotes the executable path and appends ` --autostart`.
    pub fn build(exe_path: &str) -> Result<String, AutostartCommandError> {
        if exe_path.trim().is_empty() {
            return Err(AutostartCommandError::EmptyPath);
        }

        if exe_path.contains('"') {
            return Err(AutostartCommandError::ContainsQuote);
        }

        Ok(format!("\"{exe_path}\" --autostart"))
    }
}

/// Extended-window-style bit math (C# `ExStyle`).
#[derive(Debug, Default, Clone, Copy)]
pub struct ExStyle;

impl ExStyle {
    const WS_EX_TRANSPARENT: i32 = 0x20;
    const WS_EX_TOOLWINDOW: i32 = 0x80;
    const WS_EX_LAYERED: i32 = 0x0008_0000;
    const WS_EX_NOACTIVATE: i32 = 0x0800_0000;

    /// Adds the click-through, tool-window, layered and no-activate bits to `current`.
    pub fn click_through(current: i32) -> i32 {
        current
            | Self::WS_EX_TRANSPARENT
            | Self::WS_EX_TOOLWINDOW
            | Self::WS_EX_LAYERED
            | Self::WS_EX_NOACTIVATE
    }
}

/// Escapes XML text and builds `ToastGeneric` payloads without raw string interpolation.
#[derive(Debug, Default, Clone, Copy)]
pub struct ToastXml;

impl ToastXml {
    /// Escapes `&`, `<`, `>`, `"`, and `'` and strips disallowed XML 1.0 C0 control characters.
    #[must_use]
    pub fn escape(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            match ch {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                '\t' | '\n' | '\r' => out.push(ch),
                c if (c as u32) < 0x20 => {}
                c => out.push(c),
            }
        }
        out
    }

    /// Builds a `ToastGeneric` XML template containing escaped `title` and `body`.
    #[must_use]
    pub fn build_generic(title: &str, body: &str) -> String {
        let escaped_title = Self::escape(title);
        let escaped_body = Self::escape(body);
        format!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>{escaped_title}</text><text>{escaped_body}</text></binding></visual></toast>"
        )
    }
}

/// Returns the per-user registry subkey under `HKEY_CURRENT_USER` for an AppUserModelID.
#[must_use]
pub fn aumid_registry_subkey(aumid: &str) -> String {
    format!(r"Software\Classes\AppUserModelId\{aumid}")
}

/// Resolves `%LOCALAPPDATA%\<app_dir_name>\notify-icon.png` (falling back to
/// `%USERPROFILE%\AppData\Local`), returning `None` when neither environment value is present.
#[must_use]
pub fn notify_icon_path(
    app_dir_name: &str,
    local_app_data: Option<&OsStr>,
    user_profile: Option<&OsStr>,
) -> Option<PathBuf> {
    let base = match local_app_data {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => match user_profile {
            Some(value) if !value.is_empty() => PathBuf::from(value).join("AppData").join("Local"),
            _ => return None,
        },
    };
    Some(base.join(app_dir_name).join("notify-icon.png"))
}

/// Whether a Game Bar `Children` value name stores an executable path.
///
/// `MatchedExeFullPath` is the value Windows writes. Names that contain both `exe` and `path`
/// (for example `OriginalExePath`) are treated the same way. `ExeParentDirectory` is not.
#[must_use]
pub fn is_game_bar_exe_value_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.eq_ignore_ascii_case("MatchedExeFullPath") {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.contains("exe") && lower.contains("path")
}

/// Returns the stored executable path when `name` is a game-bar exe value and `value` points at
/// an `.exe`. Quotes and surrounding whitespace are stripped. Anything else is ignored.
#[must_use]
pub fn game_bar_exe_path(name: &str, value: &str) -> Option<String> {
    if !is_game_bar_exe_value_name(name) {
        return None;
    }
    let trimmed = value.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }
    let file = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    if file.to_ascii_lowercase().ends_with(".exe") {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

/// Which Windows autostart mechanism applies to the running process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartKind {
    /// Unpackaged Win32 app: per-user `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
    RegistryRunKey,
    /// Packaged MSIX app with a declared `desktop:StartupTask`.
    PackagedStartupTask,
    /// Packaged MSIX app without a configured `StartupTask` id (Run key must not be used).
    Unsupported,
}

/// Pure startup-path selection and `GetCurrentPackageFullName` error classification.
#[derive(Debug, Default, Clone, Copy)]
pub struct AutostartPathSelector;

impl AutostartPathSelector {
    /// Win32 `APPMODEL_ERROR_NO_PACKAGE` (15700).
    pub const APPMODEL_ERROR_NO_PACKAGE: u32 = 15_700;
    /// Win32 `ERROR_SUCCESS` (0).
    pub const ERROR_SUCCESS: u32 = 0;
    /// Win32 `ERROR_INSUFFICIENT_BUFFER` (122).
    pub const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

    /// Returns `true` when `GetCurrentPackageFullName` indicates a packaged process
    /// (i.e. not `APPMODEL_ERROR_NO_PACKAGE`, specifically `ERROR_SUCCESS` or
    /// `ERROR_INSUFFICIENT_BUFFER`).
    #[must_use]
    pub const fn is_packaged_error_code(win32_error: u32) -> bool {
        win32_error != Self::APPMODEL_ERROR_NO_PACKAGE
            && (win32_error == Self::ERROR_SUCCESS
                || win32_error == Self::ERROR_INSUFFICIENT_BUFFER)
    }

    /// Chooses the autostart backend for `(is_packaged, has_startup_task_id)`.
    #[must_use]
    pub const fn select(is_packaged: bool, has_startup_task_id: bool) -> AutostartKind {
        if !is_packaged {
            AutostartKind::RegistryRunKey
        } else if has_startup_task_id {
            AutostartKind::PackagedStartupTask
        } else {
            AutostartKind::Unsupported
        }
    }
}

/// Pure mirror of `Windows.ApplicationModel.StartupTaskState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupTaskToggleState {
    /// `StartupTaskState::Disabled` (0).
    Disabled,
    /// `StartupTaskState::DisabledByUser` (1).
    DisabledByUser,
    /// `StartupTaskState::Enabled` (2).
    Enabled,
    /// `StartupTaskState::DisabledByPolicy` (3).
    DisabledByPolicy,
    /// `StartupTaskState::EnabledByPolicy` (4).
    EnabledByPolicy,
    /// Any unrecognized WinRT enum discriminant.
    Unknown(i32),
}

impl StartupTaskToggleState {
    /// Converts the raw WinRT `StartupTaskState` integer discriminant.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Disabled,
            1 => Self::DisabledByUser,
            2 => Self::Enabled,
            3 => Self::DisabledByPolicy,
            4 => Self::EnabledByPolicy,
            other => Self::Unknown(other),
        }
    }

    /// Maps the startup task state to the UI toggle ("Start with system").
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled | Self::EnabledByPolicy)
    }

    /// Validates the state returned by `StartupTask::RequestEnableAsync`.
    pub fn check_after_enable(self) -> Result<(), String> {
        match self {
            Self::Enabled | Self::EnabledByPolicy => Ok(()),
            Self::DisabledByUser => {
                Err("startup task is disabled by user in Windows Startup settings".to_owned())
            }
            Self::DisabledByPolicy => Err("startup task is disabled by group policy".to_owned()),
            Self::Disabled => Err("startup task remained disabled".to_owned()),
            Self::Unknown(code) => Err(format!("startup task returned unexpected state ({code})")),
        }
    }

    /// Validates the state observed after `StartupTask::Disable`.
    pub fn check_after_disable(self) -> Result<(), String> {
        match self {
            Self::Disabled | Self::DisabledByUser | Self::DisabledByPolicy => Ok(()),
            Self::EnabledByPolicy => {
                Err("startup task is enabled by group policy and cannot be disabled".to_owned())
            }
            Self::Enabled => Err("startup task remained enabled".to_owned()),
            Self::Unknown(code) => Err(format!("startup task returned unexpected state ({code})")),
        }
    }
}

/// Why a Cargo version string could not be formatted as a four-part MSIX version.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MsixVersionError {
    /// Input was empty or contained non-numeric parts.
    #[error("invalid MSIX version '{0}': expected major.minor.patch[.revision]")]
    InvalidFormat(String),
    /// A version component exceeded `u16::MAX` (65535), which MSIX forbids.
    #[error("invalid MSIX version '{0}': components must be <= 65535")]
    ComponentOutOfRange(String),
}

/// Formats a Cargo SemVer string (e.g. `"0.2.2-dev"`) as a four-part MSIX version (`"0.2.2.0"`).
pub fn format_msix_version(cargo_version: &str) -> Result<String, MsixVersionError> {
    let trimmed = cargo_version.trim();
    let core = trimmed.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 && parts.len() != 4 {
        return Err(MsixVersionError::InvalidFormat(cargo_version.to_owned()));
    }

    let mut nums = [0u16; 4];
    for (idx, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(MsixVersionError::InvalidFormat(cargo_version.to_owned()));
        }
        nums[idx] = part
            .parse::<u16>()
            .map_err(|_| MsixVersionError::ComponentOutOfRange(cargo_version.to_owned()))?;
    }

    Ok(format!("{}.{}.{}.{}", nums[0], nums[1], nums[2], nums[3]))
}

/// Pure mirror of `Windows.Services.Store.StoreAppLicense` fields used for cross-platform
/// license-mapping unit tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawStoreLicense {
    /// `StoreAppLicense::IsActive`.
    pub is_active: bool,
    /// `StoreAppLicense::IsTrial`.
    pub is_trial: bool,
    /// `StoreAppLicense::ExpirationDate.UniversalTime` (100-ns ticks since 1601-01-01 UTC).
    pub expiration_universal_time: i64,
    /// `StoreAppLicense::TrialTimeRemaining.Duration` (100-ns ticks).
    pub trial_time_remaining_100ns: i64,
}

/// Pure target-agnostic license classification mapped from [`RawStoreLicense`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreLicenseKind {
    /// Active paid license (`IsActive == true && IsTrial == false`).
    Full,
    /// Active trial license ending at `ends_at_unix_secs` (UTC Unix seconds).
    Trial { ends_at_unix_secs: i64 },
    /// Expired trial license.
    TrialExpired,
    /// Indeterminate / no license information yet.
    Unknown,
}

/// Pure WinRT `StoreAppLicense` → [`StoreLicenseKind`] mapper.
#[derive(Debug, Default, Clone, Copy)]
pub struct StoreLicenseMapper;

impl StoreLicenseMapper {
    /// 100-ns ticks between Windows FILETIME epoch (1601-01-01) and Unix epoch (1970-01-01).
    pub const WINRT_UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;
    /// Number of 100-ns ticks in one second.
    pub const TICKS_PER_SECOND: i64 = 10_000_000;
    /// Upper bound for finite license dates (year 9999), filtering out WinRT far-future sentinels.
    pub const MAX_FINITE_UNIX_SECS: i64 = 253_402_300_800;

    /// Converts a WinRT `DateTime.UniversalTime` to finite post-1970 Unix seconds.
    #[must_use]
    pub const fn winrt_datetime_to_unix_secs(universal_time: i64) -> Option<i64> {
        if universal_time <= Self::WINRT_UNIX_EPOCH_TICKS {
            return None;
        }
        let secs = (universal_time - Self::WINRT_UNIX_EPOCH_TICKS) / Self::TICKS_PER_SECOND;
        if secs >= Self::MAX_FINITE_UNIX_SECS {
            return None;
        }
        Some(secs)
    }

    /// Converts a WinRT `TimeSpan.Duration` (100-ns ticks) to non-negative whole seconds.
    #[must_use]
    pub const fn winrt_timespan_to_secs(duration_100ns: i64) -> i64 {
        if duration_100ns <= 0 {
            0
        } else {
            duration_100ns / Self::TICKS_PER_SECOND
        }
    }

    /// Maps `raw` at `now_unix_secs` to [`StoreLicenseKind`].
    #[must_use]
    pub const fn map(raw: RawStoreLicense, now_unix_secs: i64) -> StoreLicenseKind {
        match (raw.is_active, raw.is_trial) {
            (true, false) => StoreLicenseKind::Full,
            (true, true) => {
                let exp_secs = Self::winrt_datetime_to_unix_secs(raw.expiration_universal_time);
                let rem_secs = Self::winrt_timespan_to_secs(raw.trial_time_remaining_100ns);
                if rem_secs > 0 {
                    let ends_at_unix_secs = match exp_secs {
                        Some(exp) if exp > now_unix_secs => exp,
                        _ => now_unix_secs.saturating_add(rem_secs),
                    };
                    StoreLicenseKind::Trial { ends_at_unix_secs }
                } else if let Some(exp) = exp_secs
                    && exp > now_unix_secs
                {
                    StoreLicenseKind::Trial {
                        ends_at_unix_secs: exp,
                    }
                } else {
                    StoreLicenseKind::TrialExpired
                }
            }
            (false, true) => StoreLicenseKind::TrialExpired,
            (false, false) => {
                if let Some(exp) = Self::winrt_datetime_to_unix_secs(raw.expiration_universal_time)
                    && exp <= now_unix_secs
                {
                    StoreLicenseKind::TrialExpired
                } else {
                    StoreLicenseKind::Unknown
                }
            }
        }
    }
}

/// Pure mirror of `Windows.Services.Store.StorePurchaseStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorePurchaseOutcome {
    /// `StorePurchaseStatus::Succeeded` (0).
    Succeeded,
    /// `StorePurchaseStatus::AlreadyPurchased` (1).
    AlreadyPurchased,
    /// `StorePurchaseStatus::NotPurchased` (2) — user cancelled or declined.
    NotPurchased,
    /// `StorePurchaseStatus::NetworkError` (3).
    NetworkError,
    /// `StorePurchaseStatus::ServerError` (4).
    ServerError,
    /// Unrecognized WinRT enum discriminant.
    Unknown(i32),
}

impl StorePurchaseOutcome {
    /// Converts the raw WinRT `StorePurchaseStatus` discriminant.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Succeeded,
            1 => Self::AlreadyPurchased,
            2 => Self::NotPurchased,
            3 => Self::NetworkError,
            4 => Self::ServerError,
            other => Self::Unknown(other),
        }
    }

    /// Returns `true` when the outcome means the user now holds a full license.
    #[must_use]
    pub const fn is_owned(self) -> bool {
        matches!(self, Self::Succeeded | Self::AlreadyPurchased)
    }
}
