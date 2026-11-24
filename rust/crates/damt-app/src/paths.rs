//! Per-OS data directories, matching the C# `AppDataPaths`.

use std::ffi::OsStr;
use std::path::PathBuf;

/// The data directory for one app name.
///
/// The rules match the C# host:
/// - Windows: `%APPDATA%\<AppName>`
/// - macOS: `~/Library/Application Support/<AppName>`
/// - Linux and other XDG systems: `$XDG_CONFIG_HOME/<appname>` or `~/.config/<appname>`, lowercased
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDataPaths {
    app_name: String,
}

impl AppDataPaths {
    /// Create the path helper for `app_name` (for Ocy, `"Ocy"`).
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
        }
    }

    /// The app name given to [`new`](Self::new).
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// The data directory. This does not touch the filesystem.
    pub fn data_dir(&self) -> PathBuf {
        data_dir_for(&self.app_name)
    }

    /// Create the data directory if missing and return it.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O error when the directory cannot be created.
    pub fn ensure(&self) -> std::io::Result<PathBuf> {
        let dir = self.data_dir();
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

/// The data directory for `app_name`. This does not touch the filesystem.
pub fn data_directory(app_name: &str) -> PathBuf {
    data_dir_for(app_name)
}

/// Create the data directory for `app_name` if missing and return it.
///
/// # Errors
///
/// Returns the underlying I/O error when the directory cannot be created.
pub fn ensure_data_directory(app_name: &str) -> std::io::Result<PathBuf> {
    let dir = data_dir_for(app_name);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn data_dir_for(app_name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        windows_data_dir(
            app_name,
            std::env::var_os("APPDATA").as_deref(),
            std::env::var_os("USERPROFILE").as_deref(),
        )
    }

    #[cfg(target_os = "macos")]
    {
        macos_data_dir(app_name, std::env::var_os("HOME").as_deref())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        xdg_data_dir(
            app_name,
            std::env::var_os("XDG_CONFIG_HOME").as_deref(),
            std::env::var_os("HOME").as_deref(),
        )
    }

    #[cfg(not(any(windows, unix)))]
    {
        PathBuf::from(app_name)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn windows_data_dir(
    app_name: &str,
    appdata: Option<&OsStr>,
    user_profile: Option<&OsStr>,
) -> PathBuf {
    let base = match appdata {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => match user_profile {
            Some(value) if !value.is_empty() => {
                PathBuf::from(value).join("AppData").join("Roaming")
            }
            _ => PathBuf::new(),
        },
    };

    base.join(app_name)
}

#[cfg_attr(not(test), allow(dead_code))]
fn macos_data_dir(app_name: &str, home: Option<&OsStr>) -> PathBuf {
    let base = match home {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::new(),
    };

    base.join("Library")
        .join("Application Support")
        .join(app_name)
}

#[cfg_attr(not(test), allow(dead_code))]
fn xdg_data_dir(app_name: &str, xdg_config_home: Option<&OsStr>, home: Option<&OsStr>) -> PathBuf {
    if let Some(value) = xdg_config_home
        && !value.is_empty()
    {
        return PathBuf::from(value).join(app_name.to_lowercase());
    }

    let base = match home {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::new(),
    };

    base.join(".config").join(app_name.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_uses_appdata() {
        assert_eq!(
            windows_data_dir(
                "Ocy",
                Some(OsStr::new("/appdata")),
                Some(OsStr::new("/home/user"))
            ),
            PathBuf::from("/appdata").join("Ocy")
        );
    }

    #[test]
    fn windows_falls_back_to_roaming_appdata() {
        assert_eq!(
            windows_data_dir("Ocy", None, Some(OsStr::new("/home/user"))),
            PathBuf::from("/home/user")
                .join("AppData")
                .join("Roaming")
                .join("Ocy")
        );
    }

    #[test]
    fn windows_ignores_empty_environment_values() {
        assert_eq!(
            windows_data_dir("Ocy", Some(OsStr::new("")), Some(OsStr::new("/home/user"))),
            PathBuf::from("/home/user")
                .join("AppData")
                .join("Roaming")
                .join("Ocy")
        );
    }

    #[test]
    fn macos_uses_application_support() {
        assert_eq!(
            macos_data_dir("Ocy", Some(OsStr::new("/home/user"))),
            PathBuf::from("/home/user")
                .join("Library")
                .join("Application Support")
                .join("Ocy")
        );
    }

    #[test]
    fn xdg_uses_config_home_and_lowercases_app_name() {
        assert_eq!(
            xdg_data_dir(
                "Ocy",
                Some(OsStr::new("/home/user/.config")),
                Some(OsStr::new("/home/user"))
            ),
            PathBuf::from("/home/user/.config").join("ocy")
        );
        assert_eq!(
            xdg_data_dir(
                "Ocy",
                Some(OsStr::new("/xdg")),
                Some(OsStr::new("/home/user"))
            ),
            PathBuf::from("/xdg").join("ocy")
        );
    }

    #[test]
    fn xdg_falls_back_to_home_config() {
        assert_eq!(
            xdg_data_dir("Ocy", None, Some(OsStr::new("/home/user"))),
            PathBuf::from("/home/user").join(".config").join("ocy")
        );
        assert_eq!(
            xdg_data_dir("Ocy", Some(OsStr::new("")), Some(OsStr::new("/home/user"))),
            PathBuf::from("/home/user").join(".config").join("ocy")
        );
    }

    #[test]
    fn missing_home_returns_relative_path() {
        assert_eq!(
            xdg_data_dir("Ocy", None, None),
            PathBuf::from(".config").join("ocy")
        );
        assert_eq!(
            macos_data_dir("Ocy", None),
            PathBuf::from("Library")
                .join("Application Support")
                .join("Ocy")
        );
    }
}
