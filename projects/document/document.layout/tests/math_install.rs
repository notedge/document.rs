//! 字体资产冒烟：装前必须 `not_typeset`，装完必须出真实分数与上标几何。
//!
//! 这是一个独立的测试二进制：`document-text` 的字体注册表是进程级的，放在
//! `tests/main.rs` 里会和「无字体」断言并行竞争。

use document_compose::Severity;
use document_compose::math::MathConstant;
use document_fonts::{LATIN_MODERN_MATH, install_default_math_font};
use document_formats::lower_tex_formula_source;
use document_ir::{Block, DocumentGraph, DocumentId, Inline, MathDisplay, MathFormula};
use document_layout::{
    Diagrams,
    document::layout_with_diagnostics,
    hint::LayoutHint,
    math::{LayoutNode, layout_formula},
    plan::{LayoutItem, LayoutPage},
};
use document_text::math::{MathFontFacts, MathFontFamily};

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

fn collect_glyph_sizes(node: &LayoutNode, sizes: &mut Vec<f64>) {
    match node {
        LayoutNode::Glyph(glyph) => sizes.push(glyph.size.to_f64()),
        LayoutNode::HBox(hbox) => hbox.children.iter().for_each(|child| collect_glyph_sizes(child, sizes)),
        LayoutNode::VBox(vbox) => vbox.children.iter().for_each(|child| collect_glyph_sizes(child, sizes)),
        _ => {}
    }
}

fn fraction_stack(node: &LayoutNode) -> Option<(&document_layout::math::HBox, &document_layout::math::VBox)> {
    match node {
        LayoutNode::HBox(hbox) if hbox.shift.to_f64() != 0.0 => match hbox.children.first() {
            Some(LayoutNode::VBox(vbox)) => Some((hbox, vbox)),
            _ => None,
        },
        LayoutNode::HBox(hbox) => hbox.children.iter().find_map(fraction_stack),
        LayoutNode::VBox(vbox) => vbox.children.iter().find_map(fraction_stack),
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

    // --- fraction: script-scaled digits on the font's own shifts -----------
    //
    // `\frac{1}{2}` inline demotes both arguments from Text to Script style, so
    // the digits must come out at `ScriptPercentScaleDown` of the base em and
    // the numerator/denominator baselines must sit where the font's
    // `FractionNumeratorShiftUp` / `FractionDenominatorShiftDown` put them.
    // KaTeX reports 0.700 and MathJax 0.707 for the same formula, which is the
    // script scale of the faces they ship, so 1.0 here would be the defect.
    let facts = MathFontFacts::from_bytes(LATIN_MODERN_MATH).expect("the bundled Latin Modern Math parses");
    let constant = |name: MathConstant| {
        facts
            .constants()
            .font_value(name)
            .unwrap_or_else(|| panic!("the bundled font publishes {name:?}"))
            .to_f64()
    };
    let base = 14.0;
    let script_ratio = constant(MathConstant::ScriptPercentScaleDown);
    let numerator_shift = constant(MathConstant::FractionNumeratorShiftUp);
    let denominator_shift = constant(MathConstant::FractionDenominatorShiftDown);

    let fraction = lower_tex_formula_source(r"\frac{1}{2}", MathDisplay::Inline);
    let (fraction_result, fraction_root, _) =
        layout_formula(&fraction, base).expect("the bundled default font lays a fraction out");
    let mut sizes = Vec::new();
    collect_glyph_sizes(&fraction_root, &mut sizes);
    assert_eq!(sizes.len(), 2, "one glyph per fraction argument");
    let script_size = base * script_ratio;
    for size in &sizes {
        assert!(
            (size - script_size).abs() < 1e-3,
            "numerator and denominator are script-sized: {size} != {script_size} ({script_ratio} em of {base})"
        );
    }

    let (wrapper, stack) = fraction_stack(&fraction_root).expect("the fraction wraps its stack in a shifted box");
    let denominator_offset = wrapper.shift.to_f64();
    assert!(
        (denominator_offset - base * denominator_shift).abs() < 1e-3,
        "denominator baseline sits `FractionDenominatorShiftDown` below the fraction baseline: {denominator_offset}"
    );
    let numerator_offset = stack.height.to_f64() - stack.children[0].height().to_f64() - denominator_offset;
    assert!(
        (numerator_offset - base * numerator_shift).abs() < 1e-3,
        "numerator baseline sits `FractionNumeratorShiftUp` above the fraction baseline: {numerator_offset}"
    );
    assert!(
        (fraction_result.metrics.depth.to_f64() - denominator_offset).abs() < 1e-3,
        "a `1`/`2` denominator has no descender, so the fraction depth is its baseline offset"
    );
    println!(
        "fraction: glyph_size={script_size:.3}px ({script_ratio:.3}em) numerator_offset={numerator_offset:.3} denominator_offset={denominator_offset:.3} height={:.3} depth={:.3}",
        fraction_result.metrics.height.to_f64(),
        fraction_result.metrics.depth.to_f64()
    );
}
