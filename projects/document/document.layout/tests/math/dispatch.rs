//! 合同 28：公式引擎按 `MathFormula.language` 标签派发，不嗅探 source 内容。

use document_ir::{Block, DocumentGraph, DocumentId, Inline, MathAtom, MathClass, MathDisplay, MathExpression, MathFormula};
use document_layout::{
    BodyMetrics, Diagrams, LayoutHint,
    document::layout_with_diagnostics,
    math::{has_math_lowering, is_tex_formula, is_typst_formula, lower_formula_tree},
};

fn formula(language: Option<&str>, expression: MathExpression) -> MathFormula {
    MathFormula { source: "x + y".into(), language: language.map(str::to_string), display: MathDisplay::Inline, expression }
}

fn identifier(text: &str) -> MathExpression {
    MathExpression::Atom(MathAtom { text: text.to_string(), class: MathClass::Identifier, span: None })
}

#[test]
fn tag_dispatch_selects_the_engine_without_sniffing_source() {
    let expression = MathExpression::Sequence(vec![identifier("x"), identifier("y")]);
    let typst = formula(Some("typst"), expression.clone());
    assert!(is_typst_formula(&typst), "the `typst` tag names the Typst profile");
    assert!(has_math_lowering(&typst), "the Typst profile feeds the shared engine");
    assert!(!is_tex_formula(&typst), "the Typst profile must not fall back to the TeX gate");
    assert!(has_math_lowering(&formula(Some("latex"), expression.clone())));
    assert!(has_math_lowering(&formula(Some("tex"), expression.clone())));
    assert!(has_math_lowering(&formula(None, expression.clone())));
    assert!(has_math_lowering(&formula(Some(""), expression.clone())));
    assert!(!has_math_lowering(&formula(Some("asciimath"), expression)));
}

#[test]
fn typst_expression_reaches_the_formula_tree_and_unknown_tags_stay_refused() {
    let expression = MathExpression::Sequence(vec![identifier("x")]);
    let (_tree, diagnostics) = lower_formula_tree(&formula(Some("typst"), expression.clone()))
        .expect("a typst formula has a lowering into the shared tree");
    assert!(diagnostics.is_empty(), "a plain identifier body adds no diagnostics: {diagnostics:?}");
    assert!(lower_formula_tree(&formula(Some("asciimath"), expression)).is_err(), "an unlowered tag stays refused");
}

/// The dispatch is tag-only: a `typst` formula never reports
/// `vutex.layout.math.unsupported_language`, whether it arrives as a display
/// block or as inline math, while an unknown tag keeps the existing behavior.
#[test]
fn typst_math_never_reports_the_unsupported_language_gap() {
    let expression = MathExpression::Sequence(vec![identifier("x")]);
    let mut graph = DocumentGraph::new(DocumentId(7));
    graph.push_block(Block::Math { formula: formula(Some("typst"), expression.clone()) });
    graph.push_block(Block::Paragraph {
        content: vec![Inline::InlineMath { formula: formula(Some("typst"), expression.clone()) }],
    });
    graph.push_block(Block::Math { formula: formula(Some("asciimath"), expression) });

    let (_plan, diagnostics) = layout_with_diagnostics(&graph, &LayoutHint::a4(), &Diagrams::new(), &BodyMetrics::default());
    let unsupported: Vec<_> =
        diagnostics.iter().filter(|diagnostic| diagnostic.code == "vutex.layout.math.unsupported_language").collect();
    assert_eq!(unsupported.len(), 1, "only the unknown tag keeps the language gap: {diagnostics:?}");
    assert!(unsupported[0].message.contains("asciimath"), "the gap names the unlowered tag: {:?}", unsupported[0]);
}
