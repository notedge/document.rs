//! Reflow HTML formula typesetting.

use std::path::{Path, PathBuf};

use document_compose::math::FontStyle;
use document_fonts::{MathFontPack, install_math_fonts};
use document_ir::{MathAtom, MathClass, MathDisplay, MathExpression, MathFormula, MathFractionLine};
use document_presentation::flow::{FlowBlock, FlowPresentation, FlowRun};
use document_renderer_html::{FlowMathContext, render_html_reflow, render_html_reflow_with_math};
use document_text::math::MathFontFamily;

/// Host font pack directory, overridable through `VUTEX_FONT_DIR`.
fn host_font_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VUTEX_FONT_DIR") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("workspace root")
        .join("VuTeX")
        .join("projects")
        .join("vutex.rs")
        .join("fonts")
}

fn install_host_math_fonts() {
    let dir = host_font_dir();
    assert!(dir.is_dir(), "typesetting needs the host font pack at {}; set VUTEX_FONT_DIR", dir.display());
    install_math_fonts(&MathFontPack::load_from(&dir).expect("math font pack"));
}

fn atom(text: &str) -> MathExpression {
    MathExpression::Atom(MathAtom { text: text.to_string(), class: MathClass::Identifier, span: None })
}

fn fraction(display: MathDisplay) -> MathFormula {
    MathFormula {
        source: "\\frac{a}{b}".to_string(),
        language: Some("latex".to_string()),
        display,
        expression: MathExpression::Fraction {
            numerator: Box::new(atom("a")),
            denominator: Box::new(atom("b")),
            line: MathFractionLine::Solid,
        },
    }
}

fn text_run(text: &str) -> FlowRun {
    FlowRun { text: text.to_string(), style: FontStyle::Normal, href: None, title: None, math: None }
}

fn math_run(formula: MathFormula) -> FlowRun {
    FlowRun { text: String::new(), style: FontStyle::Normal, href: None, title: None, math: Some(formula) }
}

fn presentation(blocks: Vec<FlowBlock>) -> FlowPresentation {
    FlowPresentation { root_tag: "article".to_string(), blocks }
}

fn math_context() -> FlowMathContext {
    FlowMathContext { family: MathFontFamily::LmMath, base_size: 11.0 }
}

#[test]
fn inline_formulas_typeset_into_baseline_aligned_svg() {
    install_host_math_fonts();
    let flow = presentation(vec![FlowBlock::Paragraph {
        runs: vec![text_run("Area "), math_run(fraction(MathDisplay::Inline)), text_run(" units")],
    }]);
    let html = render_html_reflow_with_math(&flow, &math_context()).expect("reflow");
    assert!(html.markup.contains("<span class=\"vutex-math\""), "{}", html.markup);
    assert!(html.markup.contains("vertical-align:-"), "{}", html.markup);
    assert!(html.markup.contains("<svg"), "{}", html.markup);
    // The authored source only survives as the image label, never as body
    // text or a code run.
    assert!(!html.markup.contains("<code>\\frac{a}{b}"), "{}", html.markup);
    assert!(html.diagnostics.is_empty(), "{:?}", html.diagnostics);
}

#[test]
fn display_formulas_replace_the_code_block() {
    install_host_math_fonts();
    let flow = presentation(vec![FlowBlock::Math { formula: fraction(MathDisplay::Display) }]);
    let html = render_html_reflow_with_math(&flow, &math_context()).expect("reflow");
    assert!(html.markup.contains("<div class=\"vutex-math\" data-vutex-block=\"math\""), "{}", html.markup);
    assert!(!html.markup.contains("<pre data-vutex-block=\"code\">"), "{}", html.markup);
    assert!(html.markup.contains("<svg"), "{}", html.markup);
    assert!(html.diagnostics.is_empty(), "{:?}", html.diagnostics);
}

#[test]
fn formulas_without_math_context_keep_their_source() {
    let flow = presentation(vec![
        FlowBlock::Math { formula: fraction(MathDisplay::Display) },
        FlowBlock::Paragraph { runs: vec![math_run(fraction(MathDisplay::Inline))] },
    ]);
    let html = render_html_reflow(&flow).expect("reflow");
    assert!(html.contains("<pre data-vutex-block=\"code\"><code>\\frac{a}{b}</code></pre>"), "{html}");
    assert!(html.contains("\\frac{a}{b}"), "{html}");
    assert!(!html.contains("<svg"), "{html}");
}

#[test]
fn a_formula_with_no_typesetting_language_keeps_source_and_warns() {
    let formula = MathFormula::unsupported("x^2", Some("typst".to_string()), MathDisplay::Display, "typst");
    let flow = presentation(vec![FlowBlock::Math { formula }]);
    let html = render_html_reflow_with_math(&flow, &math_context()).expect("reflow");
    assert!(html.markup.contains("<pre data-vutex-block=\"code\"><code>x^2</code></pre>"), "{}", html.markup);
    assert!(
        html.diagnostics.iter().any(|diagnostic| diagnostic.code == "vutex.render.html.math.not-typeset"),
        "{:?}",
        html.diagnostics
    );
}
