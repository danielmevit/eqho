use damt_platform::{
    ActiveScreenProvider, AppThemeProvider, AutostartService, FocusStateProvider, ForegroundProbe,
    IdleProvider, MotionPreference, Notifier, PlatformServices, TaskbarThemeProvider, Unsupported,
};

use super::{
    WinActiveScreen, WinAppTheme, WinAutostart, WinFocus, WinForeground, WinIdle, WinMotion,
    WinNotifier, WinSessionEvents, WinTaskbarTheme,
};

/// All Windows platform services, composed once at startup (C# `WindowsPlatformServices`).
#[derive(Debug)]
pub struct WinPlatform {
    idle: WinIdle,
    focus: WinFocus,
    foreground: WinForeground,
    autostart: WinAutostart,
    session: WinSessionEvents,
    active_screen: WinActiveScreen,
    motion: WinMotion,
    taskbar_theme: WinTaskbarTheme,
    app_theme: WinAppTheme,
    notifier: Option<WinNotifier>,
}

impl WinPlatform {
    /// `autostart_value_name` is the Run-key value the app writes (for example its product name).
    pub fn new(autostart_value_name: impl Into<String>) -> Self {
        Self {
            idle: WinIdle,
            focus: WinFocus,
            foreground: WinForeground,
            autostart: WinAutostart::new(autostart_value_name),
            session: WinSessionEvents::new(),
            active_screen: WinActiveScreen,
            motion: WinMotion,
            taskbar_theme: WinTaskbarTheme,
            app_theme: WinAppTheme,
            notifier: None,
        }
    }

    /// Attaches a Windows toast [`WinNotifier`] to this platform service bundle.
    #[must_use]
    pub fn with_notifier(mut self, notifier: WinNotifier) -> Self {
        self.notifier = Some(notifier);
        self
    }

    /// Configures the `desktop:StartupTask` `TaskId` used for autostart when packaged as MSIX.
    #[must_use]
    pub fn with_startup_task_id(mut self, task_id: impl Into<String>) -> Self {
        self.autostart = self.autostart.with_startup_task_id(task_id);
        self
    }

    /// The session event source, which is not part of the [`PlatformServices`] trait.
    pub fn session(&mut self) -> &mut WinSessionEvents {
        &mut self.session
    }
}

impl PlatformServices for WinPlatform {
    fn idle(&self) -> &dyn IdleProvider {
        &self.idle
    }

    fn focus(&self) -> &dyn FocusStateProvider {
        &self.focus
    }

    fn foreground(&self) -> &dyn ForegroundProbe {
        &self.foreground
    }

    fn autostart(&self) -> &dyn AutostartService {
        &self.autostart
    }

    fn active_screen(&self) -> &dyn ActiveScreenProvider {
        &self.active_screen
    }

    fn motion(&self) -> &dyn MotionPreference {
        &self.motion
    }

    fn taskbar_theme(&self) -> &dyn TaskbarThemeProvider {
        &self.taskbar_theme
    }

    fn app_theme(&self) -> &dyn AppThemeProvider {
        &self.app_theme
    }

    fn notifier(&self) -> &dyn Notifier {
        match &self.notifier {
            Some(notifier) => notifier,
            None => &Unsupported,
        }
    }
}
