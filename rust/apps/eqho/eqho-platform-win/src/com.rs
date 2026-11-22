//! Per-thread RAII COM initialization guard.

use std::ffi::c_void;

use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
use windows::Win32::Media::Audio::IMMDevice;
use windows::Win32::System::Com::{
    CLSCTX, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
};
use windows::core::{GUID, HRESULT, Interface};

/// Initializes COM on the current thread and calls `CoUninitialize` on drop only when this
/// guard's `CoInitializeEx` call returned `S_OK` or `S_FALSE`.
#[derive(Debug)]
pub struct ComGuard {
    should_uninit: bool,
}

impl ComGuard {
    /// Initializes COM with `COINIT_MULTITHREADED`, falling back to `COINIT_APARTMENTTHREADED`
    /// if the thread was already initialized in STA mode.
    pub fn init() -> windows::core::Result<Self> {
        // SAFETY: `CoInitializeEx` initializes COM for the calling thread; `pvReserved` is `None`.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            // SAFETY: Thread is already STA; retry with `COINIT_APARTMENTTHREADED` to balance refcount.
            let hr_sta = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            if hr_sta == S_OK || hr_sta == S_FALSE {
                return Ok(Self { should_uninit: true });
            }
            hr_sta.ok()?;
            return Ok(Self {
                should_uninit: false,
            });
        }
        if hr == S_OK || hr == S_FALSE {
            Ok(Self { should_uninit: true })
        } else {
            hr.ok()?;
            Ok(Self {
                should_uninit: false,
            })
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            // SAFETY: Matches a preceding `CoInitializeEx` on this thread that returned `S_OK`/`S_FALSE`.
            unsafe { CoUninitialize() };
        }
    }
}

/// Calls `IMMDevice::Activate` via its COM vtable (slot 3) without requiring `Win32_System_Variant`.
pub fn activate_mm_device<T: Interface>(
    device: &IMMDevice,
    clsctx: CLSCTX,
) -> windows::core::Result<T> {
    type ActivateFn = unsafe extern "system" fn(
        this: *mut c_void,
        iid: *const GUID,
        dw_cls_ctx: CLSCTX,
        p_activation_params: *const c_void,
        pp_interface: *mut *mut c_void,
    ) -> HRESULT;

    let raw = device.as_raw();
    // SAFETY: `raw` is a valid `IMMDevice` COM pointer whose vtable slot 3 is `IMMDevice::Activate`.
    let vtbl = unsafe { *(raw.cast::<*const usize>()) };
    // SAFETY: Slot 3 of `IMMDevice_Vtbl` (after `IUnknown`'s 3 methods) is `Activate`.
    let activate: ActivateFn = unsafe { std::mem::transmute(*vtbl.add(3)) };

    let mut out: *mut c_void = std::ptr::null_mut();
    // SAFETY: `raw` is valid, `&T::IID` is a valid GUID, `p_activation_params` is null, and `out` is a valid out-pointer.
    let hr = unsafe { activate(raw, &T::IID, clsctx, std::ptr::null(), &mut out) };
    hr.ok()?;
    // SAFETY: `Activate` succeeded and wrote a non-null `T` COM interface pointer to `out`.
    unsafe { T::from_raw_borrowed(&out).cloned() }
        .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))
}
