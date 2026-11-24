//! Compiles Eqho's `.slint` UI with `@damt-ui` and `@app-theme`.
use std::collections::HashMap;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let theme_dir = manifest.join("../theme/generated");
    let mut paths = HashMap::new();
    paths.insert(damt_ui::LIBRARY_NAME.to_owned(), damt_ui::ui_dir());
    paths.insert(damt_ui::THEME_LIBRARY_NAME.to_owned(), theme_dir.clone());
    let config = slint_build::CompilerConfiguration::new()
        .with_library_paths(paths)
        .with_style("fluent".into())
        .with_debug_info(std::env::var("PROFILE").as_deref() == Ok("debug"));
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed={}", damt_ui::ui_dir().display());
    println!("cargo:rerun-if-changed={}", theme_dir.display());
    if let Err(error) = slint_build::compile_with_config("ui/eqho.slint", config) {
        println!("cargo:warning=slint compile failed: {error}");
        std::process::exit(1);
    }
}
