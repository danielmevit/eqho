//! Eqho Slint 1.19 software-rendered UI crate.
#![deny(clippy::unwrap_used, clippy::expect_used)]

#[allow(
    clippy::all,
    clippy::unwrap_used,
    clippy::expect_used,
    missing_debug_implementations
)]
pub mod slint_generated {
    slint::include_modules!();
}

pub mod flags;
pub mod logo;
pub mod viewmodel;

pub use flags::*;
pub use logo::*;
pub use slint_generated::*;
pub use viewmodel::*;
