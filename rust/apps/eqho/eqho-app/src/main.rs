//! Eqho desktop application binary entry point.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;

use eqho_app::app::run_desktop_app;
use eqho_app::cli::{CliCommand, help_text, parse_args};
use eqho_app::headless::{run_e2e_wav_cli, run_smoke_cli, run_soak_cli};

#[cfg(windows)]
fn attach_parent_console() {
    use windows::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    // SAFETY: Queries the process stdout handle and only attaches to the parent console when
    // stdout is not already redirected (e.g., pipes/files from WSL or CI stay intact).
    unsafe {
        let has_stdout = GetStdHandle(STD_OUTPUT_HANDLE)
            .map(|h| !h.is_invalid() && !h.0.is_null())
            .unwrap_or(false);
        if !has_stdout {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}

fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--model-host") {
        let code = eqho_host::run_child_stdio_loop();
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }

    match parse_args(std::env::args_os().skip(1)) {
        Ok(CliCommand::ModelHost) => {
            let code = eqho_host::run_child_stdio_loop();
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Ok(CliCommand::Help) => {
            attach_parent_console();
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(CliCommand::Version) => {
            attach_parent_console();
            println!("Eqho {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(CliCommand::Smoke { dll_dir }) => {
            attach_parent_console();
            run_smoke_cli(dll_dir)
        }
        Ok(CliCommand::E2eWav {
            wav_path,
            model,
            dll_dir,
            model_dir,
            gpu,
        }) => {
            attach_parent_console();
            run_e2e_wav_cli(wav_path, model, dll_dir, model_dir, gpu)
        }
        Ok(CliCommand::Soak { cycles, dll_dir }) => {
            attach_parent_console();
            run_soak_cli(cycles, dll_dir)
        }
        Ok(CliCommand::Toggle) => {
            attach_parent_console();
            eqho_app::app::send_toggle_ipc()
        }
        Ok(CliCommand::Desktop {
            open_dashboard,
            tab,
            dll_dir,
            demo_dashboard,
            demo_license,
        }) => {
            if demo_license.is_some()
                && !eqho_app::license::demo_license_is_honoured(demo_dashboard)
            {
                attach_parent_console();
            }
            run_desktop_app(open_dashboard, tab, dll_dir, demo_dashboard, demo_license)
        }
        Err(err) => {
            attach_parent_console();
            eprintln!("error: {err}\n\n{}", help_text());
            ExitCode::from(2)
        }
    }
}
