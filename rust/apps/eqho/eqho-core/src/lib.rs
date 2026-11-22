//! Pure-Rust Eqho core logic: settings, transcript history, deterministic text
//! processing, adaptive VAD, voice features, iridescent pill renderer, chime
//! synthesis, model selection, and last-dictation recovery.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(missing_debug_implementations)]

pub mod chime;
pub mod history;
pub mod modelpick;
pub mod modelstore;
pub mod pillfx;
pub mod recovery;
pub mod settings;
pub mod textproc;
pub mod vad;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
