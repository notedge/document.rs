//! Entry smoke for the TeX profile: one minimal document reaches SVG.

use document_compose::Severity;
use document_input_tex::render_svg;
use document_text::math::{MathFontFamily, install_math_font};

/// The MATH font payload is a test asset, so the suite never depends on
/// `E:\文档研究\VuTeX` existing on the machine.
fn install_test_math_font() {
    install_math_font(MathFontFamily::LmMath, include_bytes!("../../../test-assets/fonts/LatinModernMath-Regular.otf"));
}

#[test]
fn minimal_source_reaches_the_rendered_svg() {
    install_test_math_font();
    let source = "\\begin{document}\n\\section{TeX to SVG}\nHello $x^2$.\n\\end{document}\n";
    let rendered = render_svg("virtual://tex-smoke.tex", source).expect("minimal TeX must render");

    assert!(rendered.report.unsupported.is_empty(), "{:?}", rendered.report.unsupported);
    assert!(rendered.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error), "{:?}", rendered.diagnostics);
    assert!(rendered.pages >= 1, "{}", rendered.pages);
    assert!(rendered.items >= 1, "{}", rendered.items);
    assert!(rendered.svg.starts_with("<svg"), "{}", &rendered.svg[..rendered.svg.len().min(80)]);
    assert!(rendered.svg.contains("TeX to SVG"), "heading text must reach the svg");
    assert!(rendered.svg.contains("Hello"), "paragraph text must reach the svg");
}
