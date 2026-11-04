//! Smoke coverage for the TeX profile: one minimal document reaches SVG
//! through the profile's own closed loop.

use document_compose::Severity;
use document_input_tex::{import_source, render_svg};
use document_text::math::{MathFontFamily, install_math_font};

/// The MATH font payload is a test asset, so the suite never depends on
/// `E:\文档研究\VuTeX` existing on the machine.
fn install_test_math_font() {
    install_math_font(MathFontFamily::LmMath, include_bytes!("../../../test-assets/fonts/LatinModernMath-Regular.otf"));
}

#[test]
fn structures_reach_the_rendered_svg() {
    install_test_math_font();
    let source = r"\begin{document}
\section{Structures}

\begin{tabular}{ll}
Alpha & Beta \\
Gamma & Delta \\
\end{tabular}

\begin{equation}
E = mc^2
\label{eq:energy}
\end{equation}

Body\footnote{Footnote body} text.

Citation \cite{smith2020}.

See equation \ref{eq:energy}.
\end{document}
";
    let rendered = render_svg("virtual://tex-structures.tex", source).expect("the structures must render");

    assert!(rendered.svg.contains("Alpha") && rendered.svg.contains("Gamma"), "table cells must reach the svg");
    assert!(
        !rendered.diagnostics.iter().any(|diagnostic| diagnostic.code == "vutex.layout.math.not_typeset"),
        "the embedded math font must typeset the display formula: {:#?}",
        rendered.diagnostics
    );
    assert!(rendered.svg.contains("<path"), "the display formula must be painted as outlines");
    assert!(!rendered.svg.contains("(1)"), "an unnumbered display must not grow a number in the svg");
    assert!(rendered.svg.contains("Footnote body"), "the footnote body must reach the svg");
    assert!(rendered.svg.contains("[@smith2020]"), "the citation marker must reach the svg");
    assert!(rendered.svg.contains("See equation 1"), "the reference must resolve to the numbered formula");
    assert!(
        rendered.diagnostics.iter().any(|diagnostic| diagnostic.code == "import.tex.math_numbering"),
        "the unplaceable equation number must stay a reported loss: {:#?}",
        rendered.diagnostics
    );
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

#[test]
fn citations_and_unresolved_references_report_their_authored_site() {
    let source = "Body \\cite{smith2020} and \\ref{missing}.\n";
    let imported = import_source("virtual://tex-cite.tex", source).expect("citations and dangling references are downgrades");

    let citation = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "import.tex.citation_marker")
        .expect("the citation marker must be reported");
    assert_eq!(citation.severity, Severity::Warning);
    let span = citation.span.expect("the citation loss must keep its authored range");
    assert_eq!(&source[span.start as usize..span.end as usize], "\\cite{smith2020}");

    let reference = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "import.tex.unresolved_reference")
        .expect("the dangling reference must be reported");
    let span = reference.span.expect("the unresolved reference must keep the site it was written at");
    assert_eq!(&source[span.start as usize..span.end as usize], "\\ref{missing}");
}

#[test]
fn text_mode_math_style_keeps_its_argument_and_reports_the_loss() {
    let source = "Body \\mathbf{Alpha} text.\n";
    let imported = import_source("virtual://tex-math-style.tex", source).expect("a stray font command is a downgrade");
    let rendered = render_svg("virtual://tex-math-style.tex", source).expect("render must succeed");
    assert!(rendered.svg.contains("Alpha"), "the argument must not disappear: {}", rendered.svg);

    let loss = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "import.tex.math_style_in_text")
        .expect("the downgrade must be diagnosed");
    assert_eq!(loss.severity, Severity::Warning);
    let span = loss.span.expect("the downgrade must keep its authored range");
    let authored = &source[span.start as usize..span.end as usize];
    assert!(authored.contains("\\mathbf{Alpha}"), "the span must cover the command: {authored:?}");
}
