//! Global cross-platform speech model recommendation based on real PC specs
//! (discrete GPU VRAM / Apple Silicon Metal working set, CPU logical cores, AVX2/NEON SIMD,
//! and available physical RAM) plus target language.

/// High-level compute class used to pick GPU vs CPU inference in `Auto` backend mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardwareClass {
    Gpu,
    Cpu,
}

/// Fine-grained hardware tier derived from real probed PC specs across Windows, macOS, and Linux.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardwareTier {
    /// Discrete GPU (or Apple Silicon Metal) with >= 6 GiB VRAM and >= 3.5 GiB available RAM.
    GpuHigh,
    /// Mid-range GPU (or 8 GB Apple Silicon Mac) with 3..6 GiB VRAM and >= 2 GiB available RAM.
    GpuMid,
    /// Entry discrete GPU with 2..3 GiB VRAM and >= 2 GiB available RAM.
    GpuEntry,
    /// Modern multi-core CPU (or integrated laptop GPU) with fast SIMD (AVX2/ARM64),
    /// >= 4 logical threads, and >= 2 GiB available RAM.
    CpuFast,
    /// Older/low-core/non-AVX2 CPU or tight-memory PC (< 2 GiB available RAM).
    CpuLight,
}

/// Cross-platform snapshot of real probed PC specs used globally (Windows, macOS, Linux) before
/// placing the `"Recommended for this PC"` label on any model card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardwareSpecs {
    /// Dedicated VRAM of the best discrete GPU (or Apple Silicon Metal working set) in MiB.
    /// Integrated x86 iGPUs (Intel UHD/Iris, etc.) report `0` so shared RAM is never mistaken
    /// for dedicated GPU VRAM.
    pub gpu_vram_mb: u64,
    /// Logical CPU thread count (`std::thread::available_parallelism()`).
    pub cpu_threads: u32,
    /// Whether the CPU supports fast vector SIMD (x86_64 AVX2 or ARM64 NEON/Apple Silicon).
    pub cpu_fast_simd: bool,
    /// Available physical RAM in MiB at probe time (`None` if OS memory probe failed).
    pub available_ram_mb: Option<u64>,
}

// Real 6 GB discrete GPUs (e.g. RTX 3060 Laptop 6 GB) report ~5968–6023 MiB via DXGI/Vulkan due
// to driver reservations, so use 5.5 GiB (5632 MiB) as the 6 GB class threshold.
const GPU_HIGH_MIN_VRAM_MB: u64 = 5632;
const GPU_MID_MIN_VRAM_MB: u64 = 3 * 1024;
const GPU_ENTRY_MIN_VRAM_MB: u64 = 2 * 1024;
const GPU_HIGH_MIN_RAM_MB: u64 = 3584; // 3.5 GiB
const STANDARD_MIN_RAM_MB: u64 = 2048; // 2 GiB

impl HardwareSpecs {
    /// Representative default specs corresponding to a coarse [`HardwareClass`] (used by tests
    /// and legacy callers that only supply `HardwareClass`).
    #[must_use]
    pub const fn from_class(hw: HardwareClass) -> Self {
        match hw {
            HardwareClass::Gpu => Self {
                gpu_vram_mb: 8 * 1024,
                cpu_threads: 8,
                cpu_fast_simd: true,
                available_ram_mb: Some(16 * 1024),
            },
            HardwareClass::Cpu => Self {
                gpu_vram_mb: 0,
                cpu_threads: 8,
                cpu_fast_simd: true,
                available_ram_mb: Some(8 * 1024),
            },
        }
    }

    /// Classify real PC specs into a [`HardwareTier`].
    #[must_use]
    pub const fn tier(self) -> HardwareTier {
        let ram_ok_high = match self.available_ram_mb {
            Some(mb) => mb >= GPU_HIGH_MIN_RAM_MB,
            None => true,
        };
        let ram_ok_std = match self.available_ram_mb {
            Some(mb) => mb >= STANDARD_MIN_RAM_MB,
            None => true,
        };

        if self.gpu_vram_mb >= GPU_HIGH_MIN_VRAM_MB && ram_ok_high {
            HardwareTier::GpuHigh
        } else if self.gpu_vram_mb >= GPU_MID_MIN_VRAM_MB && ram_ok_std {
            HardwareTier::GpuMid
        } else if self.gpu_vram_mb >= GPU_ENTRY_MIN_VRAM_MB && ram_ok_std {
            HardwareTier::GpuEntry
        } else if self.cpu_fast_simd && self.cpu_threads >= 4 && ram_ok_std {
            HardwareTier::CpuFast
        } else {
            HardwareTier::CpuLight
        }
    }

    /// Coarse GPU-vs-CPU class for `Auto` inference backend selection.
    #[must_use]
    pub const fn hardware_class(self) -> HardwareClass {
        match self.tier() {
            HardwareTier::GpuHigh | HardwareTier::GpuMid | HardwareTier::GpuEntry => {
                HardwareClass::Gpu
            }
            HardwareTier::CpuFast | HardwareTier::CpuLight => HardwareClass::Cpu,
        }
    }
}

/// Recommend the best English-optimized model key for real probed [`HardwareSpecs`], or `None`
/// on ultra-light CPUs where only the 60 MB multilingual `base-q5` fits comfortably.
#[must_use]
pub fn recommend_english_model_for_specs(specs: HardwareSpecs) -> Option<&'static str> {
    match specs.tier() {
        HardwareTier::GpuHigh => Some("distil-large-v3"),
        HardwareTier::GpuMid => Some("distil-medium.en"),
        HardwareTier::GpuEntry | HardwareTier::CpuFast => Some("small.en-q5"),
        HardwareTier::CpuLight => None,
    }
}

/// Recommend the best multilingual model key for real probed [`HardwareSpecs`].
#[must_use]
pub fn recommend_multilingual_model_for_specs(specs: HardwareSpecs) -> &'static str {
    match specs.tier() {
        HardwareTier::GpuHigh => "large-v3-turbo",
        HardwareTier::GpuMid => "large-v3-turbo-q5",
        HardwareTier::GpuEntry | HardwareTier::CpuFast => "small-q5",
        HardwareTier::CpuLight => "base-q5",
    }
}

/// Recommend the best speech model key for real probed [`HardwareSpecs`] and target language.
///
/// Global cross-platform rule (Windows, macOS, Linux):
/// - `GpuHigh` (>= 5.5 GiB reported / 6 GB+ GPU, >= 3.5 GiB RAM): `distil-large-v3` (EN) / `large-v3-turbo` (multi)
/// - `GpuMid` (3–5.5 GiB VRAM, e.g. 4 GB laptop GPU or 8 GB Apple Silicon): `large-v3-turbo-q5`
/// - `GpuEntry` (2–3 GiB VRAM): `small.en-q5` (EN) / `small-q5` (multi)
/// - `CpuFast` (>= 4 threads, AVX2/ARM64, >= 2 GiB RAM, incl. laptop iGPUs): `small.en-q5` / `small-q5`
/// - `CpuLight` (older non-AVX2 CPU, < 4 threads, or < 2 GiB free RAM): `base-q5`
#[must_use]
pub fn recommend_model_for_specs(specs: HardwareSpecs, lang: &str) -> &'static str {
    let is_en = lang.eq_ignore_ascii_case("en");
    match specs.tier() {
        HardwareTier::GpuHigh => {
            if is_en {
                "distil-large-v3"
            } else {
                "large-v3-turbo"
            }
        }
        HardwareTier::GpuMid => "large-v3-turbo-q5",
        HardwareTier::GpuEntry | HardwareTier::CpuFast => {
            if is_en {
                "small.en-q5"
            } else {
                "small-q5"
            }
        }
        HardwareTier::CpuLight => "base-q5",
    }
}

/// Convenience wrapper for callers with only a coarse [`HardwareClass`].
#[must_use]
pub fn recommend_model(hw: HardwareClass, lang: &str) -> &'static str {
    recommend_model_for_specs(HardwareSpecs::from_class(hw), lang)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn recommends_expected_models() {
        assert_eq!(recommend_model(HardwareClass::Gpu, "en"), "distil-large-v3");
        assert_eq!(recommend_model(HardwareClass::Gpu, "de"), "large-v3-turbo");
        assert_eq!(recommend_model(HardwareClass::Cpu, "en"), "small.en-q5");
        assert_eq!(recommend_model(HardwareClass::Cpu, "fr"), "small-q5");
    }

    #[test]
    fn recommends_models_across_all_pc_spec_tiers() {
        // High-VRAM discrete GPU (RTX 3060 6 GB Laptop reports ~5968 MiB via DXGI)
        let rtx_3060_laptop = HardwareSpecs {
            gpu_vram_mb: 5968,
            cpu_threads: 16,
            cpu_fast_simd: true,
            available_ram_mb: Some(12 * 1024),
        };
        assert_eq!(rtx_3060_laptop.tier(), HardwareTier::GpuHigh);
        assert_eq!(
            recommend_english_model_for_specs(rtx_3060_laptop),
            Some("distil-large-v3")
        );
        assert_eq!(
            recommend_multilingual_model_for_specs(rtx_3060_laptop),
            "large-v3-turbo"
        );

        // High-VRAM discrete GPU (RTX 3060 12 GB) or 16 GB Apple Silicon (~11 GB Metal)
        let high_gpu = HardwareSpecs {
            gpu_vram_mb: 12 * 1024,
            cpu_threads: 12,
            cpu_fast_simd: true,
            available_ram_mb: Some(16 * 1024),
        };
        assert_eq!(high_gpu.tier(), HardwareTier::GpuHigh);
        assert_eq!(high_gpu.hardware_class(), HardwareClass::Gpu);
        assert_eq!(recommend_model_for_specs(high_gpu, "en"), "distil-large-v3");
        assert_eq!(recommend_model_for_specs(high_gpu, "bg"), "large-v3-turbo");

        // Mid-VRAM laptop GPU (4 GB GTX 1650 / RTX 3050) or 8 GB Apple Silicon (~5.3 GB Metal)
        let mid_gpu = HardwareSpecs {
            gpu_vram_mb: 4 * 1024,
            cpu_threads: 8,
            cpu_fast_simd: true,
            available_ram_mb: Some(6 * 1024),
        };
        assert_eq!(mid_gpu.tier(), HardwareTier::GpuMid);
        assert_eq!(mid_gpu.hardware_class(), HardwareClass::Gpu);
        assert_eq!(recommend_model_for_specs(mid_gpu, "en"), "large-v3-turbo-q5");
        assert_eq!(recommend_model_for_specs(mid_gpu, "de"), "large-v3-turbo-q5");

        // Entry 2 GB GPU (MX450 / GT 1030)
        let entry_gpu = HardwareSpecs {
            gpu_vram_mb: 2 * 1024,
            cpu_threads: 8,
            cpu_fast_simd: true,
            available_ram_mb: Some(8 * 1024),
        };
        assert_eq!(entry_gpu.tier(), HardwareTier::GpuEntry);
        assert_eq!(recommend_model_for_specs(entry_gpu, "en"), "small.en-q5");
        assert_eq!(recommend_model_for_specs(entry_gpu, "es"), "small-q5");

        // Modern iGPU laptop (e.g. Intel i7-8650U + UHD 620, 8 threads, AVX2, 23 GB free RAM)
        let fast_cpu = HardwareSpecs {
            gpu_vram_mb: 0,
            cpu_threads: 8,
            cpu_fast_simd: true,
            available_ram_mb: Some(23 * 1024),
        };
        assert_eq!(fast_cpu.tier(), HardwareTier::CpuFast);
        assert_eq!(fast_cpu.hardware_class(), HardwareClass::Cpu);
        assert_eq!(recommend_model_for_specs(fast_cpu, "en"), "small.en-q5");
        assert_eq!(recommend_model_for_specs(fast_cpu, "fr"), "small-q5");

        // Low-spec / older CPU (dual-core or no AVX2) or tight RAM (< 2 GiB free)
        let dual_core = HardwareSpecs {
            gpu_vram_mb: 0,
            cpu_threads: 2,
            cpu_fast_simd: true,
            available_ram_mb: Some(4096),
        };
        assert_eq!(dual_core.tier(), HardwareTier::CpuLight);
        assert_eq!(recommend_model_for_specs(dual_core, "en"), "base-q5");

        let no_avx2 = HardwareSpecs {
            gpu_vram_mb: 0,
            cpu_threads: 4,
            cpu_fast_simd: false,
            available_ram_mb: Some(8192),
        };
        assert_eq!(no_avx2.tier(), HardwareTier::CpuLight);
        assert_eq!(recommend_model_for_specs(no_avx2, "de"), "base-q5");

        let tight_ram = HardwareSpecs {
            gpu_vram_mb: 8 * 1024,
            cpu_threads: 8,
            cpu_fast_simd: true,
            available_ram_mb: Some(1200),
        };
        assert_eq!(tight_ram.tier(), HardwareTier::CpuLight);
        assert_eq!(recommend_model_for_specs(tight_ram, "en"), "base-q5");
    }
}
