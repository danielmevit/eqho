use damt_platform::{AppThemeProvider, TaskbarThemeProvider};

use super::registry::{PERSONALIZE_KEY, READ_ACCESS, open_key, read_registry_value};
use crate::TaskbarThemeMath;

const LIGHT_THEME_VALUE: &str = "SystemUsesLightTheme";
const LIGHT_APPS_VALUE: &str = "AppsUseLightTheme";

/// The taskbar theme from `HKCU\...\Themes\Personalize` (C# `WindowsTaskbarThemeProvider`).
#[derive(Debug, Default, Clone, Copy)]
pub struct WinTaskbarTheme;

impl TaskbarThemeProvider for WinTaskbarTheme {
    fn is_supported(&self) -> bool {
        true
    }

    fn is_light_taskbar(&self) -> Option<bool> {
        let key = open_key(PERSONALIZE_KEY, READ_ACCESS)?;
        let value = read_registry_value(&key, LIGHT_THEME_VALUE);
        TaskbarThemeMath::from_registry_value(value)
    }
}

/// The app (window content) theme from `HKCU\...\Themes\Personalize` `AppsUseLightTheme` (same DWORD rules).
#[derive(Debug, Default, Clone, Copy)]
pub struct WinAppTheme;

impl AppThemeProvider for WinAppTheme {
    fn is_supported(&self) -> bool {
        true
    }

    fn is_light_app_theme(&self) -> Option<bool> {
        let key = open_key(PERSONALIZE_KEY, READ_ACCESS)?;
        let value = read_registry_value(&key, LIGHT_APPS_VALUE);
        TaskbarThemeMath::from_registry_value(value)
    }
}
