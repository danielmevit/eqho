//! Ref and alpha resolution: the "@ x %" design rules resolve to the exact bytes the C# app uses.

use damt_theme::{Rgba, Tokens};

const SOURCE: &str = r##"
[color.text]
light = "#1B1E1F"
dark = "#ECEEEC"

[color.text-muted]
light = "#5A6064"
dark = "#A4ABAF"

[color.accent]
light = "#2F7D62"
dark = "#5FBF97"

[color.attention]
light = "#B04E2C"
dark = "#E08A64"

[color.divider]
ref = "text"
alpha = 0.08

[color.border]
ref = "text-muted"
alpha = { light = 0.40, dark = 0.30 }

[color.edge-frame]
ref = "accent"
alpha = 0.55

[color.attention-frame]
ref = "attention"
alpha = 0.55

[color.button-accent-hover]
light = { ref = "accent" }
dark = { ref = "text-muted" }

[color.copy]
light = { ref = "accent" }
dark = { ref = "text-muted", alpha = 0.5 }
"##;

#[test]
fn divider_is_text_at_eight_percent() {
    // DESIGN_SYSTEM: Divider = Text @ 8 %. The C# DividerColor is #141B1E1F in ARGB order,
    // which is #1B1E1F14 in RGBA order — the exact bytes this resolves to.
    let tokens = Tokens::parse(SOURCE).unwrap();
    let divider = tokens.color("divider").unwrap();
    assert_eq!(divider.light.to_hex(), "#1B1E1F14");
    assert_eq!(divider.dark.to_hex(), "#ECEEEC14");
    assert_eq!(divider.light.to_argb_hex(), "#141B1E1F");
}

#[test]
fn per_theme_alpha_resolves() {
    let tokens = Tokens::parse(SOURCE).unwrap();
    let border = tokens.color("border").unwrap();
    // Light: #5A6064 @ 40 % = #5A606466 (C# #665A6064). Dark: #A4ABAF @ 30 % = #A4ABAF4D (C# #4DA4ABAF).
    assert_eq!(border.light.to_hex(), "#5A606466");
    assert_eq!(border.dark.to_hex(), "#A4ABAF4D");
    assert_eq!(border.light.to_argb_hex(), "#665A6064");
    assert_eq!(border.dark.to_argb_hex(), "#4DA4ABAF");
}

#[test]
fn frame_alphas_resolve_to_140() {
    let tokens = Tokens::parse(SOURCE).unwrap();
    // 55 % of 255 = 140.25 -> 140 (0x8C), the brush alpha the C# app sets on the frame brushes.
    assert_eq!(
        tokens.color("edge-frame").unwrap().light.to_hex(),
        "#2F7D628C"
    );
    assert_eq!(
        tokens.color("edge-frame").unwrap().dark.to_hex(),
        "#5FBF978C"
    );
    assert_eq!(
        tokens.color("attention-frame").unwrap().light.to_hex(),
        "#B04E2C8C"
    );
    assert_eq!(
        tokens.color("attention-frame").unwrap().dark.to_hex(),
        "#E08A648C"
    );
}

#[test]
fn refs_without_alpha_copy_the_target() {
    let tokens = Tokens::parse(SOURCE).unwrap();
    let hover = tokens.color("button-accent-hover").unwrap();
    assert_eq!(hover.light, tokens.color("accent").unwrap().light);
    assert_eq!(hover.dark, tokens.color("text-muted").unwrap().dark);
}

#[test]
fn per_theme_value_refs_and_alpha() {
    let tokens = Tokens::parse(SOURCE).unwrap();
    let copy = tokens.color("copy").unwrap();
    assert_eq!(copy.light, Rgba::from_hex("#2F7D62").unwrap());
    // The dark value re-tints the referenced token's alpha: #A4ABAF @ 50 % = #A4ABAF80.
    assert_eq!(copy.dark.to_hex(), "#A4ABAF80");
}

#[test]
fn refs_may_be_defined_in_any_order() {
    let tokens = Tokens::parse(
        "[color.a]\nref = \"b\"\n\n[color.b]\nlight = \"#000000\"\ndark = \"#FFFFFF\"\n",
    )
    .unwrap();
    assert_eq!(tokens.color("a").unwrap().dark.to_hex(), "#FFFFFFFF");
}
