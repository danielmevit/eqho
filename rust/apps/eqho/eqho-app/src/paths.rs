//! Runtime DLL directory, isolated config paths, and theme polarity resolution.

use std::path::{Path, PathBuf};

use eqho_core::settings::resolve_config_dir;

/// Runtime file paths anchored at `eqho_core::settings::resolve_config_dir()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    pub config_dir: PathBuf,
    pub settings_path: PathBuf,
    pub history_path: PathBuf,
    pub log_dir: PathBuf,
}

/// Resolve runtime config, settings, history, and log paths.
#[must_use]
pub fn resolve_runtime_paths() -> RuntimePaths {
    let config_dir = resolve_config_dir();
    let settings_path = config_dir.join("settings.json");
    let history_path = config_dir.join("history.jsonl");
    let log_dir = config_dir.clone();
    RuntimePaths {
        config_dir,
        settings_path,
        history_path,
        log_dir,
    }
}

/// Resolve the whisper.cpp DLL directory in priority order:
/// 1. Explicit CLI `--dll-dir` override
/// 2. `EQHO_WHISPER_DLL_DIR` environment variable
/// 3. `<exe_dir>/dlls` if `whisper.dll` exists there
/// 4. `<exe_dir>` (co-located DLLs next to `Eqho.exe`)
#[must_use]
pub fn resolve_dll_dir(cli_override: Option<&Path>, exe_path: &Path) -> PathBuf {
    let env_override = std::env::var("EQHO_WHISPER_DLL_DIR").ok();
    resolve_dll_dir_with_env(cli_override, env_override.as_deref(), exe_path)
}

#[must_use]
pub(crate) fn resolve_dll_dir_with_env(
    cli_override: Option<&Path>,
    env_override: Option<&str>,
    exe_path: &Path,
) -> PathBuf {
    if let Some(dir) = cli_override
        && !dir.as_os_str().is_empty()
    {
        return dir.to_path_buf();
    }

    if let Some(env_dir) = env_override
        && !env_dir.trim().is_empty()
    {
        return PathBuf::from(env_dir.trim());
    }

    let exe_dir = exe_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    let sub_dlls = exe_dir.join("dlls");
    if sub_dlls.join("whisper.dll").is_file() {
        return sub_dlls;
    }

    exe_dir.to_path_buf()
}

/// Resolve whether the UI and overlay should use dark mode.
///
/// - `"light"` => `false`
/// - `"dark"` => `true`
/// - `"system"` (or unknown) => `!system_is_light.unwrap_or(false)` (defaults to dark)
#[must_use]
pub fn resolve_theme_is_dark(setting: &str, system_is_light: Option<bool>) -> bool {
    match setting.trim().to_ascii_lowercase().as_str() {
        "light" => false,
        "dark" => true,
        _ => !system_is_light.unwrap_or(false),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("eqho_app_paths_{tag}_{}_{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn dll_dir_priority_order() {
        let root = make_temp_dir("dll");
        let exe_path = root.join("Eqho.exe");
        let cli_dir = root.join("cli_dlls");
        let env_dir = root.join("env_dlls");
        let sub_dlls = root.join("dlls");
        fs::create_dir_all(&sub_dlls).unwrap();
        fs::write(sub_dlls.join("whisper.dll"), b"stub").unwrap();

        // 1. CLI override wins over everything.
        assert_eq!(
            resolve_dll_dir_with_env(
                Some(&cli_dir),
                Some(env_dir.to_str().unwrap()),
                &exe_path,
            ),
            cli_dir
        );

        // 2. Env var wins over <exe_dir>/dlls and <exe_dir>.
        assert_eq!(
            resolve_dll_dir_with_env(None, Some(env_dir.to_str().unwrap()), &exe_path),
            env_dir
        );

        // 3. <exe_dir>/dlls wins when whisper.dll exists there.
        assert_eq!(
            resolve_dll_dir_with_env(None, Some("   "), &exe_path),
            sub_dlls
        );

        // 4. <exe_dir> fallback when <exe_dir>/dlls/whisper.dll is absent.
        fs::remove_file(sub_dlls.join("whisper.dll")).unwrap();
        assert_eq!(resolve_dll_dir_with_env(None, None, &exe_path), root);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn theme_resolution_handles_light_dark_and_system() {
        assert!(!resolve_theme_is_dark("light", Some(false)));
        assert!(resolve_theme_is_dark("dark", Some(true)));
        assert!(!resolve_theme_is_dark("system", Some(true)));
        assert!(resolve_theme_is_dark("system", Some(false)));
        assert!(resolve_theme_is_dark("system", None));
    }
}
