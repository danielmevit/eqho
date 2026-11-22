use std::ffi::c_void;

use damt_platform::PlatformError;
use windows::Services::Store::StoreContext;
use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE, S_FALSE, S_OK};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::IInitializeWithWindow;
use windows::core::{HSTRING, Interface};

use crate::{RawStoreLicense, StorePurchaseOutcome};

struct MtaComGuard {
    should_uninit: bool,
}

impl MtaComGuard {
    fn init() -> Result<Self, PlatformError> {
        // SAFETY: `CoInitializeEx` initializes COM on this worker thread; `pvReserved` must be `None`.
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

/// Queries `Windows.Services.Store.StoreContext::GetDefault()?.GetAppLicenseAsync()` on an MTA
/// worker thread and returns the raw license fields.
pub fn query_app_license() -> Result<RawStoreLicense, PlatformError> {
    std::thread::scope(|scope| {
        let handle = scope.spawn(query_app_license_on_current_thread);
        match handle.join() {
            Ok(result) => result,
            Err(_) => Err(PlatformError(
                "Store license query thread panicked".to_owned(),
            )),
        }
    })
}

/// Queries `StoreContext::GetDefault()?.GetAppLicenseAsync()` asynchronously on a background thread
/// without blocking the UI thread, invoking `on_complete` when finished.
pub fn query_app_license_async<F>(on_complete: F)
where
    F: FnOnce(Result<RawStoreLicense, PlatformError>) + Send + 'static,
{
    std::thread::spawn(move || {
        let result = query_app_license_on_current_thread();
        on_complete(result);
    });
}

fn query_app_license_on_current_thread() -> Result<RawStoreLicense, PlatformError> {
    let _com = MtaComGuard::init()?;
    let context = StoreContext::GetDefault()
        .map_err(|err| PlatformError(format!("StoreContext::GetDefault failed: {err}")))?;
    let license = context
        .GetAppLicenseAsync()
        .map_err(|err| PlatformError(format!("StoreContext::GetAppLicenseAsync failed: {err}")))?
        .join()
        .map_err(|err| {
            PlatformError(format!(
                "StoreContext::GetAppLicenseAsync join failed: {err}"
            ))
        })?;
    let is_active = license
        .IsActive()
        .map_err(|err| PlatformError(format!("StoreAppLicense::IsActive failed: {err}")))?;
    let is_trial = license
        .IsTrial()
        .map_err(|err| PlatformError(format!("StoreAppLicense::IsTrial failed: {err}")))?;
    let expiration = license
        .ExpirationDate()
        .map_err(|err| PlatformError(format!("StoreAppLicense::ExpirationDate failed: {err}")))?;
    let trial_remaining = license.TrialTimeRemaining().map_err(|err| {
        PlatformError(format!("StoreAppLicense::TrialTimeRemaining failed: {err}"))
    })?;
    Ok(RawStoreLicense {
        is_active,
        is_trial,
        expiration_universal_time: expiration.UniversalTime,
        trial_time_remaining_100ns: trial_remaining.Duration,
    })
}

/// Starts `StoreContext::RequestPurchaseAsync(store_id)` on the calling UI thread, initializing
/// the `StoreContext` with `owner_hwnd` via `IInitializeWithWindow`, and invokes `on_complete`
/// without blocking the UI message loop.
pub fn request_purchase_async<F>(
    store_id: &str,
    owner_hwnd: isize,
    on_complete: F,
) -> Result<(), PlatformError>
where
    F: FnOnce(Result<StorePurchaseOutcome, PlatformError>) + Send + 'static,
{
    if store_id.trim().is_empty() {
        return Err(PlatformError(
            "Store product ID must not be empty".to_owned(),
        ));
    }
    if owner_hwnd == 0 {
        return Err(PlatformError(
            "Settings window HWND is unavailable for Store purchase UI".to_owned(),
        ));
    }

    let context = StoreContext::GetDefault()
        .map_err(|err| PlatformError(format!("StoreContext::GetDefault failed: {err}")))?;
    let init: IInitializeWithWindow = context
        .cast()
        .map_err(|err| PlatformError(format!("IInitializeWithWindow cast failed: {err}")))?;
    // SAFETY: `owner_hwnd` is the live Win32 HWND of the Settings window on the UI thread,
    // required by `StoreContext` desktop purchase UI via `IInitializeWithWindow::Initialize`.
    unsafe {
        init.Initialize(HWND(owner_hwnd as *mut c_void))
            .map_err(|err| {
                PlatformError(format!("IInitializeWithWindow::Initialize failed: {err}"))
            })?;
    }

    let op = context
        .RequestPurchaseAsync(&HSTRING::from(store_id))
        .map_err(|err| {
            PlatformError(format!("StoreContext::RequestPurchaseAsync failed: {err}"))
        })?;

    op.when(move |res| {
        let mapped = match res {
            Ok(purchase_result) => match purchase_result.Status() {
                Ok(status) => Ok(StorePurchaseOutcome::from_raw(status.0)),
                Err(err) => Err(PlatformError(format!(
                    "StorePurchaseResult::Status failed: {err}"
                ))),
            },
            Err(err) => Err(PlatformError(format!(
                "StoreContext::RequestPurchaseAsync failed: {err}"
            ))),
        };
        on_complete(mapped);
    })
    .map_err(|err| {
        PlatformError(format!(
            "StoreContext::RequestPurchaseAsync completion hook failed: {err}"
        ))
    })?;

    Ok(())
}
