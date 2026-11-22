//! Eqho Slint 1.19 software-rendered UI crate.
#![deny(clippy::unwrap_used, clippy::expect_used)]

#[allow(clippy::all)]
pub mod slint_generated {
    slint::include_modules!();
}

pub use slint_generated::*;
