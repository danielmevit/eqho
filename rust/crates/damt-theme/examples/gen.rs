//! Generates the committed token outputs for an app:
//!
//! ```text
//! cargo run -p damt-theme --example gen -- <tokens.toml> <out_dir> [--check]
//! ```
//!
//! Writes `theme.slint`, `tokens.css` and `tokens.rs` into `<out_dir>`. With `--check` nothing is
//! written and the exit code is 1 when the committed files are missing or stale (like
//! `svg2axaml.py --check`).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use damt_theme::Tokens;

const USAGE: &str =
    "usage: cargo run -p damt-theme --example gen -- <tokens.toml> <out_dir> [--check]";

fn main() -> ExitCode {
    let mut check = false;
    let mut positional: Vec<String> = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--check" => check = true,
            _ if arg.starts_with('-') => {
                eprintln!("unknown option `{arg}`\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => positional.push(arg),
        }
    }
    if positional.len() != 2 {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let tokens_path = Path::new(&positional[0]);
    let out_dir = Path::new(&positional[1]);

    let source = match fs::read_to_string(tokens_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {e}", tokens_path.display());
            return ExitCode::from(2);
        }
    };
    let tokens = match Tokens::parse(&source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}: {e}", tokens_path.display());
            return ExitCode::from(1);
        }
    };

    let outputs = [
        ("theme.slint", tokens.to_slint()),
        ("tokens.css", tokens.to_css()),
        ("tokens.rs", tokens.to_rust()),
    ];

    if check {
        let mut stale = false;
        for (name, content) in &outputs {
            let path = out_dir.join(name);
            match fs::read_to_string(&path) {
                Ok(existing) if existing == *content => {}
                Ok(_) => {
                    eprintln!("stale: {} (regenerate without --check)", path.display());
                    stale = true;
                }
                Err(e) => {
                    eprintln!("missing: {} ({e})", path.display());
                    stale = true;
                }
            }
        }
        if stale {
            return ExitCode::from(1);
        }
        println!("{} is up to date", out_dir.display());
        return ExitCode::SUCCESS;
    }

    if let Err(e) = fs::create_dir_all(out_dir) {
        eprintln!("cannot create {}: {e}", out_dir.display());
        return ExitCode::from(2);
    }
    for (name, content) in &outputs {
        let path = out_dir.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("cannot write {}: {e}", path.display());
            return ExitCode::from(2);
        }
        println!("wrote {}", path.display());
    }
    ExitCode::SUCCESS
}
