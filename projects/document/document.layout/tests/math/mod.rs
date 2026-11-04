//! 数学排版冒烟：缺字体必须显式声明，装了字体则要出结构几何。

use document_compose::Severity;
use document_formats::lower_tex_formula_source;
use document_ir::{Block, DocumentGraph, DocumentId, Inline, MathDisplay, MathFormula};
use document_layout::{
    Diagrams,
    document::layout_with_diagnostics,
    hint::LayoutHint,
    math::{FontConformance, LayoutNode, layout_formula_for_with_conformance},
    plan::{LayoutItem, LayoutPage},
};
use document_text::math::MathFontFamily;

const NOT_TYPESET: &str = "vutex.layout.math.not_typeset";

fn formula(source: &str, display: MathDisplay) -> MathFormula {
    lower_tex_formula_source(source, display)
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

/// No host math font: the formula still reaches the page as source text, but
/// the layout must declare the loss instead of reporting a clean pass.
#[test]
fn missing_math_font_is_reported_instead_of_passing_as_clean() {
    assert!(
        MathFontFamily::builtin_default().is_none(),
        "this smoke assumes no host font is installed; install one only in the downstream harness"
    );

    let mut graph = DocumentGraph::new(DocumentId(1));
    graph.push_block(Block::Math { formula: formula(r"\frac{a}{b}+x^2", MathDisplay::Display) });
    graph.push_block(Block::Paragraph {
        content: vec![
            Inline::Text { text: "inline ".to_string() },
            Inline::InlineMath { formula: formula("x^2", MathDisplay::Inline) },
        ],
    });

    let (plan, diagnostics) = layout_with_diagnostics(&graph, &LayoutHint::a4(), &Diagrams::default());
    let reported: Vec<_> = diagnostics.iter().filter(|diagnostic| diagnostic.code == NOT_TYPESET).collect();
    assert_eq!(reported.len(), 2, "display and inline math both report: {diagnostics:?}");
    for diagnostic in &reported {
        assert_eq!(diagnostic.severity, Severity::Error);
        let span = diagnostic.span.expect("the diagnostic points at the formula source");
        assert_eq!(span.start, 0);
        assert!(span.end > 0);
        assert!(diagnostic.message.contains("kept as text"));
    }

    let runs = text_runs(&plan.pages[0]);
    assert!(runs.iter().any(|text| *text == r"\frac{a}{b}+x^2"), "display source stays visible: {runs:?}");
    assert!(runs.iter().any(|text| text.contains("x^2")), "inline source stays visible: {runs:?}");
    assert!(
        !plan.pages[0].items.iter().any(|item| matches!(item, LayoutItem::Path(_))),
        "nothing on the page is typeset math"
    );
}

/// With an explicit family the engine computes real fraction and script
/// geometry. Glyph outlines still need host-installed bytes, which the
/// downstream harness owns; this smoke pins the box geometry that exists
/// without them.
#[test]
fn explicit_family_lays_out_fraction_and_superscript_geometry() {
    let formula = formula(r"\frac{a}{b}+x^2", MathDisplay::Display);
    let (result, root, _diagnostics) =
        layout_formula_for_with_conformance(&formula, 16.0, MathFontFamily::LmMath, FontConformance::AllowFallbacks)
            .expect("an explicit family always lays the formula out");

    let (width, height) = fraction_rule(&root).expect("fraction bar");
    assert!(width > 0.0);
    assert!(height > 0.0, "the fraction rule has a thickness");
    assert!(
        root.height().to_f64() > 16.0 * 1.2,
        "the fraction stacks above the baseline: {}",
        result.metrics.height.to_f64()
    );
    assert!(shifted_box(&root).is_some(), "the superscript sits on a shifted box");
}
