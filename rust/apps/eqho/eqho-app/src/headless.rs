//! Headless CLI modes: `--smoke`, `--e2e-wav`, and `--soak`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use eqho_core::settings::Settings;
use eqho_core::textproc::clean_text;
use eqho_core::vad::{SAMPLE_RATE, SILENCE_THRESHOLD, Segment, filter_segments};
use eqho_engine::{load_wav_16k_mono, run_smoke};
use eqho_host::{HostSegment, ModelHost, ModelHostConfig};
use eqho_overlay::{OverlayConfig, OverlayHandle};
use eqho_platform::GpuProbe;
use serde::Serialize;

use crate::paths::resolve_dll_dir;

#[cfg(windows)]
type PlatformBundle = eqho_platform_win::WinEqhoPlatform;
#[cfg(target_os = "linux")]
type PlatformBundle = eqho_platform_linux::LinuxEqhoPlatform;
#[cfg(target_os = "macos")]
type PlatformBundle = eqho_platform_macos::MacEqhoPlatform;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
type PlatformBundle = eqho_platform::UnsupportedEqhoPlatform;

fn create_platform() -> PlatformBundle {
    PlatformBundle::default()
}

fn current_exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Eqho"))
}

fn has_discrete_gpu(platform: &dyn GpuProbe) -> bool {
    const MIN_VRAM: u64 = 2 * 1024 * 1024 * 1024;
    platform
        .list_adapters()
        .iter()
        .any(|a| a.dedicated_video_memory_bytes >= MIN_VRAM)
}

/// Run `--smoke` and print JSON report to stdout.
pub fn run_smoke_cli(dll_dir_override: Option<PathBuf>) -> ExitCode {
    let exe = current_exe_path();
    let dll_dir = resolve_dll_dir(dll_dir_override.as_deref(), &exe);
    let platform = create_platform();
    let cfg = ModelHostConfig::new(exe, dll_dir);
    let report = run_smoke(&platform, Some(&cfg));
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(err) => eprintln!("failed to serialize smoke report: {err}"),
    }
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[derive(Debug, Serialize)]
struct E2eWavReport {
    ok: bool,
    model: String,
    device: String,
    whisper_version: String,
    load_ms: u64,
    infer_ms: u64,
    audio_sec: f64,
    raw_text: String,
    clean_text: String,
    segments: Vec<HostSegment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Run `--e2e-wav <wav_path>` through the real `Eqho --model-host` child process.
pub fn run_e2e_wav_cli(
    wav_path: PathBuf,
    model: Option<String>,
    dll_dir_override: Option<PathBuf>,
    model_dir_override: Option<PathBuf>,
    gpu_override: Option<bool>,
) -> ExitCode {
    let exe = current_exe_path();
    let dll_dir = resolve_dll_dir(dll_dir_override.as_deref(), &exe);
    let defaults = Settings::default();
    let model_key = model.unwrap_or(defaults.model_size.clone());
    let model_dir = model_dir_override.unwrap_or_else(|| defaults.resolve_model_dir());
    let platform = create_platform();
    let use_gpu = gpu_override.unwrap_or_else(|| has_discrete_gpu(&platform));

    let samples = match load_wav_16k_mono(&wav_path) {
        Ok(s) => s,
        Err(err) => {
            let report = E2eWavReport {
                ok: false,
                model: model_key,
                device: String::new(),
                whisper_version: String::new(),
                load_ms: 0,
                infer_ms: 0,
                audio_sec: 0.0,
                raw_text: String::new(),
                clean_text: String::new(),
                segments: Vec::new(),
                error: Some(err),
            };
            if let Ok(json) = serde_json::to_string_pretty(&report) {
                println!("{json}");
            }
            return ExitCode::from(1);
        }
    };

    let audio_sec = samples.len() as f64 / SAMPLE_RATE as f64;
    let mut host = ModelHost::new(ModelHostConfig::new(exe, dll_dir));

    let load_start = Instant::now();
    let info = match host.load_model(&model_key, &model_dir, use_gpu, 4, &mut |_, _| {}) {
        Ok(info) => info,
        Err(err) => {
            host.shutdown();
            let report = E2eWavReport {
                ok: false,
                model: model_key,
                device: String::new(),
                whisper_version: String::new(),
                load_ms: load_start.elapsed().as_millis() as u64,
                infer_ms: 0,
                audio_sec,
                raw_text: String::new(),
                clean_text: String::new(),
                segments: Vec::new(),
                error: Some(err.to_string()),
            };
            if let Ok(json) = serde_json::to_string_pretty(&report) {
                println!("{json}");
            }
            return ExitCode::from(1);
        }
    };
    let load_ms = load_start.elapsed().as_millis() as u64;

    let infer_start = Instant::now();
    let segments = match host.transcribe(&samples, "en", 1, "") {
        Ok(segs) => segs,
        Err(err) => {
            host.shutdown();
            let report = E2eWavReport {
                ok: false,
                model: info.model,
                device: info.device,
                whisper_version: info.whisper_version,
                load_ms,
                infer_ms: infer_start.elapsed().as_millis() as u64,
                audio_sec,
                raw_text: String::new(),
                clean_text: String::new(),
                segments: Vec::new(),
                error: Some(err.to_string()),
            };
            if let Ok(json) = serde_json::to_string_pretty(&report) {
                println!("{json}");
            }
            return ExitCode::from(1);
        }
    };
    let infer_ms = infer_start.elapsed().as_millis() as u64;
    host.shutdown();

    let raw_text = segments
        .iter()
        .map(|s| s.text.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    let vad_segs: Vec<Segment> = segments.iter().map(Segment::from).collect();
    let cleaned = filter_segments(&vad_segs, audio_sec, None, SILENCE_THRESHOLD)
        .map(|filtered| clean_text(&filtered, "en", false))
        .unwrap_or_default();

    let report = E2eWavReport {
        ok: true,
        model: info.model,
        device: info.device,
        whisper_version: info.whisper_version,
        load_ms,
        infer_ms,
        audio_sec,
        raw_text,
        clean_text: cleaned,
        segments,
        error: None,
    };
    if let Ok(json) = serde_json::to_string_pretty(&report) {
        println!("{json}");
    }
    ExitCode::SUCCESS
}

#[derive(Debug, Serialize)]
struct SoakReport {
    ok: bool,
    cycles: usize,
    start_private_bytes: u64,
    end_private_bytes: u64,
    delta_bytes: i64,
}

/// Run `--soak <cycles>` rapid overlay/UI lifecycle iterations and print memory JSON.
pub fn run_soak_cli(cycles: usize, _dll_dir_override: Option<PathBuf>) -> ExitCode {
    if let Err(err) = slint::BackendSelector::new()
        .renderer_name("software".into())
        .select()
    {
        eprintln!("failed to select Slint software renderer: {err}");
        return ExitCode::from(1);
    }

    let overlay = OverlayHandle::start(OverlayConfig::default());
    let (start_private, _) = process_memory_bytes();

    for cycle in 0..cycles {
        overlay.show("Listening...");
        overlay.update_text(&format!("Eqho soak cycle {cycle} transcription sample"));
        overlay.set_level(0.65);
        overlay.set_waveform(0.55, 0.45);
        std::thread::sleep(std::time::Duration::from_millis(20));
        overlay.hide();
        std::thread::sleep(std::time::Duration::from_millis(10));

        let (priv_bytes, ws_bytes) = process_memory_bytes();
        eprintln!(
            "soak cycle {}/{}: private_bytes={}, working_set_bytes={}",
            cycle + 1,
            cycles,
            priv_bytes,
            ws_bytes
        );
    }

    overlay.shutdown();
    let (end_private, _) = process_memory_bytes();
    let delta_bytes = i64::try_from(end_private).unwrap_or(i64::MAX)
        - i64::try_from(start_private).unwrap_or(i64::MAX);

    let report = SoakReport {
        ok: true,
        cycles,
        start_private_bytes: start_private,
        end_private_bytes: end_private,
        delta_bytes,
    };
    if let Ok(json) = serde_json::to_string_pretty(&report) {
        println!("{json}");
    }
    ExitCode::SUCCESS
}

#[cfg(windows)]
fn process_memory_bytes() -> (u64, u64) {
    use std::mem::{size_of, zeroed};
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: `counters` is zero-initialized with its `cb` field set to its exact byte size,
    // and `GetCurrentProcess()` returns a valid pseudo-handle for this process.
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = zeroed();
        counters.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ptr = (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>();
        if GetProcessMemoryInfo(GetCurrentProcess(), ptr, counters.cb).is_ok() {
            (counters.PrivateUsage as u64, counters.WorkingSetSize as u64)
        } else {
            (0, 0)
        }
    }
}

#[cfg(target_os = "linux")]
fn process_memory_bytes() -> (u64, u64) {
    // `/proc/self/statm`: size resident shared text lib data dt (in pages).
    let Ok(statm) = std::fs::read_to_string("/proc/self/statm") else {
        return (0, 0);
    };
    let mut parts = statm.split_whitespace();
    let _size = parts.next();
    let resident_pages: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let shared_pages: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let page_size = 4096_u64;
    let rss = resident_pages.saturating_mul(page_size);
    let private = resident_pages
        .saturating_sub(shared_pages)
        .saturating_mul(page_size);
    (private, rss)
}

#[cfg(target_os = "macos")]
fn process_memory_bytes() -> (u64, u64) {
    #[repr(C)]
    struct TimeVal {
        tv_sec: i64,
        tv_usec: i32,
        _pad: i32,
    }
    #[repr(C)]
    struct Rusage {
        ru_utime: TimeVal,
        ru_stime: TimeVal,
        ru_maxrss: i64,
        _rest: [i64; 13],
    }
    #[link(name = "System", kind = "framework")]
    unsafe extern "C" {
        fn getrusage(who: i32, usage: *mut Rusage) -> i32;
    }
    // SAFETY: `getrusage(RUSAGE_SELF = 0, &mut ru)` fills a valid stack `Rusage` struct.
    // On macOS, `ru_maxrss` is reported directly in bytes.
    unsafe {
        let mut ru: Rusage = std::mem::zeroed();
        if getrusage(0, &mut ru) == 0 {
            let bytes = u64::try_from(ru.ru_maxrss).unwrap_or(0);
            (bytes, bytes)
        } else {
            (0, 0)
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn process_memory_bytes() -> (u64, u64) {
    (0, 0)
}
