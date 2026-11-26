//! Headless `--smoke` self-check: settings, textproc, VAD/FFT, history, hardware probes,
//! and optional live `--model-host` verification.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use eqho_core::history::TranscriptHistory;
use eqho_core::modelstore::whispercpp_model_path;
use eqho_core::settings::Settings;
use eqho_core::textproc::{
    apply_replacements, clean_text, is_delete_command, match_command, smart_join,
};
use eqho_core::vad::{
    BlockAssembler, CHUNK_SAMPLES, PhraseSegmenter, SAMPLE_RATE, Segment, VadEvent,
    WAVE_BLOCK_SAMPLES, filter_segments, level_meter, voice_features,
};
use eqho_host::{ModelHost, ModelHostConfig};
use serde::Serialize;

use crate::traits::EqhoPlatform;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SmokeCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SmokeReport {
    pub ok: bool,
    pub checks: Vec<SmokeCheck>,
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("eqho_smoke_{tag}_{}_{nanos}", std::process::id()))
}

fn check_settings_roundtrip() -> SmokeCheck {
    let dir = temp_dir("settings");
    let path = dir.join("settings.json");
    let res = (|| -> Result<String, String> {
        let s = Settings {
            language: "fr".to_owned(),
            hotkey: "ctrl+shift+space".to_owned(),
            ..Settings::default()
        };
        s.save_to_path(&path).map_err(|e| e.to_string())?;
        let loaded = Settings::load_from_path(&path);
        if loaded != s {
            return Err("loaded settings did not match saved settings".into());
        }
        Ok("ok".into())
    })();
    let _ = fs::remove_dir_all(&dir);
    match res {
        Ok(detail) => SmokeCheck {
            name: "settings_roundtrip".into(),
            ok: true,
            detail,
        },
        Err(detail) => SmokeCheck {
            name: "settings_roundtrip".into(),
            ok: false,
            detail,
        },
    }
}

fn check_textproc_invariants() -> SmokeCheck {
    let mut map = std::collections::BTreeMap::new();
    map.insert("eqho".to_owned(), "Eqho".to_owned());

    let ok = match_command("Period.") == Some(".")
        && match_command("New line") == Some("\n")
        && match_command("hello world").is_none()
        && is_delete_command("Delete that")
        && apply_replacements("hello eqho app", &map) == "hello Eqho app"
        && smart_join(&["hello", ".", "world"]) == "hello. world"
        && smart_join(&["one", "\n", "two"]) == "one\ntwo"
        && clean_text("hello world", "en", false) == "Hello world"
        && clean_text("i think i'm right . yes", "en", false) == "I think I'm right. Yes"
        && clean_text("um so uh hi", "en", true) == "So hi"
        && clean_text(&clean_text("i said hi", "en", false), "en", false)
            == clean_text("i said hi", "en", false);

    SmokeCheck {
        name: "textproc_invariants".into(),
        ok,
        detail: if ok {
            "ok".into()
        } else {
            "textproc invariant assertion failed".into()
        },
    }
}

fn check_vad_fft_sanity() -> SmokeCheck {
    let mut asm = BlockAssembler::new(CHUNK_SAMPLES);
    let block = vec![0.1_f32; WAVE_BLOCK_SAMPLES];
    let mut chunks = Vec::new();
    for _ in 0..10 {
        chunks.extend(asm.push(&block));
    }
    let (loud, tone) = voice_features(&block);
    let meter = level_meter(0.012);

    let mut seg = PhraseSegmenter::new(1.0, 2, 30.0);
    let silence = vec![0.0_f32; CHUNK_SAMPLES];
    let _ = seg.push_chunk(&silence);
    let speech = vec![0.05_f32; CHUNK_SAMPLES];
    let _ = seg.push_chunk(&speech);
    let flushed = seg.flush_on_stop();

    let seg_filtered = filter_segments(
        &[Segment {
            text: "hello world".into(),
            no_speech_prob: 0.05,
            avg_logprob: -0.2,
        }],
        1.0,
        Some(0.05),
        0.003,
    );

    let ok = chunks.len() == 1
        && (0.0..=1.0).contains(&loud)
        && (0.0..=1.0).contains(&tone)
        && (meter - 1.0).abs() < 1e-5
        && matches!(flushed, Some(VadEvent::PhraseReady { .. }))
        && seg_filtered.as_deref() == Some("hello world");

    SmokeCheck {
        name: "vad_fft_sanity".into(),
        ok,
        detail: if ok {
            "ok".into()
        } else {
            "VAD/FFT sanity check failed".into()
        },
    }
}

fn check_history_roundtrip() -> SmokeCheck {
    let dir = temp_dir("history");
    let path = dir.join("history.jsonl");
    let res = (|| -> Result<String, String> {
        let hist = TranscriptHistory::new(&path, 3);
        for i in 0..5 {
            hist.append_with_ts(1000.0 + f64::from(i), &format!("entry {i}"), 1.0, "tiny", "en")
                .map_err(|e| e.to_string())?;
        }
        let entries = hist.read_all();
        if entries.len() != 3 || entries[0].text != "entry 4" {
            return Err("history prune/order mismatch".into());
        }
        if hist.search("entry 3").len() != 1 {
            return Err("history search mismatch".into());
        }
        Ok("ok".into())
    })();
    let _ = fs::remove_dir_all(&dir);
    match res {
        Ok(detail) => SmokeCheck {
            name: "history_roundtrip".into(),
            ok: true,
            detail,
        },
        Err(detail) => SmokeCheck {
            name: "history_roundtrip".into(),
            ok: false,
            detail,
        },
    }
}

fn check_hardware_probes(platform: &dyn EqhoPlatform) -> SmokeCheck {
    let adapters = platform.list_adapters();
    let avx2 = platform.cpu_has_avx2();
    let free_mb = platform.free_memory_mb();
    let devices = platform.list_input_devices();
    let detail = format!(
        "gpus={}, avx2={avx2}, free_mb={free_mb:?}, audio_devices={}",
        adapters.len(),
        devices.len()
    );
    SmokeCheck {
        name: "hardware_probes".into(),
        ok: true,
        detail,
    }
}

fn dll_dir_has_whisper(dir: &std::path::Path) -> bool {
    #[cfg(windows)]
    const NAMES: &[&str] = &["whisper.dll"];
    #[cfg(target_os = "macos")]
    const NAMES: &[&str] = &["libwhisper.dylib", "libwhisper.1.dylib"];
    #[cfg(all(unix, not(target_os = "macos")))]
    const NAMES: &[&str] = &["libwhisper.so", "libwhisper.so.1"];
    NAMES.iter().any(|n| dir.join(n).is_file())
}

fn check_live_model_host(cfg: &ModelHostConfig) -> SmokeCheck {
    if !cfg.dll_dir.exists() || !dll_dir_has_whisper(&cfg.dll_dir) {
        return SmokeCheck {
            name: "live_model_host".into(),
            ok: true,
            detail: "skipped (whisper shared library absent)".into(),
        };
    }

    let model_dir = Settings::default().resolve_model_dir();
    let model_path = whispercpp_model_path(&model_dir, "tiny");
    if !model_path.exists() {
        return match eqho_host::whisper_ffi::WhisperEngine::probe_library(&cfg.dll_dir) {
            Ok((ver, sys)) => {
                let sys_trimmed = sys.trim();
                let upper = sys_trimmed.to_ascii_uppercase();
                let has_backend = upper.contains("CPU")
                    || upper.contains("METAL")
                    || upper.contains("BLAS")
                    || upper.contains("VULKAN");
                SmokeCheck {
                    name: "live_model_host".into(),
                    ok: has_backend,
                    detail: if has_backend {
                        format!("library ok: whisper.cpp {ver} ({sys_trimmed})")
                    } else {
                        format!(
                            "library loaded whisper.cpp {ver} but zero GGML backends registered ({sys_trimmed})"
                        )
                    },
                }
            }
            Err(err) => SmokeCheck {
                name: "live_model_host".into(),
                ok: false,
                detail: format!("library load failed: {err}"),
            },
        };
    }

    let mut host = ModelHost::new(cfg.clone());
    let res = (|| -> Result<String, String> {
        let info = host
            .load_model("tiny", &model_dir, false, 4, &mut |_, _| {})
            .map_err(|e| e.to_string())?;
        let silence = vec![0.0_f32; SAMPLE_RATE / 2];
        let segs = host
            .transcribe(&silence, "en", 1, "")
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "backend={}, device={}, segments={}",
            info.backend,
            info.device,
            segs.len()
        ))
    })();
    host.shutdown();

    match res {
        Ok(detail) => SmokeCheck {
            name: "live_model_host".into(),
            ok: true,
            detail,
        },
        Err(detail) => SmokeCheck {
            name: "live_model_host".into(),
            ok: false,
            detail,
        },
    }
}

/// Run the headless `--smoke` self-check suite.
pub fn run_smoke(
    platform: &dyn EqhoPlatform,
    model_host_cfg: Option<&ModelHostConfig>,
) -> SmokeReport {
    let mut checks = vec![
        check_settings_roundtrip(),
        check_textproc_invariants(),
        check_vad_fft_sanity(),
        check_history_roundtrip(),
        check_hardware_probes(platform),
    ];
    if let Some(cfg) = model_host_cfg {
        checks.push(check_live_model_host(cfg));
    }
    let ok = checks.iter().all(|c| c.ok);
    SmokeReport { ok, checks }
}
