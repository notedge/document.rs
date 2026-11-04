//! Smoke coverage for the TeX profile: one minimal document reaches SVG
//! through the profile's own closed loop.

use document_compose::Severity;
use document_input_tex::render_svg;

#[test]
fn structures_reach_the_rendered_svg() {
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
    assert!(rendered.svg.contains("E = mc^2"), "the formula source must reach the svg");
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
