//! Read-only hardware and OS-service probe for `WinEqhoPlatform`.
//!
//! Prints a single JSON object to stdout. Compiles as a no-op JSON stub on non-Windows targets.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::fmt::Write as _;

use eqho_platform_win::pure::json_escape;

struct ProbeReport {
    adapters: Vec<AdapterEntry>,
    cpu_has_avx2: bool,
    free_memory_mb: Option<u64>,
    mic_access_blocked: Option<bool>,
    volume_supported: bool,
    current_volume: Option<f32>,
    foreground_exe: Option<String>,
    input_devices: Vec<InputDeviceEntry>,
}

struct AdapterEntry {
    name: String,
    vendor_id: u32,
    dedicated_mb: u64,
}

struct InputDeviceEntry {
    index: usize,
    name: String,
    is_default: bool,
}

fn format_opt_u64(val: Option<u64>) -> String {
    match val {
        Some(v) => v.to_string(),
        None => "null".to_owned(),
    }
}

fn format_opt_bool(val: Option<bool>) -> &'static str {
    match val {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    }
}

fn format_opt_f32(val: Option<f32>) -> String {
    match val {
        Some(v) if v.is_finite() => format!("{v:.4}"),
        _ => "null".to_owned(),
    }
}

fn format_opt_str(val: Option<&str>) -> String {
    match val {
        Some(s) => format!("\"{}\"", json_escape(s)),
        None => "null".to_owned(),
    }
}

fn format_report_json(report: &ProbeReport) -> String {
    let mut out = String::from("{\n");

    out.push_str("  \"adapters\": [");
    for (i, adapter) in report.adapters.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "\n    {{\"name\": \"{}\", \"vendor_id\": {}, \"dedicated_mb\": {}}}",
            json_escape(&adapter.name),
            adapter.vendor_id,
            adapter.dedicated_mb
        );
    }
    if !report.adapters.is_empty() {
        out.push_str("\n  ");
    }
    out.push_str("],\n");

    let _ = writeln!(out, "  \"cpu_has_avx2\": {},", report.cpu_has_avx2);
    let _ = writeln!(
        out,
        "  \"free_memory_mb\": {},",
        format_opt_u64(report.free_memory_mb)
    );
    let _ = writeln!(
        out,
        "  \"mic_access_blocked\": {},",
        format_opt_bool(report.mic_access_blocked)
    );
    let _ = writeln!(out, "  \"volume_supported\": {},", report.volume_supported);
    let _ = writeln!(
        out,
        "  \"current_volume\": {},",
        format_opt_f32(report.current_volume)
    );
    let _ = writeln!(
        out,
        "  \"foreground_exe\": {},",
        format_opt_str(report.foreground_exe.as_deref())
    );

    out.push_str("  \"input_devices\": [");
    for (i, dev) in report.input_devices.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "\n    {{\"index\": {}, \"name\": \"{}\", \"is_default\": {}}}",
            dev.index,
            json_escape(&dev.name),
            dev.is_default
        );
    }
    if !report.input_devices.is_empty() {
        out.push_str("\n  ");
    }
    out.push_str("]\n}");
    out
}

#[cfg(windows)]
fn collect_probe() -> ProbeReport {
    use eqho_platform::{
        AudioBackend, FocusTracker, GpuProbe, MemoryProbe, MicPrivacyProbe, VolumeControl,
    };
    use eqho_platform_win::WinEqhoPlatform;

    let platform = WinEqhoPlatform::new();

    let adapters = GpuProbe::list_adapters(&platform)
        .into_iter()
        .map(|a| AdapterEntry {
            name: a.name,
            vendor_id: a.vendor_id,
            dedicated_mb: a.dedicated_video_memory_bytes / (1024 * 1024),
        })
        .collect();

    let hwnd = FocusTracker::get_foreground_window(&platform);
    let foreground_exe = FocusTracker::get_window_process_name(&platform, hwnd);

    let input_devices = AudioBackend::list_input_devices(&platform)
        .into_iter()
        .map(|d| InputDeviceEntry {
            index: d.index,
            name: d.name,
            is_default: d.is_default,
        })
        .collect();

    ProbeReport {
        adapters,
        cpu_has_avx2: GpuProbe::cpu_has_avx2(&platform),
        free_memory_mb: MemoryProbe::free_memory_mb(&platform),
        mic_access_blocked: MicPrivacyProbe::mic_access_blocked(&platform),
        volume_supported: VolumeControl::is_supported(&platform),
        current_volume: VolumeControl::get_volume(&platform),
        foreground_exe,
        input_devices,
    }
}

#[cfg(not(windows))]
fn collect_probe() -> ProbeReport {
    ProbeReport {
        adapters: Vec::new(),
        cpu_has_avx2: false,
        free_memory_mb: None,
        mic_access_blocked: None,
        volume_supported: false,
        current_volume: None,
        foreground_exe: None,
        input_devices: Vec::new(),
    }
}

fn main() {
    let report = collect_probe();
    println!("{}", format_report_json(&report));
}
