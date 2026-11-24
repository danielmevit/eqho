//! FROZEN platform contracts, lead-owned. The Rust mirror of `src/Ocy.Abstractions/PlatformContracts.cs`.
//!
//! Workers implement these traits (Windows in R2, macOS/Linux later); they must not change them. Every method is
//! infallible unless it returns `Result`: on error, an implementation returns the documented "unknown" value.

use std::time::Duration;

/// Error from a platform call that can fail (only autostart today).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PlatformError(pub String);

/// The current time, injectable for tests (C# `TimeProvider`).
pub trait Clock: Send + Sync {
    /// Now, in the user's local time zone. `now().timestamp()` is the UTC instant.
    fn now(&self) -> jiff::ZonedDateTime;
}

/// The real clock: system time in the system time zone.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> jiff::ZonedDateTime {
        jiff::ZonedDateTime::now()
    }
}

/// Time since the user's last keyboard or mouse input.
pub trait IdleProvider {
    /// False when this platform cannot report idle time (the engine then never enters Away by idle).
    fn is_supported(&self) -> bool;
    /// Idle time; zero when unsupported or on error. Never negative.
    fn idle_time(&self) -> Duration;
}

/// What the user is doing on screen, as far as breaks are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FocusState {
    #[default]
    Unknown,
    Normal,
    FullscreenApp,
    Presentation,
    QuietTime,
    NotPresent,
}

/// `QUNS_BUSY` from `SHQueryUserNotificationState` (full-screen app, including many videos).
pub const QUNS_BUSY: i32 = 2;
/// `QUNS_RUNNING_D3D_FULL_SCREEN` (exclusive Direct3D full screen).
pub const QUNS_RUNNING_D3D_FULL_SCREEN: i32 = 3;
/// `QUNS_PRESENTATION_MODE`.
pub const QUNS_PRESENTATION_MODE: i32 = 4;
/// `QUNS_APP` (full-screen Store app).
pub const QUNS_APP: i32 = 7;

/// One read of the foreground full-screen app. Unsupported platforms return [`ForegroundObservation::unknown`].
///
/// Added for game detection. Existing focus/idle contracts are unchanged; this trait has defaults so
/// older `PlatformServices` impls keep compiling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundObservation {
    /// Raw `SHQueryUserNotificationState` value, or `None` when the query failed or this platform
    /// cannot make it. See the `QUNS_*` constants.
    pub notification_state: Option<i32>,
    /// File name of the foreground process image (`"Game.exe"`), when it could be read.
    pub exe_name: Option<String>,
    /// Full path of the foreground process image, when it could be read.
    pub exe_path: Option<String>,
    /// Process id of the foreground window, when known.
    pub process_id: Option<u32>,
}

impl Default for ForegroundObservation {
    fn default() -> Self {
        Self::unknown()
    }
}

impl ForegroundObservation {
    /// Nothing known: not a game, not an exclusive full-screen app.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            notification_state: None,
            exe_name: None,
            exe_path: None,
            process_id: None,
        }
    }
}

/// Reads the foreground app and the Windows Game Bar game list. Read-only: implementations must
/// not write the registry or any store they do not own.
pub trait ForegroundProbe {
    /// False when this platform cannot classify the foreground app.
    fn is_supported(&self) -> bool {
        false
    }

    /// Current foreground observation. [`ForegroundObservation::unknown`] when unsupported or on error.
    fn observe(&self) -> ForegroundObservation {
        ForegroundObservation::unknown()
    }

    /// Full paths from the Game Bar game store (`MatchedExeFullPath` and similar). Empty when
    /// unsupported or unreadable. Never writes.
    fn game_bar_exe_paths(&self) -> Vec<String> {
        Vec::new()
    }

    /// Whether `process_id` has not exited.
    ///
    /// False when the id is unknown or the process is gone. An implementation that cannot tell
    /// (access denied on a live elevated process) should return true so a hold is not ended early.
    fn process_alive(&self, _process_id: u32) -> bool {
        false
    }
}

pub trait FocusStateProvider {
    fn is_supported(&self) -> bool;
    /// Current state; `Unknown` when unsupported or on error.
    fn state(&self) -> FocusState;
}

/// Start the app when the user signs in.
pub trait AutostartService {
    fn is_supported(&self) -> bool;
    fn is_enabled(&self) -> bool;
    /// Enable or disable autostart for the current user. No-op (Ok) when unsupported.
    fn set_enabled(&self, enabled: bool) -> Result<(), PlatformError>;
}

/// OS session events. Delivered on an arbitrary thread; the host marshals them to the engine's thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionEvent {
    Locked,
    Unlocked,
    Suspending,
    Resumed,
    LoggingOff,
}

pub trait SessionEventSource: Send {
    fn is_supported(&self) -> bool;
    /// Begin delivering events to `sink`. Idempotent.
    fn start(&mut self, sink: Box<dyn Fn(SessionEvent) + Send + Sync>);
}

/// A rectangle in physical screen pixels (virtual-desktop coordinates; may be negative).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Which monitor the user is working on right now.
pub trait ActiveScreenProvider {
    fn is_supported(&self) -> bool;
    /// Bounds of the monitor with the foreground window, else the one under the cursor. `None` if unknown.
    fn active_screen_bounds(&self) -> Option<ScreenRect>;
}

/// The OS-level animation preference (Windows: "Animation effects").
pub trait MotionPreference {
    fn is_supported(&self) -> bool;
    /// True when the OS asks apps to minimise animation; false when allowed or unknown.
    fn prefers_reduced_motion(&self) -> bool;
}

/// The taskbar / notification-area colour theme (can differ from the app theme on Windows).
pub trait TaskbarThemeProvider {
    fn is_supported(&self) -> bool;
    /// `Some(true)` = light taskbar, `Some(false)` = dark, `None` = unknown.
    fn is_light_taskbar(&self) -> Option<bool>;
}

/// The OS light/dark preference for app content (Windows: "Choose your app mode", `AppsUseLightTheme`).
/// Can differ from the taskbar theme. Added 2026-11-19 (R3c kit gap), FROZEN.
pub trait AppThemeProvider {
    fn is_supported(&self) -> bool;
    /// `Some(true)` = light apps, `Some(false)` = dark apps, `None` = unknown.
    fn is_light_app_theme(&self) -> Option<bool>;
}

/// Displays brief OS status notifications.
pub trait Notifier {
    fn is_supported(&self) -> bool;
    /// Shows a status notification with `title` and `body`. No-op (`Ok(())`) when unsupported.
    fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError>;
}

/// The set of platform services an app composes at startup. Defaults are "unsupported".
pub trait PlatformServices {
    fn idle(&self) -> &dyn IdleProvider {
        &Unsupported
    }
    fn focus(&self) -> &dyn FocusStateProvider {
        &Unsupported
    }
    /// Foreground game probe. The default is unsupported (no game detection).
    fn foreground(&self) -> &dyn ForegroundProbe {
        &Unsupported
    }
    fn autostart(&self) -> &dyn AutostartService {
        &Unsupported
    }
    fn active_screen(&self) -> &dyn ActiveScreenProvider {
        &Unsupported
    }
    fn motion(&self) -> &dyn MotionPreference {
        &Unsupported
    }
    fn taskbar_theme(&self) -> &dyn TaskbarThemeProvider {
        &Unsupported
    }
    fn app_theme(&self) -> &dyn AppThemeProvider {
        &Unsupported
    }
    fn notifier(&self) -> &dyn Notifier {
        &Unsupported
    }
}

/// The "unsupported" fallback for every contract (C# `Unsupported*`).
#[derive(Debug, Default, Clone, Copy)]
pub struct Unsupported;

impl IdleProvider for Unsupported {
    fn is_supported(&self) -> bool {
        false
    }
    fn idle_time(&self) -> Duration {
        Duration::ZERO
    }
}
impl FocusStateProvider for Unsupported {
    fn is_supported(&self) -> bool {
        false
    }
    fn state(&self) -> FocusState {
        FocusState::Unknown
    }
}
impl ForegroundProbe for Unsupported {}
impl AutostartService for Unsupported {
    fn is_supported(&self) -> bool {
        false
    }
    fn is_enabled(&self) -> bool {
        false
    }
    fn set_enabled(&self, _enabled: bool) -> Result<(), PlatformError> {
        Ok(())
    }
}
impl ActiveScreenProvider for Unsupported {
    fn is_supported(&self) -> bool {
        false
    }
    fn active_screen_bounds(&self) -> Option<ScreenRect> {
        None
    }
}
impl MotionPreference for Unsupported {
    fn is_supported(&self) -> bool {
        false
    }
    fn prefers_reduced_motion(&self) -> bool {
        false
    }
}
impl TaskbarThemeProvider for Unsupported {
    fn is_supported(&self) -> bool {
        false
    }
    fn is_light_taskbar(&self) -> Option<bool> {
        None
    }
}
impl AppThemeProvider for Unsupported {
    fn is_supported(&self) -> bool {
        false
    }
    fn is_light_app_theme(&self) -> Option<bool> {
        None
    }
}
impl Notifier for Unsupported {
    fn is_supported(&self) -> bool {
        false
    }
    fn notify(&self, _title: &str, _body: &str) -> Result<(), PlatformError> {
        Ok(())
    }
}
impl PlatformServices for Unsupported {}
