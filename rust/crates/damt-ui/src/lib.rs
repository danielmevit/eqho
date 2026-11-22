//! damt-ui: the app-neutral Slint component kit.
//!
//! The `.slint` sources live in `ui/`. A consuming crate maps two library paths in its build script:
//! `@damt-ui` to [`ui_dir`] and `@app-theme` to the directory holding its generated `theme.slint`
//! (which must `export global Theme`). Then `.slint` files import `@damt-ui/kit.slint`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

/// Library-path name the kit expects for itself.
pub const LIBRARY_NAME: &str = "damt-ui";
/// Library-path name the kit imports `Theme` from.
pub const THEME_LIBRARY_NAME: &str = "app-theme";

/// Directory holding the kit's `.slint` files (`kit.slint`).
pub fn ui_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/ui"))
}

/// Path to the kit's entry file.
pub fn kit_path() -> PathBuf {
    ui_dir().join("kit.slint")
}
