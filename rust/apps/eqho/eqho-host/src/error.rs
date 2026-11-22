//! Error types for `eqho-host` (child worker and parent `ModelHost`).

use thiserror::Error;

/// Errors originating inside the child model host or framing/FFI/download layers.
#[derive(Debug, Error)]
pub enum HostError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON framing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("audio frame too large: {samples} samples exceeds max {max_samples}")]
    OversizedAudio { samples: usize, max_samples: usize },

    #[error("invalid protocol frame: {0}")]
    InvalidFrame(String),

    #[error("audio buffer exceeds c_int length ({0} samples)")]
    AudioTooLarge(usize),

    #[error("failed to load whisper library from {path}: {source}")]
    LoadLibrary {
        path: String,
        source: libloading::Error,
    },

    #[error("missing symbol {symbol}: {source}")]
    MissingSymbol {
        symbol: &'static str,
        source: libloading::Error,
    },

    #[error("Windows DLL directory setup failed: {0}")]
    #[cfg_attr(not(windows), allow(dead_code))]
    DllDirSetup(String),

    #[error("whisper default params pointer returned null")]
    NullDefaultParams,

    #[error("invalid C string argument: {0}")]
    InvalidCString(#[from] std::ffi::NulError),

    #[error("whisper_init_from_file_with_params returned null for model {0}")]
    InitModelFailed(String),

    #[error("whisper_full failed with error code {0}")]
    InferenceFailed(i32),

    #[error("invalid HTTPS URL: {0}")]
    InvalidUrl(String),

    #[error("model download failed: {0}")]
    DownloadFailed(String),
}

/// Typed errors surfaced by [`crate::ModelHost`] to the parent application.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ModelHostError {
    #[error("failed to spawn model host process: {0}")]
    SpawnFailed(String),

    #[error("not enough free memory to load or run model: {0}")]
    LowMemory(String),

    #[error("model load failed: {0}")]
    LoadFailed(String),

    #[error("transcription failed: {0}")]
    TranscribeFailed(String),

    #[error("model host timed out: {0}")]
    Timeout(String),

    #[error("model host is not ready")]
    NotReady,

    #[error("model host protocol error: {0}")]
    Protocol(String),
}
