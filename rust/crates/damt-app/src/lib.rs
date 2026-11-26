//! App-shell services shared by the damt.xyz apps.
//!
//! - [`SingleInstance`]: one running instance per app id, with a signal channel from later launches.
//! - [`AppDataPaths`]: the per-OS data directory.
//! - [`ErrorLog`]: a size-capped file log, plus [`install_panic_hook`].
//!
//! The Windows paths use named kernel objects and the `windows` crate. The non-Windows paths are behind
//! `cfg` and use a lock file in the app's data directory.

#![deny(clippy::unwrap_used, clippy::expect_used)]

mod analytics;
mod error_log;
mod paths;
mod pure;
mod single_instance;

pub use analytics::{
    AnalyticsClient, AnalyticsConfig, AnalyticsProp, DEFAULT_POSTHOG_EU_HOST,
    build_capture_payload, generate_anonymous_id, parse_https_capture_target,
};
pub use error_log::{ErrorLog, install_panic_hook};
pub use paths::{AppDataPaths, data_directory, ensure_data_directory};
pub use single_instance::{Acquire, SingleInstance, SingleInstanceGuard};
