use damt_platform::{AutostartService, PlatformError};
use windows::ApplicationModel::StartupTask;
use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::HSTRING;

use super::registry::{
    READ_ACCESS, RUN_KEY, WRITE_ACCESS, create_run_key, delete_value, open_key, read_string,
    write_string,
};
use crate::{AutostartCommand, AutostartKind, AutostartPathSelector, StartupTaskToggleState};

/// Returns `true` when the running process has MSIX/AppX package identity.
#[must_use]
pub fn is_packaged_app() -> bool {
    let mut length: u32 = 0;
    // SAFETY: Passing `None` with `length == 0` queries only the required buffer length without
    // writing to any caller buffer.
    let err = unsafe { GetCurrentPackageFullName(&mut length, None) };
    AutostartPathSelector::is_packaged_error_code(err.0)
}

/// Per-user autostart: `Windows.ApplicationModel.StartupTask` when packaged (MSIX),
/// or the HKCU Run key when unpackaged (C# `WindowsAutostartService`).
#[derive(Debug, Clone)]
pub struct WinAutostart {
    value_name: String,
    command: Result<String, String>,
    startup_task_id: Option<String>,
    is_packaged: bool,
}

impl WinAutostart {
    /// Builds the stored command from the running executable; `value_name` is chosen by the app.
    pub fn new(value_name: impl Into<String>) -> Self {
        let command = match std::env::current_exe() {
            Ok(path) => {
                AutostartCommand::build(&path.to_string_lossy()).map_err(|error| error.to_string())
            }
            Err(error) => Err(format!("could not resolve the current executable: {error}")),
        };

        Self {
            value_name: value_name.into(),
            command,
            startup_task_id: None,
            is_packaged: is_packaged_app(),
        }
    }

    /// Configures the `desktop:StartupTask` `TaskId` used when running as a packaged MSIX app.
    #[must_use]
    pub fn with_startup_task_id(mut self, task_id: impl Into<String>) -> Self {
        let id = task_id.into();
        self.startup_task_id = (!id.trim().is_empty()).then_some(id);
        self
    }

    fn backend_kind(&self) -> AutostartKind {
        AutostartPathSelector::select(self.is_packaged, self.startup_task_id.is_some())
    }

    fn run_key_is_enabled(&self) -> bool {
        let Ok(command) = &self.command else {
            return false;
        };

        let Some(key) = open_key(RUN_KEY, READ_ACCESS) else {
            return false;
        };
        let stored = read_string(&key, &self.value_name);

        stored.is_some_and(|value| value.to_lowercase() == command.to_lowercase())
    }

    fn run_key_set_enabled(&self, enabled: bool) -> Result<(), PlatformError> {
        let command = self
            .command
            .as_ref()
            .map_err(|message| PlatformError(message.clone()))?;

        if enabled {
            let Some(key) = create_run_key() else {
                return Err(PlatformError(
                    "could not open the Run key for writing".to_owned(),
                ));
            };
            write_string(&key, &self.value_name, command)
                .map_err(|error| PlatformError(format!("could not write the Run value: {error:?}")))
        } else {
            let Some(key) = open_key(RUN_KEY, WRITE_ACCESS) else {
                return Ok(());
            };
            delete_value(&key, &self.value_name).map_err(|error| {
                PlatformError(format!("could not delete the Run value: {error:?}"))
            })
        }
    }
}

impl AutostartService for WinAutostart {
    fn is_supported(&self) -> bool {
        !matches!(self.backend_kind(), AutostartKind::Unsupported)
    }

    fn is_enabled(&self) -> bool {
        match self.backend_kind() {
            AutostartKind::RegistryRunKey => self.run_key_is_enabled(),
            AutostartKind::PackagedStartupTask => {
                let Some(task_id) = self.startup_task_id.as_deref() else {
                    return false;
                };
                startup_task_state(task_id).is_ok_and(StartupTaskToggleState::is_enabled)
            }
            AutostartKind::Unsupported => false,
        }
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), PlatformError> {
        match self.backend_kind() {
            AutostartKind::RegistryRunKey => self.run_key_set_enabled(enabled),
            AutostartKind::PackagedStartupTask => {
                let Some(task_id) = self.startup_task_id.as_deref() else {
                    return Ok(());
                };
                startup_task_set_enabled(task_id, enabled)
            }
            AutostartKind::Unsupported => Ok(()),
        }
    }
}

struct MtaComGuard {
    should_uninit: bool,
}

impl MtaComGuard {
    fn init() -> Result<Self, PlatformError> {
        // SAFETY: `CoInitializeEx` initializes COM for this worker thread; `pvReserved` must be `None`.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr == S_OK {
            Ok(Self {
                should_uninit: true,
            })
        } else if hr == S_FALSE || hr == RPC_E_CHANGED_MODE {
            Ok(Self {
                should_uninit: false,
            })
        } else {
            Err(PlatformError(format!("CoInitializeEx failed: {hr:?}")))
        }
    }
}

impl Drop for MtaComGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            // SAFETY: balanced with the `S_OK` return from `CoInitializeEx` on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

fn run_on_mta_thread<F, T>(f: F) -> Result<T, PlatformError>
where
    F: FnOnce() -> Result<T, PlatformError> + Send,
    T: Send,
{
    std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let _com = MtaComGuard::init()?;
            f()
        });
        match handle.join() {
            Ok(result) => result,
            Err(_) => Err(PlatformError(
                "StartupTask worker thread panicked".to_owned(),
            )),
        }
    })
}

fn get_startup_task(task_id: &str) -> Result<StartupTask, PlatformError> {
    let id = HSTRING::from(task_id);
    StartupTask::GetAsync(&id)
        .map_err(|err| PlatformError(format!("StartupTask::GetAsync failed: {err}")))?
        .join()
        .map_err(|err| PlatformError(format!("StartupTask::GetAsync join failed: {err}")))
}

fn startup_task_state(task_id: &str) -> Result<StartupTaskToggleState, PlatformError> {
    run_on_mta_thread(|| {
        let task = get_startup_task(task_id)?;
        let raw = task
            .State()
            .map_err(|err| PlatformError(format!("StartupTask::State failed: {err}")))?;
        Ok(StartupTaskToggleState::from_raw(raw.0))
    })
}

fn startup_task_set_enabled(task_id: &str, enabled: bool) -> Result<(), PlatformError> {
    run_on_mta_thread(|| {
        let task = get_startup_task(task_id)?;
        if enabled {
            let raw = task
                .RequestEnableAsync()
                .map_err(|err| {
                    PlatformError(format!("StartupTask::RequestEnableAsync failed: {err}"))
                })?
                .join()
                .map_err(|err| {
                    PlatformError(format!(
                        "StartupTask::RequestEnableAsync join failed: {err}"
                    ))
                })?;
            StartupTaskToggleState::from_raw(raw.0)
                .check_after_enable()
                .map_err(PlatformError)
        } else {
            task.Disable()
                .map_err(|err| PlatformError(format!("StartupTask::Disable failed: {err}")))?;
            let raw = task
                .State()
                .map_err(|err| PlatformError(format!("StartupTask::State failed: {err}")))?;
            StartupTaskToggleState::from_raw(raw.0)
                .check_after_disable()
                .map_err(PlatformError)
        }
    })
}
