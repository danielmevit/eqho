//! The parser accepts the right documents and rejects the right ones.

use damt_theme::{ColorPair, Rgba, ThemeError, Tokens};

const MINIMAL: &str = r##"
[color.bg]
light = "#F6F7F5"
dark = "#17191A"

[color.overlay]
light = "#000000C7"
dark = "#000000D1"

[radius]
card = 8

[space]
2 = 2

[font.size]
body = 14
"##;

fn err(source: &str) -> String {
    match Tokens::parse(source) {
        Ok(_) => panic!("expected a parse error"),
        Err(ThemeError(m)) => m,
    }
}

#[test]
fn accepts_the_task_example() {
    let tokens = Tokens::parse(MINIMAL).unwrap();
    assert_eq!(
        tokens.color("bg"),
        Some(ColorPair {
            light: Rgba::from_hex("#F6F7F5").unwrap(),
            dark: Rgba::from_hex("#17191A").unwrap(),
        })
    );
    assert_eq!(
        tokens.color("overlay"),
        Some(ColorPair {
            light: Rgba::from_hex("#000000C7").unwrap(),
            dark: Rgba::from_hex("#000000D1").unwrap(),
        })
    );
    assert_eq!(tokens.radius("card"), Some(8.0));
    assert_eq!(tokens.space("2"), Some(2.0));
    assert_eq!(tokens.font_size("body"), Some(14.0));
    assert_eq!(tokens.color("missing"), None);
}

#[test]
fn accepts_lowercase_hex() {
    let tokens = Tokens::parse("[color.a]\nlight = \"#f6f7f5\"\ndark = \"#000000c7\"\n").unwrap();
    assert_eq!(tokens.color("a").unwrap().light.r, 0xF6);
    assert_eq!(tokens.color("a").unwrap().dark.a, 0xC7);
}

#[test]
fn rejects_invalid_hex() {
    let msg = err("[color.a]\nlight = \"F6F7F5\"\ndark = \"#17191A\"\n");
    assert!(msg.contains("must start with"), "{msg}");
    let msg = err("[color.a]\nlight = \"#F6F7\"\ndark = \"#17191A\"\n");
    assert!(msg.contains("#RRGGBB"), "{msg}");
    let msg = err("[color.a]\nlight = \"#GG0000\"\ndark = \"#17191A\"\n");
    assert!(msg.contains("hex"), "{msg}");
}

#[test]
fn rejects_missing_light_or_dark() {
    let msg = err("[color.a]\ndark = \"#17191A\"\n");
    assert!(msg.contains("missing `light`"), "{msg}");
    let msg = err("[color.a]\nlight = \"#F6F7F5\"\n");
    assert!(msg.contains("missing `dark`"), "{msg}");
}

#[test]
fn rejects_unknown_ref() {
    let msg = err("[color.a]\nref = \"nope\"\n");
    assert!(msg.contains("unknown colour token `nope`"), "{msg}");
}

#[test]
fn rejects_cycles() {
    let msg = err("[color.a]\nref = \"b\"\n\n[color.b]\nref = \"a\"\n");
    assert!(msg.contains("cycle"), "{msg}");
    let msg = err("[color.a]\nlight = { ref = \"a\" }\ndark = \"#000000\"\n");
    assert!(msg.contains("cycle"), "{msg}");
}

#[test]
fn rejects_alpha_out_of_range() {
    let msg = err(
        "[color.a]\nlight = \"#000000\"\ndark = \"#000000\"\n\n[color.b]\nref = \"a\"\nalpha = 1.5\n",
    );
    assert!(msg.contains("0..=1"), "{msg}");
    let msg = err(
        "[color.a]\nlight = \"#000000\"\ndark = \"#000000\"\n\n[color.b]\nref = \"a\"\nalpha = { light = 0.5, dark = -0.1 }\n",
    );
    assert!(msg.contains("0..=1"), "{msg}");
}

#[test]
fn rejects_unknown_tables_and_keys() {
    let msg = err("[colour.a]\nlight = \"#000000\"\ndark = \"#000000\"\n");
    assert!(msg.contains("unknown table `[colour]`"), "{msg}");
    let msg = err("[color.a]\nlight = \"#000000\"\ndark = \"#000000\"\nlght = \"#000000\"\n");
    assert!(msg.contains("unknown key `lght`"), "{msg}");
    let msg = err("[color.a]\nref = \"a\"\napha = 0.5\n");
    assert!(msg.contains("unknown key `apha`"), "{msg}");
    let msg = err("[font]\nsize = { body = 14 }\nweight = 400\n");
    assert!(msg.contains("unknown key `weight`"), "{msg}");
}

#[test]
fn rejects_non_numbers_in_number_tables() {
    let msg = err("[radius]\ncard = \"8\"\n");
    assert!(msg.contains("must be a number"), "{msg}");
    let msg = err("[space]\n2 = -4\n");
    assert!(msg.contains("not be negative"), "{msg}");
}

#[test]
fn rejects_invalid_toml() {
    let msg = err("[color.a\n");
    assert!(msg.contains("invalid TOML"), "{msg}");
}
