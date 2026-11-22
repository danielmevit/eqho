//! Eqho desktop application entry point (scaffold for R3c-eqho-app).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--model-host") {
        let code = eqho_host::run_child_stdio_loop();
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    ExitCode::SUCCESS
}
