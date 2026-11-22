//! DXGI GPU adapter enumeration and CPU AVX2 capability probe.

use eqho_platform::{GpuAdapter, GpuProbe};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows::Win32::System::Threading::{IsProcessorFeaturePresent, PROCESSOR_FEATURE_ID};

use crate::pure::{from_utf16_nul, is_software_adapter};

/// `PF_AVX2_INSTRUCTIONS_AVAILABLE` (40) per Win32 `IsProcessorFeaturePresent`.
const PF_AVX2_INSTRUCTIONS_AVAILABLE: PROCESSOR_FEATURE_ID = PROCESSOR_FEATURE_ID(40);
const MAX_DXGI_ADAPTERS: u32 = 64;

/// Windows DXGI GPU and AVX2 CPU probe.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinGpuProbe;

impl GpuProbe for WinGpuProbe {
    fn list_adapters(&self) -> Vec<GpuAdapter> {
        // SAFETY: `CreateDXGIFactory1` creates a COM `IDXGIFactory1` with no caller-owned pointers.
        let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
            return Vec::new();
        };

        let mut adapters = Vec::new();
        for index in 0..MAX_DXGI_ADAPTERS {
            // SAFETY: `factory` is a valid `IDXGIFactory1` COM pointer; `EnumAdapters1` returns
            // `DXGI_ERROR_NOT_FOUND` when enumeration completes.
            let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
                break;
            };
            // SAFETY: `adapter` is a valid `IDXGIAdapter1` COM pointer.
            let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
                continue;
            };
            if is_software_adapter(desc.VendorId, desc.Flags) {
                continue;
            }
            adapters.push(GpuAdapter {
                name: from_utf16_nul(&desc.Description),
                vendor_id: desc.VendorId,
                dedicated_video_memory_bytes: desc.DedicatedVideoMemory as u64,
            });
        }
        adapters
    }

    fn cpu_has_avx2(&self) -> bool {
        // SAFETY: `IsProcessorFeaturePresent` queries a static kernel feature flag.
        unsafe { IsProcessorFeaturePresent(PF_AVX2_INSTRUCTIONS_AVAILABLE) }.as_bool()
    }
}
