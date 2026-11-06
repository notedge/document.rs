//! TeX 导入 → layout 结构探针（钉死后可收成断言套件）。

use document_input_tex::import_source;
use document_layout::{BodyMetrics, Diagrams, LayoutHint, document::layout_with_diagnostics, plan::LayoutItem};

fn dump(label: &str, source: &str) {
    let document = import_source(label, source).expect("source must import");
    println!("========== {label} ==========");
    println!("metadata: {:?}", document.graph.metadata);
    for node in &document.graph.blocks {
        println!("block {}: {:#?}", node.id.0, node.block);
    }
    println!("loss:");
    for marker in &document.graph.coverage.loss {
        println!("  {} | {:?} | {}", marker.code, marker.status, marker.message);
    }
    println!("report: {:#?}", document.report);
    println!("diagnostics: {:#?}", document.diagnostics);
    let (plan, diagnostics) =
        layout_with_diagnostics(&document.graph, &LayoutHint::a4(), &Diagrams::default(), &BodyMetrics::default());
    println!("layout diagnostics: {:#?}", diagnostics);
    for page in &plan.pages {
        for item in &page.items {
            if let LayoutItem::TextRun(run) = item {
                println!("text run: {:?}", run.text);
            }
        }
    }
}

#[test]
fn probe_metadata() {
    dump(
        "metadata",
        r"\documentclass{article}
\title{Profile Title}
\author{A. Author}
\begin{document}
\maketitle
\section{First}
Body text.
\end{document}
",
    );
}

#[test]
fn probe_styles() {
    dump(
        "styles",
        r"\begin{document}
A \textbf{bold} B \emph{italic} C \texttt{mono} D \underline{line} E.
\end{document}
",
    );
}

#[test]
fn probe_tabular() {
    dump(
        "tabular",
        r"\begin{document}
\begin{tabular}{ll}
Name & Value \\
a & 1 \\
\end{tabular}
\end{document}
",
    );
    dump(
        "table float",
        r"\begin{document}
\begin{table}
\begin{tabular}{ll}
Name & Value \\
\end{tabular}
\end{table}
\end{document}
",
    );
}

#[test]
fn probe_math_numbering() {
    dump(
        "math numbering",
        r"\begin{document}
\begin{equation}
a = b
\end{equation}
\begin{align}
c &= d \\
e &= f
\end{align}
\end{document}
",
    );
}

#[test]
fn probe_references() {
    dump(
        "references",
        r"\begin{document}
\section{Target}\label{sec:target}
See Section~\ref{sec:target} and \cite{knuth} plus a note\footnote{Note body.}.
\end{document}
",
    );
}
