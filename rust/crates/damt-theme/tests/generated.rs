//! The committed generated outputs must be exactly what the committed tokens.toml produces
//! (the in-process twin of `--example gen -- --check`).

use damt_theme::Tokens;

const TOKENS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/eqho/theme/tokens.toml"
);
const GENERATED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/eqho/theme/generated"
);

#[test]
fn committed_outputs_are_fresh() {
    let source = std::fs::read_to_string(TOKENS).unwrap();
    let tokens = Tokens::parse(&source).unwrap();

    for (name, generated) in [
        ("theme.slint", tokens.to_slint()),
        ("tokens.css", tokens.to_css()),
        ("tokens.rs", tokens.to_rust()),
    ] {
        let path = format!("{GENERATED}/{name}");
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {path} ({e}); regenerate with: cargo run -p damt-theme --example gen -- apps/ocy/theme/tokens.toml apps/ocy/theme/generated"));
        assert_eq!(
            committed, generated,
            "{path} is stale; regenerate with: cargo run -p damt-theme --example gen -- apps/ocy/theme/tokens.toml apps/ocy/theme/generated"
        );
    }
}

#[test]
fn css_contains_both_dark_blocks() {
    let source = std::fs::read_to_string(TOKENS).unwrap();
    let css = Tokens::parse(&source).unwrap().to_css();
    assert!(css.contains(":root {\n"));
    assert!(css.contains(
        "@media (prefers-color-scheme: dark) {\n  :root:not([data-theme=\"light\"]) {\n"
    ));
    assert!(css.contains(":root[data-theme=\"dark\"] {\n"));
    assert!(css.contains("--color-bg: #F5F6F8FF;"));
    // The dark blocks carry the dark values; the light block must not.
    let light_block = css.split("@media").next().unwrap();
    assert!(!light_block.contains("#0A0A0BFF"));
    assert!(css.contains("--color-bg: #0A0A0BFF;"));
}
