//! Reflow HTML formula typesetting.

use std::path::{Path, PathBuf};

use document_compose::math::FontStyle;
use document_fonts::{MathFontPack, install_math_fonts};
use document_ir::{MathAtom, MathClass, MathDisplay, MathExpression, MathFormula, MathFractionLine};
use document_layout::math::layout_formula_for;
use document_presentation::flow::{FlowBlock, FlowPresentation, FlowRun};
use document_renderer_html::{FlowMathContext, render_html_reflow, render_html_reflow_with_math};
use document_renderer_svg::math::render_formula_result;
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
#[cfg_attr(not(math_font_pack), ignore = "needs a math font pack: set `VUTEX_FONT_DIR` to `VuTeX/projects/vutex.rs/fonts`")]
fn inline_formulas_typeset_into_baseline_aligned_svg() {
    install_host_math_fonts();
    let flow = presentation(vec![FlowBlock::Paragraph {
        runs: vec![text_run("Area "), math_run(fraction(MathDisplay::Inline)), text_run(" units")],
    }]);
    let html = render_html_reflow_with_math(&flow, &math_context()).expect("reflow");
    assert!(html.markup.contains("<span class=\"vutex-math\""), "{}", html.markup);
    assert!(html.markup.contains("vertical-align:-"), "{}", html.markup);
    assert!(html.markup.contains("<svg"), "{}", html.markup);
    assert!(html.markup.contains("<svg class=\"vutex-math-defs\""), "{}", html.markup);
    assert!(html.markup.contains("<use href=\"#vutex-g"), "{}", html.markup);
    // The authored source only survives as the image label, never as body
    // text or a code run.
    assert!(!html.markup.contains("<code>\\frac{a}{b}"), "{}", html.markup);
    assert!(html.diagnostics.is_empty(), "{:?}", html.diagnostics);
}

#[test]
#[cfg_attr(not(math_font_pack), ignore = "needs a math font pack: set `VUTEX_FONT_DIR` to `VuTeX/projects/vutex.rs/fonts`")]
fn display_formulas_replace_the_code_block() {
    install_host_math_fonts();
    let flow = presentation(vec![FlowBlock::Math { formula: fraction(MathDisplay::Display) }]);
    let html = render_html_reflow_with_math(&flow, &math_context()).expect("reflow");
    assert!(html.markup.contains("<div class=\"vutex-math\" data-vutex-block=\"math\""), "{}", html.markup);
    assert!(!html.markup.contains("<pre data-vutex-block=\"code\">"), "{}", html.markup);
    assert!(html.markup.contains("<svg"), "{}", html.markup);
    assert!(html.markup.contains("<use href=\"#vutex-g"), "{}", html.markup);
    assert!(html.diagnostics.is_empty(), "{:?}", html.diagnostics);
}

#[test]
#[cfg_attr(not(math_font_pack), ignore = "needs a math font pack: set `VUTEX_FONT_DIR` to `VuTeX/projects/vutex.rs/fonts`")]
fn repeated_formulas_share_each_outline_once() {
    install_host_math_fonts();
    let flow = presentation(vec![
        FlowBlock::Math { formula: fraction(MathDisplay::Inline) },
        FlowBlock::Math { formula: fraction(MathDisplay::Inline) },
    ]);
    let html = render_html_reflow_with_math(&flow, &math_context()).expect("reflow");
    let outlines = html.markup.matches("<path id=\"vutex-g").count();
    let references = html.markup.matches("<use href=\"#vutex-g").count();
    // Two copies of one formula reference the same outlines instead of
    // painting a second copy of each glyph.
    assert!(references > outlines, "outlines {outlines} references {references}");
    assert_eq!(html.markup.matches("<svg class=\"vutex-math-defs\"").count(), 1, "{}", html.markup);
}

/// Ids declared by the hidden definitions container, in document order.
fn defined_outline_ids(html: &str) -> Vec<String> {
    let start = html.find("<svg class=\"vutex-math-defs\"").expect("defs container");
    let end = start + html[start..].find("</svg>").expect("defs container end");
    html[start..end]
        .split("<path id=\"")
        .skip(1)
        .map(|chunk| chunk[..chunk.find('"').expect("id terminator")].to_string())
        .collect()
}

/// Ids referenced by `<use>` anywhere in the document, in document order.
fn referenced_outline_ids(html: &str) -> Vec<String> {
    html.split("<use href=\"#").skip(1).map(|chunk| chunk[..chunk.find('"').expect("href terminator")].to_string()).collect()
}

/// Sharing is only safe while every reference resolves: a dangling `<use>`
/// paints nothing, and a duplicate id silently rebinds definitions.  Both are
/// invisible in a passing pixel diff, so they are asserted here.
#[test]
#[cfg_attr(not(math_font_pack), ignore = "needs a math font pack: set `VUTEX_FONT_DIR` to `VuTeX/projects/vutex.rs/fonts`")]
fn every_shared_reference_resolves_to_one_definition() {
    install_host_math_fonts();
    let flow = presentation(vec![
        FlowBlock::Paragraph {
            runs: vec![math_run(fraction(MathDisplay::Inline)), text_run(" and "), math_run(fraction(MathDisplay::Inline))],
        },
        FlowBlock::Math { formula: fraction(MathDisplay::Display) },
    ]);
    let html = render_html_reflow_with_math(&flow, &math_context()).expect("reflow");
    let defined = defined_outline_ids(&html.markup);
    let referenced = referenced_outline_ids(&html.markup);
    assert!(!defined.is_empty(), "{}", html.markup);
    assert!(referenced.len() > defined.len(), "references {:?} definitions {:?}", referenced, defined);
    let mut unique = defined.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), defined.len(), "outline ids must be unique: {defined:?}");
    for id in &referenced {
        assert!(defined.contains(id), "dangling reference `#{id}`, defined {defined:?}");
    }
    // The definitions must precede the content that uses them.
    let defs_at = html.markup.find("<svg class=\"vutex-math-defs\"").expect("defs");
    let flow_at = html.markup.find("class=\"vutex-flow\"").expect("flow article");
    assert!(defs_at < flow_at, "definitions must be resolvable before the body: {defs_at} vs {flow_at}");
    assert_eq!(html.markup.matches("<svg class=\"vutex-math-defs\"").count(), 1, "{}", html.markup);
}

/// The wrapper's width, height and baseline offset must be the layout's own
/// numbers: sharing changes markup only, so HTML text keeps sitting on the
/// formula axis instead of drifting by the descender.
#[test]
#[cfg_attr(not(math_font_pack), ignore = "needs a math font pack: set `VUTEX_FONT_DIR` to `VuTeX/projects/vutex.rs/fonts`")]
fn inline_formula_geometry_matches_the_layout_metrics() {
    install_host_math_fonts();
    let formula = fraction(MathDisplay::Inline);
    let (result, _root, diagnostics) = layout_formula_for(&formula, 11.0, MathFontFamily::LmMath).expect("layout");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let standalone = render_formula_result(&result);

    let flow = presentation(vec![FlowBlock::Paragraph { runs: vec![math_run(formula)] }]);
    let html = render_html_reflow_with_math(&flow, &math_context()).expect("reflow");

    let style = format!(
        "width:{:.3}pt;height:{:.3}pt;vertical-align:-{:.3}pt",
        standalone.width,
        standalone.height,
        standalone.height - standalone.baseline,
    );
    assert!(html.markup.contains(&style), "expected `{style}` in {}", html.markup);
    // The embedded viewport is the standalone one, so a preview and the page
    // scale the same outlines into the same box.
    let viewport = &standalone.markup["<svg ".len()..];
    let viewport = &viewport[..viewport.find('>').expect("root tag")];
    assert!(html.markup.contains(viewport), "embedded viewport differs from `{viewport}`");
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
    assert!(!html.contains("<use"), "the fallback must not reference outlines it never defined: {html}");
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
