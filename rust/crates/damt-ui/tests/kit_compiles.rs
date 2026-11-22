//! The kit must compile against a real generated theme, export every component the brief lists,
//! and contain no italics.
use std::collections::HashMap;
use std::path::PathBuf;

fn theme_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/eqho/theme/generated")
}

fn compile(source: &str) -> slint_interpreter::CompilationResult {
    let mut compiler = slint_interpreter::Compiler::default();
    let mut paths = HashMap::new();
    paths.insert(damt_ui::LIBRARY_NAME.to_owned(), damt_ui::ui_dir());
    paths.insert(damt_ui::THEME_LIBRARY_NAME.to_owned(), theme_dir());
    compiler.set_library_paths(paths);
    spin_on::spin_on(
        compiler.build_from_source(source.to_owned(), damt_ui::ui_dir().join("probe.slint")),
    )
}

#[test]
fn kit_compiles_with_every_component() {
    let source = r#"
import { Button, ButtonKind, CloseButton, LinkButton, Card, Toggle, ProgressLine, Chevron, Check, Caption, Slider, TabList, PresetCard, AccordionHeader, Dropdown, Timeline, TimelineMark, TimelineMarkKind, Swatch, CompactRow } from "@damt-ui/kit.slint";
export component Probe inherits Window {
    VerticalLayout {
        Button { text: "a"; kind: ButtonKind.accent; }
        Button { text: "b"; kind: ButtonKind.outline; enabled: false; }
        Button { text: "c"; kind: ButtonKind.text; }
        CloseButton { accessible-name: "Close"; }
        LinkButton { text: "Try"; }
        LinkButton { text: "Try disabled"; enabled: false; padding-x: 4px; }
        Card { height: 20px; }
        Toggle { checked: true; }
        ProgressLine { progress: 0.5; }
        Chevron { up: true; }
        Check {}
        Caption { text: "c"; }
        Slider { label: "S"; value-text: "5 min"; value: 5; minimum: 1; maximum: 30; step: 1; }
        TabList { items: ["a", "b"]; selected: 1; accessible-name: "Tabs"; }
        PresetCard { title: "P"; summary: "s"; selected: true; }
        AccordionHeader { text: "A"; open: true; }
        Dropdown { model: ["x", "y"]; current-index: 0; accessible-name: "D"; }
        Timeline { marks: [{ kind: TimelineMarkKind.short-break, position: 0.2 }, { kind: TimelineMarkKind.long-break, position: 0.5 }]; labels: ["1", "2"]; now-label: "Now"; }
        Swatch { block: true; }
        CompactRow { label: "R"; Toggle {} }
    }
}
"#;
    let result = compile(source);
    let errors: Vec<String> = result
        .diagnostics()
        .filter(|d| d.level() == slint_interpreter::DiagnosticLevel::Error)
        .map(|d| d.to_string())
        .collect();
    assert!(errors.is_empty(), "kit failed to compile: {errors:#?}");
    assert!(result.component("Probe").is_some());
}

#[test]
fn unknown_component_is_rejected() {
    // Guards the test above: a missing export must fail compilation.
    let result = compile(
        r#"import { NoSuchWidget } from "@damt-ui/kit.slint"; export component P inherits Window { NoSuchWidget {} }"#,
    );
    assert!(result.has_errors());
}

#[test]
fn kit_has_no_italics() {
    let text = std::fs::read_to_string(damt_ui::kit_path()).unwrap_or_default();
    assert!(!text.is_empty());
    assert!(!text.contains("font-italic: true"));
}
