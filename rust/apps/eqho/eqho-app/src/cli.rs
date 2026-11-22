//! Pure CLI argument parser for `Eqho.exe`.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::license::DemoLicense;

/// Parsed CLI command for `Eqho.exe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    /// Run the out-of-process `eqho-host` stdin/stdout worker loop.
    ModelHost,
    /// Run headless `--smoke` self-checks and print JSON report to stdout.
    Smoke { dll_dir: Option<PathBuf> },
    /// Run headless end-to-end WAV transcription through `ModelHost`.
    E2eWav {
        wav_path: PathBuf,
        model: Option<String>,
        dll_dir: Option<PathBuf>,
        model_dir: Option<PathBuf>,
        gpu: Option<bool>,
    },
    /// Run `<cycles>` rapid overlay/UI soak iterations and print JSON memory summary.
    Soak {
        cycles: usize,
        dll_dir: Option<PathBuf>,
    },
    /// Start the normal desktop tray application.
    Desktop {
        open_dashboard: bool,
        tab: Option<String>,
        dll_dir: Option<PathBuf>,
        demo_dashboard: bool,
        demo_license: Option<DemoLicense>,
    },
    /// Print CLI usage help and exit 0.
    Help,
    /// Print version string and exit 0.
    Version,
}

const VALID_TABS: [&str; 4] = ["general", "models", "history", "settings"];

/// Format CLI usage text.
#[must_use]
pub fn help_text() -> &'static str {
    "\
Eqho — Always-on local voice-to-text dictation

USAGE:
    Eqho.exe [OPTIONS]

OPTIONS:
    --open-dashboard        Open the dashboard window immediately on startup
    --tab=<name>            Select initial dashboard tab (general|models|history|settings)
    --dll-dir <dir>         Override directory containing whisper.dll / ggml*.dll
    --smoke                 Run headless self-checks and print JSON report
    --e2e-wav <path.wav>    Run headless E2E transcription benchmark via --model-host
      --model <key>         Model key for --e2e-wav (default: distil-large-v3)
      --model-dir <dir>     Model cache directory override for --e2e-wav
      --gpu <true|false>    Force GPU on/off for --e2e-wav
    --soak <cycles>         Run <cycles> rapid overlay/UI soak iterations
    --model-host            Internal: run out-of-process whisper.cpp worker loop
    -h, --help              Print help information
    -V, --version           Print version information
"
}

/// Parse CLI arguments (excluding `argv[0]`).
pub fn parse_args<I, S>(args: I) -> Result<CliCommand, String>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let raw: Vec<String> = args
        .into_iter()
        .map(|s| s.into().to_string_lossy().into_owned())
        .collect();

    if raw.iter().any(|a| a == "--model-host") {
        return Ok(CliCommand::ModelHost);
    }

    let mut is_help = false;
    let mut is_version = false;
    let mut is_smoke = false;
    let mut e2e_wav: Option<PathBuf> = None;
    let mut soak_cycles: Option<usize> = None;
    let mut open_dashboard = false;
    let mut tab: Option<String> = None;
    let mut dll_dir: Option<PathBuf> = None;
    let mut demo_dashboard = false;
    let mut demo_license: Option<DemoLicense> = None;
    let mut model: Option<String> = None;
    let mut model_dir: Option<PathBuf> = None;
    let mut gpu: Option<bool> = None;

    let mut i = 0;
    while i < raw.len() {
        let arg = &raw[i];
        match arg.as_str() {
            "-h" | "--help" => {
                is_help = true;
            }
            "-V" | "--version" => {
                is_version = true;
            }
            "--smoke" => {
                is_smoke = true;
            }
            "--open-dashboard" => {
                open_dashboard = true;
            }
            "--demo-dashboard" => {
                demo_dashboard = true;
                open_dashboard = true;
            }
            "--demo-license" => {
                i += 1;
                let val = raw
                    .get(i)
                    .ok_or_else(|| {
                        "--demo-license requires a state (full|trial|trial-last-day|trial-ended|unknown)"
                            .to_owned()
                    })?;
                demo_license = Some(validate_demo_license(val)?);
            }
            "--dll-dir" => {
                i += 1;
                let val = raw
                    .get(i)
                    .ok_or_else(|| "--dll-dir requires a directory path".to_owned())?;
                dll_dir = Some(PathBuf::from(val));
            }
            "--e2e-wav" => {
                i += 1;
                let val = raw
                    .get(i)
                    .ok_or_else(|| "--e2e-wav requires a WAV file path".to_owned())?;
                e2e_wav = Some(PathBuf::from(val));
            }
            "--soak" => {
                i += 1;
                let val = raw
                    .get(i)
                    .ok_or_else(|| "--soak requires a cycle count".to_owned())?;
                let parsed = val
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --soak cycle count: {val}"))?;
                if parsed == 0 {
                    return Err("--soak cycle count must be >= 1".to_owned());
                }
                soak_cycles = Some(parsed);
            }
            "--model" => {
                i += 1;
                let val = raw
                    .get(i)
                    .ok_or_else(|| "--model requires a model key".to_owned())?;
                model = Some(val.clone());
            }
            "--model-dir" => {
                i += 1;
                let val = raw
                    .get(i)
                    .ok_or_else(|| "--model-dir requires a directory path".to_owned())?;
                model_dir = Some(PathBuf::from(val));
            }
            "--gpu" => {
                i += 1;
                let val = raw
                    .get(i)
                    .ok_or_else(|| "--gpu requires true or false".to_owned())?;
                gpu = Some(parse_bool_flag("--gpu", val)?);
            }
            "--tab" => {
                i += 1;
                let val = raw
                    .get(i)
                    .ok_or_else(|| "--tab requires a tab name".to_owned())?;
                tab = Some(validate_tab(val)?);
            }
            _ => {
                if let Some(val) = arg.strip_prefix("--tab=") {
                    tab = Some(validate_tab(val)?);
                } else if let Some(val) = arg.strip_prefix("--dll-dir=") {
                    dll_dir = Some(PathBuf::from(val));
                } else if let Some(val) = arg.strip_prefix("--demo-license=") {
                    demo_license = Some(validate_demo_license(val)?);
                } else if let Some(val) = arg.strip_prefix("--model=") {
                    model = Some(val.to_owned());
                } else if let Some(val) = arg.strip_prefix("--model-dir=") {
                    model_dir = Some(PathBuf::from(val));
                } else if let Some(val) = arg.strip_prefix("--gpu=") {
                    gpu = Some(parse_bool_flag("--gpu", val)?);
                } else if let Some(val) = arg.strip_prefix("--e2e-wav=") {
                    e2e_wav = Some(PathBuf::from(val));
                } else if let Some(val) = arg.strip_prefix("--soak=") {
                    let parsed = val
                        .parse::<usize>()
                        .map_err(|_| format!("invalid --soak cycle count: {val}"))?;
                    if parsed == 0 {
                        return Err("--soak cycle count must be >= 1".to_owned());
                    }
                    soak_cycles = Some(parsed);
                } else {
                    return Err(format!("unknown argument: {arg}"));
                }
            }
        }
        i += 1;
    }

    if is_help {
        return Ok(CliCommand::Help);
    }
    if is_version {
        return Ok(CliCommand::Version);
    }
    if is_smoke {
        return Ok(CliCommand::Smoke { dll_dir });
    }
    if let Some(wav_path) = e2e_wav {
        return Ok(CliCommand::E2eWav {
            wav_path,
            model,
            dll_dir,
            model_dir,
            gpu,
        });
    }
    if let Some(cycles) = soak_cycles {
        return Ok(CliCommand::Soak { cycles, dll_dir });
    }

    Ok(CliCommand::Desktop {
        open_dashboard,
        tab,
        dll_dir,
        demo_dashboard,
        demo_license,
    })
}

fn validate_tab(raw: &str) -> Result<String, String> {
    let lower = raw.trim().to_ascii_lowercase();
    if VALID_TABS.contains(&lower.as_str()) {
        Ok(lower)
    } else {
        Err(format!(
            "invalid --tab value '{raw}' (expected one of: general, models, history, settings)"
        ))
    }
}

fn validate_demo_license(raw: &str) -> Result<DemoLicense, String> {
    DemoLicense::parse(raw).ok_or_else(|| {
        format!(
            "invalid --demo-license value '{raw}' (expected one of: full, trial, trial-last-day, trial-ended, unknown)"
        )
    })
}

fn parse_bool_flag(flag: &str, raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(format!("invalid {flag} value '{raw}' (expected true or false)")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_model_host() {
        assert_eq!(parse_args(["--model-host"]).unwrap(), CliCommand::ModelHost);
    }

    #[test]
    fn parses_smoke_with_optional_dll_dir() {
        assert_eq!(
            parse_args(["--smoke"]).unwrap(),
            CliCommand::Smoke { dll_dir: None }
        );
        assert_eq!(
            parse_args(["--smoke", "--dll-dir", "/tmp/dlls"]).unwrap(),
            CliCommand::Smoke {
                dll_dir: Some(PathBuf::from("/tmp/dlls"))
            }
        );
    }

    #[test]
    fn parses_e2e_wav_with_all_options() {
        let cmd = parse_args([
            "--e2e-wav",
            "sample.wav",
            "--model",
            "tiny",
            "--dll-dir",
            "dlls",
            "--model-dir",
            "models",
            "--gpu",
            "false",
        ])
        .unwrap();
        assert_eq!(
            cmd,
            CliCommand::E2eWav {
                wav_path: PathBuf::from("sample.wav"),
                model: Some("tiny".to_owned()),
                dll_dir: Some(PathBuf::from("dlls")),
                model_dir: Some(PathBuf::from("models")),
                gpu: Some(false),
            }
        );
    }

    #[test]
    fn parses_soak() {
        assert_eq!(
            parse_args(["--soak", "5"]).unwrap(),
            CliCommand::Soak {
                cycles: 5,
                dll_dir: None
            }
        );
        assert!(parse_args(["--soak", "0"]).is_err());
    }

    #[test]
    fn parses_open_dashboard_and_tab() {
        assert_eq!(
            parse_args(["--open-dashboard", "--tab=models"]).unwrap(),
            CliCommand::Desktop {
                open_dashboard: true,
                tab: Some("models".to_owned()),
                dll_dir: None,
                demo_dashboard: false,
                demo_license: None,
            }
        );
        assert!(parse_args(["--tab=bogus"]).is_err());
    }

    #[test]
    fn rejects_unknown_flags() {
        let err = parse_args(["--does-not-exist"]).unwrap_err();
        assert!(err.contains("unknown argument"));
    }

    #[test]
    fn parses_help_and_version() {
        assert_eq!(parse_args(["--help"]).unwrap(), CliCommand::Help);
        assert_eq!(parse_args(["-h"]).unwrap(), CliCommand::Help);
        assert_eq!(parse_args(["--version"]).unwrap(), CliCommand::Version);
        assert_eq!(parse_args(["-V"]).unwrap(), CliCommand::Version);
    }
}
