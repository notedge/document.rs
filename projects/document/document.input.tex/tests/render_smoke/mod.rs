//! Entry smoke for the TeX profile: one minimal document reaches SVG.

use std::path::{Path, PathBuf};

use document_compose::Severity;
use document_fonts::{MathFontPack, install_math_fonts};
use document_input_tex::render_svg;

/// Math bytes are host-owned, so read the VuTeX pack beside this repository,
/// or the directory named by `VUTEX_FONT_DIR`. A missing pack fails the test.
fn install_test_math_font() {
    let dir = std::env::var("VUTEX_FONT_DIR").map(PathBuf::from).unwrap_or_else(|_| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .expect("workspace root")
            .join("VuTeX")
            .join("projects")
            .join("vutex.rs")
            .join("fonts")
    });
    assert!(dir.is_dir(), "tex smoke needs the host font pack at {}; set VUTEX_FONT_DIR", dir.display());
    install_math_fonts(&MathFontPack::load_from(&dir).expect("math font pack"));
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
