//! The Slint output must compile: `to_slint` is fed to the slint-interpreter (1.19, no renderers,
//! no backends) and the test fails on any error-level diagnostic.

use damt_theme::Tokens;
use slint_interpreter::DiagnosticLevel;

const TOKENS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/eqho/theme/tokens.toml"
);

#[test]
fn slint_output_compiles() {
    let source = std::fs::read_to_string(TOKENS).unwrap();
    let tokens = Tokens::parse(&source).unwrap();
    let slint = tokens.to_slint();

    // Sanity: the global switches on `dark`, carries `scale`, and emits the palette.
    assert!(slint.contains("export global Theme"));
    assert!(slint.contains("in-out property <bool> dark"));
    assert!(slint.contains("in-out property <float> scale: 1.0;"));
    assert!(slint.contains("out property <color> bg: dark ? #121215FF : #F5F6F8FF;"));

    let compiler = slint_interpreter::Compiler::default();
    // The interpreter requires a component in the file; the generated output is a global meant to
    // be imported. The probe also reads the global, so the test checks the properties exist too.
    let source = format!(
        "{slint}\nexport component ThemeProbe {{\n    out property <color> probe-bg: Theme.bg;\n    out property <length> probe-radius: Theme.radius-card;\n}}\n"
    );
    let result = spin_on::spin_on(compiler.build_from_source(source, "theme.slint".into()));
    let errors: Vec<String> = result
        .diagnostics()
        .filter(|d| d.level() == DiagnosticLevel::Error)
        .map(|d| {
            let (line, column) = d.line_column();
            format!("theme.slint:{line}:{column}: {}", d.message())
        })
        .collect();
    assert!(
        errors.is_empty(),
        "theme.slint does not compile:\n{}",
        errors.join("\n")
    );
}
