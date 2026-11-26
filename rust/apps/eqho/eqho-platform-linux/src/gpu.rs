//! Dynamic Linux Vulkan GPU enumeration (`libvulkan.so.1`) and x86 AVX2 probe.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use eqho_platform::{GpuAdapter, GpuProbe};
use libloading::Library;

use crate::pure::{is_software_vulkan_device, vulkan_dedicated_vram_bytes};

static VULKAN_LIB: OnceLock<Option<Library>> = OnceLock::new();

fn load_vulkan_lib() -> Option<&'static Library> {
    VULKAN_LIB
        .get_or_init(|| {
            // SAFETY: Keep `libvulkan.so.1` loaded for process lifetime so Mesa/ICD destructor
            // hooks are never unmapped unexpectedly on `dlclose`.
            unsafe { Library::new("libvulkan.so.1").ok() }
        })
        .as_ref()
}

const VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO: u32 = 1;
const VK_MEMORY_HEAP_DEVICE_LOCAL_BIT: u32 = 1;

#[repr(C)]
struct VkInstanceCreateInfo {
    s_type: u32,
    p_next: *const c_void,
    flags: u32,
    p_application_info: *const c_void,
    enabled_layer_count: u32,
    pp_enabled_layer_names: *const *const c_char,
    enabled_extension_count: u32,
    pp_enabled_extension_names: *const *const c_char,
}

#[repr(C)]
struct VkPhysicalDevicePropertiesHeader {
    api_version: u32,
    driver_version: u32,
    vendor_id: u32,
    device_id: u32,
    device_type: u32,
    device_name: [c_char; 256],
    pipeline_cache_uuid: [u8; 16],
    // Padding covers `VkPhysicalDeviceLimits` + `VkPhysicalDeviceSparseProperties` (total >= 824 B).
    _rest: [u8; 1024],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VkMemoryType {
    property_flags: u32,
    heap_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VkMemoryHeap {
    size: u64,
    flags: u32,
    _pad: u32,
}

#[repr(C)]
struct VkPhysicalDeviceMemoryProperties {
    memory_type_count: u32,
    memory_types: [VkMemoryType; 32],
    memory_heap_count: u32,
    memory_heaps: [VkMemoryHeap; 16],
}

/// Probes real hardware GPUs via dynamically loaded `libvulkan.so.1` and CPU AVX2 support.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxGpuProbe;

impl GpuProbe for LinuxGpuProbe {
    fn list_adapters(&self) -> Vec<GpuAdapter> {
        let Some(vk) = load_vulkan_lib() else {
            return Vec::new();
        };

        // SAFETY: Standard Vulkan 1.0 C ABI entry points from `libvulkan.so.1`.
        unsafe {
            type VkCreateInstance = unsafe extern "C" fn(
                *const VkInstanceCreateInfo,
                *const c_void,
                *mut *mut c_void,
            ) -> c_int;
            type VkDestroyInstance = unsafe extern "C" fn(*mut c_void, *const c_void);
            type VkEnumeratePhysicalDevices =
                unsafe extern "C" fn(*mut c_void, *mut u32, *mut *mut c_void) -> c_int;
            type VkGetPhysicalDeviceProperties =
                unsafe extern "C" fn(*mut c_void, *mut VkPhysicalDevicePropertiesHeader);
            type VkGetPhysicalDeviceMemoryProperties =
                unsafe extern "C" fn(*mut c_void, *mut VkPhysicalDeviceMemoryProperties);

            let Ok(create_inst) = vk.get::<VkCreateInstance>(b"vkCreateInstance\0") else {
                return Vec::new();
            };
            let Ok(destroy_inst) = vk.get::<VkDestroyInstance>(b"vkDestroyInstance\0") else {
                return Vec::new();
            };
            let Ok(enum_devs) =
                vk.get::<VkEnumeratePhysicalDevices>(b"vkEnumeratePhysicalDevices\0")
            else {
                return Vec::new();
            };
            let Ok(get_props) =
                vk.get::<VkGetPhysicalDeviceProperties>(b"vkGetPhysicalDeviceProperties\0")
            else {
                return Vec::new();
            };
            let Ok(get_mem) = vk
                .get::<VkGetPhysicalDeviceMemoryProperties>(b"vkGetPhysicalDeviceMemoryProperties\0")
            else {
                return Vec::new();
            };

            let info = VkInstanceCreateInfo {
                s_type: VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
                p_next: null(),
                flags: 0,
                p_application_info: null(),
                enabled_layer_count: 0,
                pp_enabled_layer_names: null(),
                enabled_extension_count: 0,
                pp_enabled_extension_names: null(),
            };
            let mut instance: *mut c_void = null_mut();
            if create_inst(&info, null(), &mut instance) != 0 || instance.is_null() {
                return Vec::new();
            }

            let mut count: u32 = 0;
            let mut adapters = Vec::new();
            if enum_devs(instance, &mut count, null_mut()) == 0 && count > 0 {
                let mut devs = vec![null_mut::<c_void>(); count as usize];
                if enum_devs(instance, &mut count, devs.as_mut_ptr()) >= 0 {
                    for dev in devs.into_iter().take(count as usize) {
                        if dev.is_null() {
                            continue;
                        }
                        let mut props: VkPhysicalDevicePropertiesHeader = std::mem::zeroed();
                        get_props(dev, &mut props);
                        let name = CStr::from_ptr(props.device_name.as_ptr())
                            .to_string_lossy()
                            .trim()
                            .to_owned();
                        if name.is_empty() || is_software_vulkan_device(props.device_type, &name) {
                            continue;
                        }
                        let mut mem: VkPhysicalDeviceMemoryProperties = std::mem::zeroed();
                        get_mem(dev, &mut mem);
                        let heap_count = (mem.memory_heap_count as usize).min(16);
                        let vram = mem.memory_heaps[..heap_count]
                            .iter()
                            .filter(|h| (h.flags & VK_MEMORY_HEAP_DEVICE_LOCAL_BIT) != 0)
                            .map(|h| h.size)
                            .max()
                            .unwrap_or(0);
                        let dedicated_vram = vulkan_dedicated_vram_bytes(props.device_type, vram);
                        adapters.push(GpuAdapter {
                            name,
                            vendor_id: props.vendor_id,
                            dedicated_video_memory_bytes: dedicated_vram,
                        });
                    }
                }
            }

            destroy_inst(instance, null());
            adapters
        }
    }

    fn cpu_has_avx2(&self) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            std::arch::is_x86_feature_detected!("avx2")
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }
}
