//! macOS Metal / Apple Silicon GPU probe and x86_64 AVX2 detection.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::null_mut;

use eqho_platform::{GpuAdapter, GpuProbe};

use crate::pure::classify_apple_gpu;

#[link(name = "System")]
unsafe extern "C" {
    fn sysctlbyname(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *mut c_void,
        newlen: usize,
    ) -> c_int;
}

#[link(name = "Metal", kind = "framework")]
unsafe extern "C" {
    fn MTLCreateSystemDefaultDevice() -> *mut c_void;
}

#[link(name = "objc")]
unsafe extern "C" {
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
}

/// Probes Apple Silicon / Metal GPU adapters and Intel AVX2 support.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacGpuProbe;

impl MacGpuProbe {
    fn sysctl_string(name: &CStr) -> Option<String> {
        let mut len: usize = 0;
        // SAFETY: Querying sysctl string length first.
        if unsafe { sysctlbyname(name.as_ptr(), null_mut(), &mut len, null_mut(), 0) } != 0
            || len == 0
        {
            return None;
        }
        let mut buf = vec![0u8; len];
        // SAFETY: Reading into allocated buffer of exact length `len`.
        if unsafe {
            sysctlbyname(
                name.as_ptr(),
                buf.as_mut_ptr().cast(),
                &mut len,
                null_mut(),
                0,
            )
        } != 0
        {
            return None;
        }
        let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..nul]).trim().to_owned())
    }

    fn sysctl_u64(name: &CStr) -> Option<u64> {
        let mut val: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        // SAFETY: Reading 64-bit integer sysctl value into stack `u64`.
        if unsafe {
            sysctlbyname(
                name.as_ptr(),
                (&mut val as *mut u64).cast(),
                &mut len,
                null_mut(),
                0,
            )
        } == 0
        {
            Some(val)
        } else {
            None
        }
    }

    fn metal_default_device_info() -> Option<(String, u64)> {
        // SAFETY: Querying default Metal GPU device name and recommended working set size.
        unsafe {
            let dev = MTLCreateSystemDefaultDevice();
            if dev.is_null() {
                return None;
            }
            type MsgRetPtr = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
            type MsgRetU64 = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64;
            type MsgRetCstr = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *const c_char;
            type MsgVoid = unsafe extern "C" fn(*mut c_void, *mut c_void);

            let send_ptr: MsgRetPtr =
                std::mem::transmute::<*const (), MsgRetPtr>(objc_msgSend as *const ());
            let send_u64: MsgRetU64 =
                std::mem::transmute::<*const (), MsgRetU64>(objc_msgSend as *const ());
            let send_cstr: MsgRetCstr =
                std::mem::transmute::<*const (), MsgRetCstr>(objc_msgSend as *const ());
            let send_void: MsgVoid =
                std::mem::transmute::<*const (), MsgVoid>(objc_msgSend as *const ());

            let name_obj = send_ptr(dev, sel_registerName(c"name".as_ptr()));
            let name = if !name_obj.is_null() {
                let ptr = send_cstr(name_obj, sel_registerName(c"UTF8String".as_ptr()));
                if ptr.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(ptr).to_string_lossy().trim().to_owned()
                }
            } else {
                String::new()
            };

            let vram = send_u64(
                dev,
                sel_registerName(c"recommendedMaxWorkingSetSize".as_ptr()),
            );
            send_void(dev, sel_registerName(c"release".as_ptr()));

            if name.is_empty() {
                None
            } else {
                Some((name, vram))
            }
        }
    }
}

impl GpuProbe for MacGpuProbe {
    fn list_adapters(&self) -> Vec<GpuAdapter> {
        if let Some((name, vram)) = Self::metal_default_device_info() {
            return vec![GpuAdapter {
                name,
                vendor_id: 0x106B,
                dedicated_video_memory_bytes: vram,
            }];
        }
        let brand = Self::sysctl_string(c"machdep.cpu.brand_string").unwrap_or_default();
        let ram = Self::sysctl_u64(c"hw.memsize").unwrap_or(0);
        classify_apple_gpu(&brand, ram, cfg!(target_arch = "aarch64"))
            .into_iter()
            .collect()
    }

    fn cpu_has_avx2(&self) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            std::arch::is_x86_feature_detected!("avx2")
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            true // Apple Silicon uses NEON + AMX + Metal
        }
    }
}
