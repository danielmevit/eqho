//! R0 measurement spike binary for Eqho 0.9.x.
#![deny(clippy::unwrap_used, clippy::expect_used)]

mod pill_probe;
mod tray_probe;
mod wav;
mod whisper_ffi;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use pill_probe::{PillBenchArgs, run_pill_bench};
use tray_probe::{TrayBenchArgs, run_tray_bench};
use whisper_ffi::{TranscribeArgs, run_transcribe};

fn print_usage() {
    eprintln!(
        "Usage:
  eqho-spike transcribe --dll-dir <dir> --model <path.bin> --wav <path.wav> [--gpu true|false] [--lang en] [--threads N] [--beam 1]
  eqho-spike pill-bench [--frames 60] [--width 280] [--height 51] [--light false] [--show-ms 0]
  eqho-spike tray-bench [--hold-ms 500]"
    );
}

fn next_val(args: &[String], idx: &mut usize, flag: &str) -> Result<String, String> {
    *idx += 1;
    args.get(*idx)
        .cloned()
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_bool(s: &str, flag: &str) -> Result<bool, String> {
    match s {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(format!("invalid boolean '{s}' for {flag}")),
    }
}

fn parse_transcribe(args: &[String]) -> Result<TranscribeArgs, String> {
    let mut dll_dir: Option<PathBuf> = None;
    let mut model: Option<PathBuf> = None;
    let mut wav: Option<PathBuf> = None;
    let mut gpu = true;
    let mut lang = "en".to_string();
    let mut threads = 4i32;
    let mut beam = 1i32;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--dll-dir" => dll_dir = Some(PathBuf::from(next_val(args, &mut i, flag)?)),
            "--model" => model = Some(PathBuf::from(next_val(args, &mut i, flag)?)),
            "--wav" => wav = Some(PathBuf::from(next_val(args, &mut i, flag)?)),
            "--gpu" => gpu = parse_bool(&next_val(args, &mut i, flag)?, flag)?,
            "--lang" => lang = next_val(args, &mut i, flag)?,
            "--threads" => {
                let v = next_val(args, &mut i, flag)?;
                threads = v.parse::<i32>().map_err(|e| format!("invalid --threads '{v}': {e}"))?;
            }
            "--beam" => {
                let v = next_val(args, &mut i, flag)?;
                beam = v.parse::<i32>().map_err(|e| format!("invalid --beam '{v}': {e}"))?;
            }
            other => return Err(format!("unknown flag for transcribe: {other}")),
        }
        i += 1;
    }

    Ok(TranscribeArgs {
        dll_dir: dll_dir.ok_or_else(|| "missing required --dll-dir".to_string())?,
        model: model.ok_or_else(|| "missing required --model".to_string())?,
        wav: wav.ok_or_else(|| "missing required --wav".to_string())?,
        gpu,
        lang,
        threads,
        beam,
    })
}

fn parse_pill_bench(args: &[String]) -> Result<PillBenchArgs, String> {
    let mut out = PillBenchArgs {
        frames: 60,
        width: 280,
        height: 51,
        light: false,
        show_ms: 0,
    };

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--frames" => {
                let v = next_val(args, &mut i, flag)?;
                out.frames = v.parse().map_err(|e| format!("invalid --frames '{v}': {e}"))?;
            }
            "--width" => {
                let v = next_val(args, &mut i, flag)?;
                out.width = v.parse().map_err(|e| format!("invalid --width '{v}': {e}"))?;
            }
            "--height" => {
                let v = next_val(args, &mut i, flag)?;
                out.height = v.parse().map_err(|e| format!("invalid --height '{v}': {e}"))?;
            }
            "--light" => out.light = parse_bool(&next_val(args, &mut i, flag)?, flag)?,
            "--show-ms" => {
                let v = next_val(args, &mut i, flag)?;
                out.show_ms = v.parse().map_err(|e| format!("invalid --show-ms '{v}': {e}"))?;
            }
            other => return Err(format!("unknown flag for pill-bench: {other}")),
        }
        i += 1;
    }
    Ok(out)
}

fn parse_tray_bench(args: &[String]) -> Result<TrayBenchArgs, String> {
    let mut out = TrayBenchArgs { hold_ms: 500 };
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--hold-ms" => {
                let v = next_val(args, &mut i, flag)?;
                out.hold_ms = v.parse().map_err(|e| format!("invalid --hold-ms '{v}': {e}"))?;
            }
            other => return Err(format!("unknown flag for tray-bench: {other}")),
        }
        i += 1;
    }
    Ok(out)
}

fn emit_json<T: serde::Serialize>(val: &T) -> ExitCode {
    match serde_json::to_string(val) {
        Ok(s) => {
            println!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("JSON serialization error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let process_start = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(subcmd) = args.first().map(String::as_str) else {
        print_usage();
        return ExitCode::FAILURE;
    };

    let rest = &args[1..];
    match subcmd {
        "-h" | "--help" | "help" => {
            print_usage();
            ExitCode::SUCCESS
        }
        "transcribe" => match parse_transcribe(rest).and_then(|a| run_transcribe(&a).map_err(|e| e.to_string())) {
            Ok(out) => emit_json(&out),
            Err(e) => {
                println!("{}", serde_json::json!({ "ok": false, "error": e }));
                ExitCode::FAILURE
            }
        },
        "pill-bench" => match parse_pill_bench(rest).and_then(|a| run_pill_bench(&a)) {
            Ok(out) => emit_json(&out),
            Err(e) => {
                println!("{}", serde_json::json!({ "ok": false, "error": e }));
                ExitCode::FAILURE
            }
        },
        "tray-bench" => match parse_tray_bench(rest).and_then(|a| run_tray_bench(&a, process_start)) {
            Ok(out) => emit_json(&out),
            Err(e) => {
                println!("{}", serde_json::json!({ "ok": false, "error": e }));
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!("unknown subcommand: {other}");
            print_usage();
            ExitCode::FAILURE
        }
    }
}
