//! Design tokens for the damt.xyz apps: one `tokens.toml` file is the single source of truth for an
//! app's colours (light and dark), corner radii, spacing and font sizes, and this crate validates it
//! and generates every consumer of it, so the app and its website cannot drift apart.
//!
//! - [`Tokens::parse`] validates the file (hex syntax, light AND dark for every colour, refs resolve
//!   without cycles, alpha within 0..=1).
//! - [`Tokens::to_slint`] emits an `export global Theme { … }` with an `in-out property <bool> dark`
//!   that switches every colour between its light and dark value.
//! - [`Tokens::to_css`] emits `:root { --color-…: … }` plus the dark values under both
//!   `@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) {…} }` and
//!   `:root[data-theme="dark"] {…}` (the scheme the websites use).
//! - [`Tokens::to_rust`] emits Rust constants for native code such as the overlays.
//!
//! Each app keeps its own `tokens.toml` (Ocy's lives in `rust/apps/ocy/theme/`); this crate owns the
//! format, the validation and the generators. Regenerate the committed outputs with
//! `cargo run -p damt-theme --example gen -- <tokens.toml> <out_dir>` (add `--check` in CI).
//!
//! Token names are kebab-case (`text-muted`). A colour may reference another token and re-tint its
//! alpha channel, mirroring the design-system rules ("Text @ 8 %"):
//!
//! ```toml
//! [color.divider]
//! ref = "text"
//! alpha = 0.08
//! ```
//!
//! `alpha` may differ per theme (`alpha = { light = 0.40, dark = 0.30 }`), and `light`/`dark` may
//! each be a hex string or a `{ ref = "…" }` table. The alpha replaces the referenced colour's alpha
//! channel (it does not multiply into it).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

mod emit;
mod parse;

use std::collections::BTreeMap;

/// A failed parse or generation, with the offending token path in the message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ThemeError(pub String);

/// A colour in RGBA channel order, the same order the hex syntax `#RRGGBBAA` uses.
///
/// The C# app writes `#AARRGGBB` (Avalonia); use [`Rgba::from_argb_hex`] when comparing with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    /// Parses `#RRGGBB` (opaque) or `#RRGGBBAA`, case-insensitive.
    pub fn from_hex(s: &str) -> Result<Rgba, ThemeError> {
        let digits = s
            .strip_prefix('#')
            .ok_or_else(|| ThemeError(format!("colour `{s}` must start with `#`")))?;
        if digits.len() != 6 && digits.len() != 8 {
            return Err(ThemeError(format!(
                "colour `{s}` must be `#RRGGBB` or `#RRGGBBAA`"
            )));
        }
        let mut channels = [0u8; 4];
        channels[3] = 0xFF;
        for (i, channel) in channels.iter_mut().enumerate().take(digits.len() / 2) {
            let hex = &digits[i * 2..i * 2 + 2];
            *channel = parse_hex_byte(hex)?;
        }
        Ok(Rgba {
            r: channels[0],
            g: channels[1],
            b: channels[2],
            a: channels[3],
        })
    }

    /// Formats as `#RRGGBBAA` (always eight digits, the same in Slint and CSS).
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
    }

    /// Parses Avalonia's `#RRGGBB` or `#AARRGGBB` channel order (used by the C# app's XAML).
    pub fn from_argb_hex(s: &str) -> Result<Rgba, ThemeError> {
        let digits = s
            .strip_prefix('#')
            .ok_or_else(|| ThemeError(format!("colour `{s}` must start with `#`")))?;
        if digits.len() != 6 && digits.len() != 8 {
            return Err(ThemeError(format!(
                "colour `{s}` must be `#RRGGBB` or `#AARRGGBB`"
            )));
        }
        let mut channels = [0u8; 4];
        channels[3] = 0xFF;
        for (i, channel) in channels.iter_mut().enumerate().take(digits.len() / 2) {
            let hex = &digits[i * 2..i * 2 + 2];
            *channel = parse_hex_byte(hex)?;
        }
        if digits.len() == 8 {
            Ok(Rgba {
                r: channels[1],
                g: channels[2],
                b: channels[3],
                a: channels[0],
            })
        } else {
            Ok(Rgba {
                r: channels[0],
                g: channels[1],
                b: channels[2],
                a: 0xFF,
            })
        }
    }

    /// Formats as `#AARRGGBB` (Avalonia's channel order).
    pub fn to_argb_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}{:02X}", self.a, self.r, self.g, self.b)
    }
}

/// A colour's resolved values: `light` and `dark` are both required, by design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorPair {
    pub light: Rgba,
    pub dark: Rgba,
}

/// A validated, fully resolved set of design tokens.
///
/// Refs and alphas are resolved at parse time, so every generator (and every consumer) sees plain
/// colours; nothing can drift between the Slint, CSS and Rust outputs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tokens {
    colors: BTreeMap<String, ColorPair>,
    radii: BTreeMap<String, f64>,
    spaces: BTreeMap<String, f64>,
    font_sizes: BTreeMap<String, f64>,
}

impl Tokens {
    /// Parses and validates a `tokens.toml` document.
    pub fn parse(source: &str) -> Result<Tokens, ThemeError> {
        parse::parse(source)
    }

    /// A colour by token name (`"text-muted"`).
    pub fn color(&self, name: &str) -> Option<ColorPair> {
        self.colors.get(name).copied()
    }

    /// All colours, ordered by name.
    pub fn colors(&self) -> impl Iterator<Item = (&str, ColorPair)> {
        self.colors.iter().map(|(k, v)| (k.as_str(), *v))
    }

    /// A corner radius by name (`"card"`), in px.
    pub fn radius(&self, name: &str) -> Option<f64> {
        self.radii.get(name).copied()
    }

    /// All corner radii, ordered by name.
    pub fn radii(&self) -> impl Iterator<Item = (&str, f64)> {
        self.radii.iter().map(|(k, v)| (k.as_str(), *v))
    }

    /// A spacing step by name (`"2"`), in px.
    pub fn space(&self, name: &str) -> Option<f64> {
        self.spaces.get(name).copied()
    }

    /// All spacing steps, ordered by name.
    pub fn spaces(&self) -> impl Iterator<Item = (&str, f64)> {
        self.spaces.iter().map(|(k, v)| (k.as_str(), *v))
    }

    /// A font size by name (`"body"`), in px.
    pub fn font_size(&self, name: &str) -> Option<f64> {
        self.font_sizes.get(name).copied()
    }

    /// All font sizes, ordered by name.
    pub fn font_sizes(&self) -> impl Iterator<Item = (&str, f64)> {
        self.font_sizes.iter().map(|(k, v)| (k.as_str(), *v))
    }

    /// Generates `theme.slint`: an `export global Theme` with one property per token and an
    /// `in-out property <bool> dark` that switches the colours between light and dark.
    pub fn to_slint(&self) -> String {
        emit::slint(self)
    }

    /// Generates `tokens.css`: light values on `:root`, dark values under both
    /// `@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) {…} }` and
    /// `:root[data-theme="dark"] {…}`.
    pub fn to_css(&self) -> String {
        emit::css(self)
    }

    /// Generates `tokens.rs`: Rust constants (`pub const ACCENT_LIGHT: Rgba = …`) for native code.
    pub fn to_rust(&self) -> String {
        emit::rust(self)
    }
}

/// Scales a 0..=1 alpha to a byte the way the design system states percentages: the exact decimal
/// product `alpha × 255` rounded half up (0.08 → 20, 0.30 → 77, 0.55 → 140).
///
/// The product is snapped to six fractional digits first so a decimal like `0.30` (exactly 76.5)
/// is not pushed down to 76 by f64 representation error.
pub(crate) fn scale_alpha(alpha: f64) -> u8 {
    let snapped = (alpha * 255.0 * 1e6).round() / 1e6;
    ((snapped + 0.5).floor()).clamp(0.0, 255.0) as u8
}

/// Parses two hex digits; the caller guarantees the slice length.
fn parse_hex_byte(s: &str) -> Result<u8, ThemeError> {
    u8::from_str_radix(s, 16).map_err(|_| ThemeError(format!("`{s}` is not a valid hex byte")))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let c = Rgba::from_hex("#F6F7F5").unwrap();
        assert_eq!(
            c,
            Rgba {
                r: 0xF6,
                g: 0xF7,
                b: 0xF5,
                a: 0xFF
            }
        );
        assert_eq!(c.to_hex(), "#F6F7F5FF");
        let c = Rgba::from_hex("#000000c7").unwrap();
        assert_eq!(c.a, 0xC7);
        assert_eq!(c.to_hex(), "#000000C7");
    }

    #[test]
    fn argb_round_trip() {
        // The C# DividerColor: #141B1E1F in ARGB = #1B1E1F14 in RGBA.
        let c = Rgba::from_argb_hex("#141B1E1F").unwrap();
        assert_eq!(
            c,
            Rgba {
                r: 0x1B,
                g: 0x1E,
                b: 0x1F,
                a: 0x14
            }
        );
        assert_eq!(c.to_hex(), "#1B1E1F14");
        assert_eq!(c.to_argb_hex(), "#141B1E1F");
        assert_eq!(
            Rgba::from_argb_hex("#F6F7F5").unwrap(),
            Rgba::from_hex("#F6F7F5").unwrap()
        );
    }

    #[test]
    fn rejects_bad_hex() {
        assert!(Rgba::from_hex("F6F7F5").is_err());
        assert!(Rgba::from_hex("#F6F7").is_err());
        assert!(Rgba::from_hex("#F6F7F5F").is_err());
        assert!(Rgba::from_hex("#GG0000").is_err());
        assert!(Rgba::from_hex("").is_err());
    }

    #[test]
    fn alpha_scaling_matches_the_design_percentages() {
        assert_eq!(scale_alpha(0.0), 0);
        assert_eq!(scale_alpha(0.08), 20); // divider: Text @ 8 %
        assert_eq!(scale_alpha(0.10), 26); // progress track
        assert_eq!(scale_alpha(0.12), 31); // outline pressed
        assert_eq!(scale_alpha(0.30), 77); // dark border: 0.30 * 255 = 76.5 exactly
        assert_eq!(scale_alpha(0.40), 102); // light border
        assert_eq!(scale_alpha(0.55), 140); // edge frame
        assert_eq!(scale_alpha(0.78), 199); // light overlay
        assert_eq!(scale_alpha(0.82), 209); // dark overlay
        assert_eq!(scale_alpha(1.0), 255);
    }
}
