//! The `tokens.toml` parser and validator.
//!
//! Grammar (kebab-case names everywhere):
//!
//! ```toml
//! [color.bg]                  # a colour with explicit values
//! light = "#F6F7F5"           # #RRGGBB or #RRGGBBAA
//! dark = "#17191A"
//!
//! [color.overlay]
//! light = "#000000C7"
//! dark = "#000000D1"
//!
//! [color.divider]             # a colour derived from another token
//! ref = "text"                # refs resolve; cycles are rejected
//! alpha = 0.08                # 0..=1, replaces the target's alpha channel
//!
//! [color.border]              # alpha (and light/dark refs) may differ per theme
//! ref = "text-muted"
//! alpha = { light = 0.40, dark = 0.30 }
//!
//! [radius]                    # px
//! card = 8
//!
//! [space]                     # px
//! 2 = 2
//!
//! [font.size]                 # px
//! body = 14
//! ```
//!
//! Unknown tables and keys are rejected, so a typo cannot silently drop a token.

use std::collections::{BTreeMap, BTreeSet};

use crate::{ColorPair, Rgba, ThemeError, Tokens, scale_alpha};

/// A single colour value before resolution: a hex literal or a reference.
enum RawValue {
    Hex(String),
    Ref {
        reference: String,
        alpha: Option<RawAlpha>,
    },
}

/// An alpha that may be shared or per-theme.
#[derive(Clone, Copy)]
enum RawAlpha {
    Both(f64),
    Themes { light: f64, dark: f64 },
}

impl RawAlpha {
    fn light(self) -> f64 {
        match self {
            RawAlpha::Both(a) => a,
            RawAlpha::Themes { light, .. } => light,
        }
    }

    fn dark(self) -> f64 {
        match self {
            RawAlpha::Both(a) => a,
            RawAlpha::Themes { dark, .. } => dark,
        }
    }
}

/// One `[color.<name>]` entry before resolution.
enum RawColor {
    /// `light`/`dark` keys, each a hex string or a `{ ref = "…" }` table.
    Themes { light: RawValue, dark: RawValue },
    /// A shared `ref` (+ optional `alpha`) applied to both themes.
    Shared {
        reference: String,
        alpha: Option<RawAlpha>,
    },
}

/// Parses and validates a `tokens.toml` document into fully resolved [`Tokens`].
pub(crate) fn parse(source: &str) -> Result<Tokens, ThemeError> {
    let root: toml::Value =
        toml::from_str(source).map_err(|e| ThemeError(format!("invalid TOML: {e}")))?;
    let root = root
        .as_table()
        .ok_or_else(|| ThemeError("the document root must be a table".to_string()))?;

    for key in root.keys() {
        if !matches!(key.as_str(), "color" | "radius" | "space" | "font") {
            return Err(ThemeError(format!(
                "unknown table `[{key}]` (expected color, radius, space, font)"
            )));
        }
    }

    let mut colors = BTreeMap::new();
    if let Some(color) = root.get("color") {
        let table = color
            .as_table()
            .ok_or_else(|| ThemeError("`[color]` must be a table".to_string()))?;
        let mut raw = BTreeMap::new();
        for (name, entry) in table {
            raw.insert(name.clone(), raw_color(name, entry)?);
        }
        for name in raw.keys() {
            resolve_color(name, &raw, &mut colors, &mut BTreeSet::new())?;
        }
    }

    let radii = number_table(root.get("radius"), "radius")?;
    let spaces = number_table(root.get("space"), "space")?;
    let font_sizes = number_table(root.get("font").and_then(|f| f.get("size")), "font.size")?;
    if let Some(font) = root.get("font") {
        let table = font
            .as_table()
            .ok_or_else(|| ThemeError("`[font]` must be a table".to_string()))?;
        for key in table.keys() {
            if key != "size" {
                return Err(ThemeError(format!(
                    "unknown key `{key}` in `[font]` (only `size` is allowed)"
                )));
            }
        }
    }

    Ok(Tokens {
        colors,
        radii,
        spaces,
        font_sizes,
    })
}

/// Reads a `[<name>]` table of token → px numbers.
fn number_table(
    value: Option<&toml::Value>,
    table: &str,
) -> Result<BTreeMap<String, f64>, ThemeError> {
    let mut out = BTreeMap::new();
    let Some(value) = value else { return Ok(out) };
    let table_value = value
        .as_table()
        .ok_or_else(|| ThemeError(format!("`[{table}]` must be a table")))?;
    for (name, v) in table_value {
        let n = v
            .as_float()
            .or_else(|| v.as_integer().map(|i| i as f64))
            .ok_or_else(|| ThemeError(format!("`{table}.{name}` must be a number")))?;
        if n < 0.0 {
            return Err(ThemeError(format!("`{table}.{name}` must not be negative")));
        }
        out.insert(name.clone(), n);
    }
    Ok(out)
}

/// Parses one `[color.<name>]` entry.
fn raw_color(name: &str, entry: &toml::Value) -> Result<RawColor, ThemeError> {
    let path = format!("color.{name}");
    let table = entry
        .as_table()
        .ok_or_else(|| ThemeError(format!("`[{path}]` must be a table")))?;

    if let Some(reference) = table.get("ref") {
        let reference = reference
            .as_str()
            .ok_or_else(|| ThemeError(format!("`{path}.ref` must be a string")))?
            .to_string();
        let alpha = table
            .get("alpha")
            .map(|a| raw_alpha(&path, a))
            .transpose()?;
        for key in table.keys() {
            if !matches!(key.as_str(), "ref" | "alpha") {
                return Err(ThemeError(format!(
                    "unknown key `{key}` in `[{path}]` (allowed: ref, alpha)"
                )));
            }
        }
        return Ok(RawColor::Shared { reference, alpha });
    }

    let light = table
        .get("light")
        .ok_or_else(|| ThemeError(format!("`[{path}]` is missing `light`")))?;
    let dark = table
        .get("dark")
        .ok_or_else(|| ThemeError(format!("`[{path}]` is missing `dark`")))?;
    for key in table.keys() {
        if !matches!(key.as_str(), "light" | "dark") {
            return Err(ThemeError(format!(
                "unknown key `{key}` in `[{path}]` (allowed: light, dark, or ref + alpha)"
            )));
        }
    }
    Ok(RawColor::Themes {
        light: raw_value(&format!("{path}.light"), light)?,
        dark: raw_value(&format!("{path}.dark"), dark)?,
    })
}

/// Parses a colour value: a hex string or a `{ ref = "…" }` table.
fn raw_value(path: &str, value: &toml::Value) -> Result<RawValue, ThemeError> {
    if let Some(hex) = value.as_str() {
        return Ok(RawValue::Hex(hex.to_string()));
    }
    let table = value.as_table().ok_or_else(|| {
        ThemeError(format!(
            "`{path}` must be a `#RRGGBBAA` string or a `{{ ref = … }}` table"
        ))
    })?;
    let reference = table
        .get("ref")
        .ok_or_else(|| ThemeError(format!("`{path}` is missing `ref`")))?;
    let reference = reference
        .as_str()
        .ok_or_else(|| ThemeError(format!("`{path}.ref` must be a string")))?
        .to_string();
    let alpha = table.get("alpha").map(|a| raw_alpha(path, a)).transpose()?;
    for key in table.keys() {
        if !matches!(key.as_str(), "ref" | "alpha") {
            return Err(ThemeError(format!(
                "unknown key `{key}` in `{path}` (allowed: ref, alpha)"
            )));
        }
    }
    Ok(RawValue::Ref { reference, alpha })
}

/// Parses an alpha: a number shared by both themes, or `{ light = …, dark = … }`.
fn raw_alpha(path: &str, value: &toml::Value) -> Result<RawAlpha, ThemeError> {
    if let Some(a) = value
        .as_float()
        .or_else(|| value.as_integer().map(|i| i as f64))
    {
        return validate_alpha(&format!("{path}.alpha"), a).map(RawAlpha::Both);
    }
    let table = value.as_table().ok_or_else(|| {
        ThemeError(format!(
            "`{path}.alpha` must be a number or `{{ light = …, dark = … }}`"
        ))
    })?;
    let light = table
        .get("light")
        .ok_or_else(|| ThemeError(format!("`{path}.alpha` is missing `light`")))?;
    let dark = table
        .get("dark")
        .ok_or_else(|| ThemeError(format!("`{path}.alpha` is missing `dark`")))?;
    for key in table.keys() {
        if !matches!(key.as_str(), "light" | "dark") {
            return Err(ThemeError(format!("unknown key `{key}` in `{path}.alpha`")));
        }
    }
    let light = light
        .as_float()
        .or_else(|| light.as_integer().map(|i| i as f64))
        .ok_or_else(|| ThemeError(format!("`{path}.alpha.light` must be a number")))?;
    let dark = dark
        .as_float()
        .or_else(|| dark.as_integer().map(|i| i as f64))
        .ok_or_else(|| ThemeError(format!("`{path}.alpha.dark` must be a number")))?;
    Ok(RawAlpha::Themes {
        light: validate_alpha(&format!("{path}.alpha.light"), light)?,
        dark: validate_alpha(&format!("{path}.alpha.dark"), dark)?,
    })
}

fn validate_alpha(path: &str, alpha: f64) -> Result<f64, ThemeError> {
    if !(0.0..=1.0).contains(&alpha) {
        return Err(ThemeError(format!(
            "`{path}` must be within 0..=1, got {alpha}"
        )));
    }
    Ok(alpha)
}

/// Resolves one colour (and, recursively, everything it references), with cycle detection.
fn resolve_color(
    name: &str,
    raw: &BTreeMap<String, RawColor>,
    resolved: &mut BTreeMap<String, ColorPair>,
    stack: &mut BTreeSet<String>,
) -> Result<ColorPair, ThemeError> {
    if let Some(pair) = resolved.get(name) {
        return Ok(*pair);
    }
    if !stack.insert(name.to_string()) {
        return Err(ThemeError(format!(
            "cycle in colour refs: `{name}` references itself"
        )));
    }
    let entry = raw
        .get(name)
        .ok_or_else(|| ThemeError(format!("unknown colour token `{name}`")))?;
    let pair = match entry {
        RawColor::Shared { reference, alpha } => {
            let target =
                resolve_color(reference, raw, resolved, stack).map_err(|e| annotate(e, name))?;
            apply_alpha(target, *alpha)
        }
        RawColor::Themes { light, dark } => ColorPair {
            light: resolve_value(
                &format!("color.{name}.light"),
                light,
                Theme::Light,
                raw,
                resolved,
                stack,
            )?,
            dark: resolve_value(
                &format!("color.{name}.dark"),
                dark,
                Theme::Dark,
                raw,
                resolved,
                stack,
            )?,
        },
    };
    stack.remove(name);
    resolved.insert(name.to_string(), pair);
    Ok(pair)
}

/// Which half of a pair a per-theme value resolves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Theme {
    Light,
    Dark,
}

fn resolve_value(
    path: &str,
    value: &RawValue,
    theme: Theme,
    raw: &BTreeMap<String, RawColor>,
    resolved: &mut BTreeMap<String, ColorPair>,
    stack: &mut BTreeSet<String>,
) -> Result<Rgba, ThemeError> {
    match value {
        RawValue::Hex(hex) => {
            Rgba::from_hex(hex).map_err(|e| ThemeError(format!("{path}: {}", e.0)))
        }
        RawValue::Ref { reference, alpha } => {
            let target = resolve_color(reference, raw, resolved, stack)
                .map_err(|e| annotate_ref(e, path, reference))?;
            // A per-theme ref takes the referenced token's value for the SAME theme
            // (`dark = { ref = "system-accent-dark-1" }` uses that token's dark value).
            let color = match theme {
                Theme::Light => target.light,
                Theme::Dark => target.dark,
            };
            Ok(apply_alpha_single(color, *alpha, theme))
        }
    }
}

/// Replaces the alpha channel (per theme) of a resolved pair.
fn apply_alpha(pair: ColorPair, alpha: Option<RawAlpha>) -> ColorPair {
    match alpha {
        None => pair,
        Some(a) => ColorPair {
            light: Rgba {
                a: scale_alpha(a.light()),
                ..pair.light
            },
            dark: Rgba {
                a: scale_alpha(a.dark()),
                ..pair.dark
            },
        },
    }
}

fn apply_alpha_single(color: Rgba, alpha: Option<RawAlpha>, theme: Theme) -> Rgba {
    match alpha {
        None => color,
        Some(a) => Rgba {
            a: scale_alpha(match theme {
                Theme::Light => a.light(),
                Theme::Dark => a.dark(),
            }),
            ..color
        },
    }
}

fn annotate(err: ThemeError, name: &str) -> ThemeError {
    ThemeError(format!("color.{name}: {}", err.0))
}

fn annotate_ref(err: ThemeError, path: &str, reference: &str) -> ThemeError {
    ThemeError(format!("{path}: ref `{reference}`: {}", err.0))
}
