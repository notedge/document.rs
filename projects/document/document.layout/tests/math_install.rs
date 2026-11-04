//! 字体资产冒烟：装前必须 `not_typeset`，装完必须出真实分数与上标几何。
//!
//! 这是一个独立的测试二进制：`document-text` 的字体注册表是进程级的，放在
//! `tests/main.rs` 里会和「无字体」断言并行竞争。

use document_compose::Severity;
use document_fonts::install_default_math_font;
use document_formats::lower_tex_formula_source;
use document_ir::{Block, DocumentGraph, DocumentId, Inline, MathDisplay, MathFormula};
use document_layout::{
    Diagrams,
    document::layout_with_diagnostics,
    hint::LayoutHint,
    math::{LayoutNode, layout_formula},
    plan::{LayoutItem, LayoutPage},
};
use document_text::math::MathFontFamily;

const NOT_TYPESET: &str = "vutex.layout.math.not_typeset";

fn display_formula() -> MathFormula {
    lower_tex_formula_source(r"\frac{a}{b}+x^2", MathDisplay::Display)
}

fn text_runs(page: &LayoutPage) -> Vec<&str> {
    page.items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::TextRun(run) => Some(run.text.as_str()),
            _ => None,
        })
        .collect()
}

fn path_count(page: &LayoutPage) -> usize {
    page.items.iter().filter(|item| matches!(item, LayoutItem::Path(_))).count()
}

fn fraction_rule(node: &LayoutNode) -> Option<(f64, f64)> {
    match node {
        LayoutNode::Rule { width, height, .. } if width.to_f64() > 0.0 => Some((width.to_f64(), height.to_f64())),
        LayoutNode::HBox(hbox) => hbox.children.iter().find_map(fraction_rule),
        LayoutNode::VBox(vbox) => vbox.children.iter().find_map(fraction_rule),
        _ => None,
    }
}

fn shifted_box(node: &LayoutNode) -> Option<f64> {
    match node {
        LayoutNode::HBox(hbox) if hbox.shift.to_f64() != 0.0 => Some(hbox.shift.to_f64()),
        LayoutNode::HBox(hbox) => hbox.children.iter().find_map(shifted_box),
        LayoutNode::VBox(vbox) => vbox.children.iter().find_map(shifted_box),
        _ => None,
    }
}

#[test]
fn bundled_fonts_turn_source_text_into_typeset_geometry() {
    assert!(
        MathFontFamily::builtin_default().is_none(),
        "this smoke starts without a host font; install one only through `install_default_math_font`"
    );

    let mut graph = DocumentGraph::new(DocumentId(1));
    graph.push_block(Block::Math { formula: display_formula() });
    graph.push_block(Block::Paragraph {
        content: vec![
            Inline::Text { text: "inline ".to_string() },
            Inline::InlineMath { formula: lower_tex_formula_source("x^2", MathDisplay::Inline) },
        ],
    });

    // --- before: no font reaches the process registry ---------------------
    let (plan, diagnostics) = layout_with_diagnostics(&graph, &LayoutHint::a4(), &Diagrams::default());
    let reported: Vec<_> = diagnostics.iter().filter(|diagnostic| diagnostic.code == NOT_TYPESET).collect();
    assert_eq!(reported.len(), 2, "display and inline math both report: {diagnostics:?}");
    for diagnostic in &reported {
        assert_eq!(diagnostic.severity, Severity::Error);
        assert!(diagnostic.span.is_some(), "diagnostic points at the formula source");
        assert!(diagnostic.message.contains("kept as text"));
    }
    let runs = text_runs(&plan.pages[0]);
    assert!(runs.iter().any(|text| *text == r"\frac{a}{b}+x^2"), "display source stays visible: {runs:?}");
    assert_eq!(path_count(&plan.pages[0]), 0, "nothing on the page is typeset math");
    println!("before install: not_typeset={} text_runs={runs:?} paths=0", reported.len());

    // --- install: idempotent ---------------------------------------------
    install_default_math_font();
    install_default_math_font();
    assert_eq!(
        MathFontFamily::builtin_default(),
        Some(MathFontFamily::LmMath),
        "Latin Modern Math wins the preference order"
    );

    // --- after: the same formula is typeset -------------------------------
    let (plan, diagnostics) = layout_with_diagnostics(&graph, &LayoutHint::a4(), &Diagrams::default());
    assert!(
        !diagnostics.iter().any(|diagnostic| diagnostic.code == NOT_TYPESET),
        "an installed font leaves no `not_typeset`: {diagnostics:?}"
    );
    let runs = text_runs(&plan.pages[0]);
    assert!(
        !runs.iter().any(|text| *text == r"\frac{a}{b}+x^2"),
        "the source is replaced by glyph outlines, not kept as text: {runs:?}"
    );
    assert!(path_count(&plan.pages[0]) > 0, "the formula paints vector outlines: {runs:?}");

    let (result, root, math_diagnostics) =
        layout_formula(&display_formula(), 16.0).expect("the bundled default font lays the formula out");
    let (rule_width, rule_height) = fraction_rule(&root).expect("fraction bar");
    let shift = shifted_box(&root).expect("the superscript sits on a shifted box");
    let height = root.height().to_f64();
    assert!(rule_width > 0.0);
    assert!(rule_height > 0.0);
    assert!(shift != 0.0);
    assert!(height > 16.0, "the fraction stacks higher than one em: {height}");
    println!(
        "after install: paths={} fraction_rule=({rule_width:.3},{rule_height:.3}) shift={shift:.3} height={height:.3} math_diagnostics={:?}",
        path_count(&plan.pages[0]),
        math_diagnostics
    );
    println!(
        "metrics width={:.3} height={:.3} depth={:.3}",
        result.metrics.width.to_f64(),
        result.metrics.height.to_f64(),
        result.metrics.depth.to_f64()
    );
}
