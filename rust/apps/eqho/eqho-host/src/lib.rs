//! Out-of-process `whisper.cpp 1.8.4` model host.
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod client;
pub mod downloader;
pub mod error;
pub mod protocol;
pub mod whisper_ffi;
pub mod worker;

pub use client::{ModelHost, ModelHostConfig, ModelInfo};
pub use error::{HostError, ModelHostError};
pub use protocol::{HostRequest, HostResponse, HostSegment};
pub use worker::run_child_stdio_loop;
