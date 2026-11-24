use std::borrow::Cow;
use std::sync::OnceLock;

use damt_platform::{Notifier, PlatformError};
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::core::HSTRING;

use super::registry::{create_key, write_string};
use crate::{ToastXml, aumid_registry_subkey, notify_icon_path};

/// Windows WinRT toast notifier with lazy AppUserModelID and icon registration.
#[derive(Debug)]
pub struct WinNotifier {
    aumid: String,
    display_name: String,
    app_dir_name: String,
    icon_png: Cow<'static, [u8]>,
    registration: OnceLock<Result<(), String>>,
}

impl WinNotifier {
    /// Creates a lazy Windows toast notifier.
    ///
    /// No icon files, HKCU registry keys, or process AUMID APIs are touched until
    /// [`Notifier::notify`] is actually called for the first time.
    pub fn new(
        aumid: impl Into<String>,
        display_name: impl Into<String>,
        app_dir_name: impl Into<String>,
        icon_png: impl Into<Cow<'static, [u8]>>,
    ) -> Self {
        Self {
            aumid: aumid.into(),
            display_name: display_name.into(),
            app_dir_name: app_dir_name.into(),
            icon_png: icon_png.into(),
            registration: OnceLock::new(),
        }
    }

    fn ensure_registered(&self) -> Result<(), PlatformError> {
        let outcome = self.registration.get_or_init(|| self.register_once());
        match outcome {
            Ok(()) => Ok(()),
            Err(message) => Err(PlatformError(message.clone())),
        }
    }

    fn register_once(&self) -> Result<(), String> {
        let icon_path = notify_icon_path(
            &self.app_dir_name,
            std::env::var_os("LOCALAPPDATA").as_deref(),
            std::env::var_os("USERPROFILE").as_deref(),
        )
        .ok_or_else(|| "LOCALAPPDATA and USERPROFILE are unavailable".to_owned())?;

        let needs_write = std::fs::metadata(&icon_path)
            .map(|meta| meta.len() == 0)
            .unwrap_or(true);
        if needs_write {
            if let Some(parent) = icon_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("failed to create notify icon directory: {err}"))?;
            }
            std::fs::write(&icon_path, self.icon_png.as_ref())
                .map_err(|err| format!("failed to write notify icon: {err}"))?;
        }

        let subkey = aumid_registry_subkey(&self.aumid);
        let key = create_key(&subkey).ok_or_else(|| format!("failed to create HKCU\\{subkey}"))?;
        write_string(&key, "DisplayName", &self.display_name)
            .map_err(|err| format!("failed to write AUMID DisplayName: {err:?}"))?;
        let icon_uri = icon_path.to_string_lossy();
        write_string(&key, "IconUri", &icon_uri)
            .map_err(|err| format!("failed to write AUMID IconUri: {err:?}"))?;

        let aumid = HSTRING::from(self.aumid.as_str());
        // SAFETY: `aumid` is a valid null-terminated UTF-16 `HSTRING` for the duration of the call.
        unsafe { SetCurrentProcessExplicitAppUserModelID(&aumid) }
            .map_err(|err| format!("SetCurrentProcessExplicitAppUserModelID failed: {err}"))?;

        Ok(())
    }
}

impl Notifier for WinNotifier {
    fn is_supported(&self) -> bool {
        true
    }

    fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        self.ensure_registered()?;
        let _com = ComInitGuard::init()?;

        let xml = ToastXml::build_generic(title, body);
        let doc = XmlDocument::new()
            .map_err(|err| PlatformError(format!("XmlDocument::new failed: {err}")))?;
        doc.LoadXml(&HSTRING::from(xml))
            .map_err(|err| PlatformError(format!("XmlDocument::LoadXml failed: {err}")))?;

        let toast = ToastNotification::CreateToastNotification(&doc)
            .map_err(|err| PlatformError(format!("CreateToastNotification failed: {err}")))?;
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(
            self.aumid.as_str(),
        ))
        .map_err(|err| PlatformError(format!("CreateToastNotifierWithId failed: {err}")))?;
        notifier
            .Show(&toast)
            .map_err(|err| PlatformError(format!("ToastNotifier::Show failed: {err}")))?;

        Ok(())
    }
}

struct ComInitGuard {
    should_uninit: bool,
}

impl ComInitGuard {
    fn init() -> Result<Self, PlatformError> {
        // SAFETY: `CoInitializeEx` initializes COM for the calling thread; `pvReserved` must be `None`.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
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

impl Drop for ComInitGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            // SAFETY: balanced with the `S_OK` return from `CoInitializeEx` on this thread.
            unsafe { CoUninitialize() };
        }
    }
}
