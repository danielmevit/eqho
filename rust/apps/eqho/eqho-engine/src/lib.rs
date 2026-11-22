//! Eqho dictation coordinator, WAV test source, and headless smoke runner.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(missing_debug_implementations)]

pub mod coordinator;
pub mod smoke;
pub mod traits;
pub mod wav_source;

pub use coordinator::DictationCoordinator;
pub use smoke::{SmokeCheck, SmokeReport, run_smoke};
pub use traits::{EqhoPlatform, NoopUiSink, SpeechBackend, UiEventSink};
pub use wav_source::{TestAudioStream, load_wav_16k_mono};
