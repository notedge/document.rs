//! Smoke coverage for the TeX profile: one minimal document reaches SVG
//! through the shared layout, presentation, and renderer chain.

use document_compose::Severity;
use document_input_tex::import_source;
use document_layout::{Diagrams, LayoutHint, document::layout_with_diagnostics};
use document_presentation::build_display_presentation;
use document_renderer_svg::render_svg;

#[test]
fn minimal_source_reaches_the_rendered_svg() {
    let source = "\\begin{document}\n\\section{TeX to SVG}\nHello $x^2$.\n\\end{document}\n";
    let document = import_source("virtual://tex-smoke.tex", source).expect("minimal TeX must import");
    assert!(document.report.unsupported.is_empty(), "{:?}", document.report.unsupported);

    let (plan, layout_diagnostics) = layout_with_diagnostics(&document.graph, &LayoutHint::a4(), &Diagrams::new());
    assert!(layout_diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error), "{:?}", layout_diagnostics);

    let display = build_display_presentation(&plan);
    let svg = render_svg(&display).expect("SVG rendering must succeed");
    assert!(svg.starts_with("<svg"), "{}", &svg[..svg.len().min(80)]);
    assert!(svg.contains("TeX to SVG"), "heading text must reach the svg");
    assert!(svg.contains("Hello"), "paragraph text must reach the svg");
}
