//! Pure cross-platform helpers for Linux desktop integration (compiled and tested on all OSes).

use std::path::{Path, PathBuf};

/// Compute the XDG autostart `.desktop` file path (`$XDG_CONFIG_HOME/autostart/<id>.desktop`).
#[must_use]
pub fn autostart_file_path(
    xdg_config_home: Option<&str>,
    home: Option<&str>,
    app_id: &str,
) -> Option<PathBuf> {
    let base = xdg_config_home
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|s| !s.trim().is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    let clean_id: String = app_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if clean_id.is_empty() {
        return None;
    }
    Some(base.join("autostart").join(format!("{clean_id}.desktop")))
}

/// Prefer `$APPIMAGE` over `/proc/self/exe` so autostart entries survive AppImage reboots.
#[must_use]
pub fn resolve_autostart_exec_path(appimage_env: Option<&str>, current_exe: &Path) -> PathBuf {
    if let Some(ai) = appimage_env.map(str::trim).filter(|s| !s.is_empty()) {
        PathBuf::from(ai)
    } else {
        current_exe.to_path_buf()
    }
}

/// Escape an executable path for the `Exec=` field of an XDG `.desktop` entry.
fn escape_desktop_exec(path: &str) -> String {
    let needs_quotes = path.chars().any(|c| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '\\' | '$' | '`' | '&' | ';' | '(' | ')')
    });
    if !needs_quotes {
        return path.to_owned();
    }
    let mut out = String::with_capacity(path.len() + 2);
    out.push('"');
    for ch in path.chars() {
        if matches!(ch, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// Build a valid XDG Autostart `.desktop` file body.
#[must_use]
pub fn build_autostart_desktop_entry(app_name: &str, exec_path: &str, icon_name: &str) -> String {
    let escaped = escape_desktop_exec(exec_path);
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name={app_name}\n\
         Comment=Offline local voice-to-text dictation\n\
         Exec={escaped}\n\
         Icon={icon_name}\n\
         Terminal=false\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

/// Parse XDG Desktop Portal (`org.freedesktop.appearance color-scheme`) or `gsettings` output
/// into `Some(true)` = light theme, `Some(false)` = dark theme, or `None` = unknown.
#[must_use]
pub fn parse_portal_color_scheme(raw: &str) -> Option<bool> {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    // Portal spec: 0 = no preference, 1 = prefer dark, 2 = prefer light.
    if lower.contains("uint32 1") || lower.contains("prefer-dark") || lower.ends_with(":dark") {
        return Some(false);
    }
    if lower.contains("uint32 2") || lower.contains("prefer-light") {
        return Some(true);
    }
    if lower.contains("default") && lower.contains('\'') {
        return Some(true);
    }
    if lower.contains("-dark") {
        return Some(false);
    }
    None
}

/// Parse GNOME `enable-animations` (`true`/`false`) or KDE `AnimationDurationFactor` (`0` = off).
#[must_use]
pub fn parse_enable_animations(raw: &str) -> Option<bool> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed == "true" {
        return Some(true);
    }
    if trimmed == "false" {
        return Some(false);
    }
    if let Ok(factor) = trimmed.parse::<f32>() {
        return Some(factor > 0.0);
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn autostart_path_respects_xdg_and_home() {
        assert_eq!(
            autostart_file_path(Some("/tmp/xdg"), Some("/home/u"), "eqho"),
            Some(PathBuf::from("/tmp/xdg/autostart/eqho.desktop"))
        );
        assert_eq!(
            autostart_file_path(None, Some("/home/u"), "xyz.damt.eqho"),
            Some(PathBuf::from("/home/u/.config/autostart/xyz.damt.eqho.desktop"))
        );
        assert_eq!(autostart_file_path(None, None, "eqho"), None);
    }

    #[test]
    fn desktop_entry_escapes_spaces_and_prefers_appimage() {
        let exe = resolve_autostart_exec_path(
            Some("/home/u/Apps/Eqho 0.9.AppImage"),
            Path::new("/tmp/.mount_Eqho/usr/bin/Eqho"),
        );
        assert_eq!(exe, PathBuf::from("/home/u/Apps/Eqho 0.9.AppImage"));
        let entry = build_autostart_desktop_entry("Eqho", &exe.to_string_lossy(), "eqho");
        assert!(entry.contains("Exec=\"/home/u/Apps/Eqho 0.9.AppImage\""));
        assert!(entry.contains("Name=Eqho"));
    }

    #[test]
    fn parses_portal_and_gsettings_color_schemes() {
        assert_eq!(parse_portal_color_scheme("(<<uint32 1>>,)"), Some(false));
        assert_eq!(parse_portal_color_scheme("(<<uint32 2>>,)"), Some(true));
        assert_eq!(parse_portal_color_scheme("'prefer-dark'"), Some(false));
        assert_eq!(parse_portal_color_scheme("'default'"), Some(true));
        assert_eq!(parse_portal_color_scheme("Adwaita:dark"), Some(false));
        assert_eq!(parse_portal_color_scheme(""), None);
    }

    #[test]
    fn parses_animation_preferences() {
        assert_eq!(parse_enable_animations("false\n"), Some(false));
        assert_eq!(parse_enable_animations("true\n"), Some(true));
        assert_eq!(parse_enable_animations("0"), Some(false));
        assert_eq!(parse_enable_animations("1.0"), Some(true));
        assert_eq!(parse_enable_animations("unknown"), None);
    }
}
