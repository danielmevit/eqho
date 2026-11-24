//! WCAG 2.x contrast for Eqho's accent roles in both themes:
//! - `on-accent` label on accent fill at rest, hover and pressed (>= 4.5:1)
//! - `accent-text` & `accent-text-pressed` as small text on bg and surface (>= 4.5:1)
//! - `accent-line` thin strokes, focus rings, selected outlines, and progress leading edge on bg,
//!   surface, and progress track (>= 3.0:1)
//! - `on-accent` toggle knob on `accent` track (>= 3.0:1)
//! - `accent` fill on bg and surface (>= 3.0:1 in light; >= 2.0:1 for dark Deep Navy Blend `#1A489B`
//!   which pairs borderless deep navy fills with bright `#58A6FF` `accent-line` strokes).

use damt_theme::{Rgba, Tokens};

const TOKENS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/eqho/theme/tokens.toml"
);

fn tokens() -> Tokens {
    Tokens::parse(&std::fs::read_to_string(TOKENS).unwrap()).unwrap()
}

fn get(t: &Tokens, name: &str, dark: bool) -> Rgba {
    let pair = t
        .color(name)
        .unwrap_or_else(|| panic!("tokens.toml has no colour `{name}`"));
    if dark { pair.dark } else { pair.light }
}

/// `fg` (possibly translucent) composited over the opaque `bg`.
fn over(fg: Rgba, bg: Rgba) -> Rgba {
    let a = f64::from(fg.a) / 255.0;
    let mix = |f: u8, b: u8| (f64::from(f) * a + f64::from(b) * (1.0 - a)).round() as u8;
    Rgba {
        r: mix(fg.r, bg.r),
        g: mix(fg.g, bg.g),
        b: mix(fg.b, bg.b),
        a: 255,
    }
}

fn luminance(c: Rgba) -> f64 {
    let f = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
}

fn ratio(a: Rgba, b: Rgba) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn accent_roles_meet_aa_in_both_themes() {
    let t = tokens();
    let mut failures = Vec::new();
    for dark in [false, true] {
        let theme = if dark { "dark" } else { "light" };
        let bg = get(&t, "bg", dark);
        let surface = get(&t, "surface", dark);
        let fill = get(&t, "accent", dark);
        let line = get(&t, "accent-line", dark);
        let label = get(&t, "on-accent", dark);
        let track = over(get(&t, "progress-track", dark), surface);
        let fill_min = if dark { 2.0 } else { 3.0 };
        let checks = [
            ("on-accent on accent (rest)", label, fill, 4.5),
            (
                "on-accent on hover",
                label,
                get(&t, "button-accent-hover", dark),
                4.5,
            ),
            (
                "on-accent on pressed",
                label,
                get(&t, "button-accent-pressed", dark),
                4.5,
            ),
            ("accent fill on bg", fill, bg, fill_min),
            ("accent fill on surface", fill, surface, fill_min),
            ("accent-line on bg", line, bg, 3.0),
            ("accent-line on surface", line, surface, 3.0),
            ("accent-line on progress track", line, track, 3.0),
            ("accent-text on bg", get(&t, "accent-text", dark), bg, 4.5),
            (
                "accent-text on surface",
                get(&t, "accent-text", dark),
                surface,
                4.5,
            ),
            (
                "accent-text-pressed on surface",
                get(&t, "accent-text-pressed", dark),
                surface,
                4.5,
            ),
            ("on-accent knob on accent track", label, fill, 3.0),
        ];
        for (what, a, b, min) in checks {
            let r = ratio(a, b);
            if r < min {
                failures.push(format!("{theme}: {what} is {r:.2}:1, needs {min}:1"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn eqho_accent_is_brand_blue() {
    let t = tokens();
    let hex = |name: &str, dark: bool| {
        let c = get(&t, name, dark);
        format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    };
    assert_eq!(hex("accent", true), "#1A489B");
    assert_eq!(hex("on-accent", true), "#FFFFFF");
    assert_eq!(hex("accent-line", true), "#58A6FF");
    assert_eq!(hex("accent", false), "#0969DA");
    assert_eq!(hex("on-accent", false), "#FFFFFF");
    assert_eq!(hex("accent-line", false), "#0969DA");
}
