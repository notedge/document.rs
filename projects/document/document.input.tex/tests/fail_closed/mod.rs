//! Fail-closed contract: rejected input, opaque constructs, and foreign math
//! language tags produce structured diagnostics instead of silent degradation.

use document_compose::Severity;
use document_input_tex::{
    INVALID_GRAPH, IO_FAILED, MATH_LANGUAGE, MAX_SOURCE_BYTES, PARSE_FAILED, SOURCE_TOO_LARGE, conversion_report, import_file,
    import_source, render_svg, validate_graph,
};
use document_ir::{Block, DocumentGraph, DocumentId, Inline, ListItem, MathDisplay, MathFormula};

const UNKNOWN_ENVIRONMENT: &str = r"\begin{document}
\begin{figure}
\centering
\includegraphics{plot.pdf}
\end{figure}
\end{document}
";

#[test]
fn rejected_source_returns_a_structured_error() {
    let error = import_source(
        "broken.tex",
        r"\begin{document}
\section{unterminated
\end{document}
",
    )
    .expect_err("an unclosed group must not yield a partial document");

    assert_eq!(error.code(), PARSE_FAILED);
    assert!(!error.message().is_empty(), "the frontend message must be preserved");
    assert_eq!(error.diagnostic.severity, Severity::Error);
}

#[test]
fn unknown_environment_is_reported_with_its_source_span() {
    let document = import_source("figure.tex", UNKNOWN_ENVIRONMENT).expect("the frontend accepts the syntax");

    assert_eq!(document.report.opaque.len(), 1, "one environment must stay opaque: {:#?}", document.report);
    assert_eq!(document.report.opaque[0].code, "tex.environment.figure");
    assert!(
        document.report.unsupported.iter().any(|item| item.code == "import.tex.unknown_environment"),
        "the lowering loss must reach the report: {:#?}",
        document.report.unsupported
    );

    let opaque = document
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "tex.environment.figure")
        .expect("the opaque environment must be diagnosed");
    let span = opaque.span.expect("the authored environment must be located in the source");
    assert_eq!(&UNKNOWN_ENVIRONMENT[span.start as usize..span.end as usize], document.report.opaque[0].detail);
    assert_eq!(opaque.severity, Severity::Warning, "opaque content is a reported loss, not a failed import");
    assert!(
        !document.diagnostics.iter().any(|diagnostic| diagnostic.code == MATH_LANGUAGE),
        "opaque content is not a math language failure"
    );
}

#[test]
fn invalid_graph_is_rejected_before_layout() {
    let mut graph = DocumentGraph::new(DocumentId(9));
    graph.push_block(Block::List {
        ordered: true,
        items: vec![ListItem {
            content: vec![Inline::Text { text: "no number".to_string() }],
            children: Vec::new(),
            number: None,
            is_task: false,
            is_checked: None,
        }],
    });

    let error = validate_graph(&graph).expect_err("an ordered item without a number breaks the list invariant");
    assert_eq!(error.code(), INVALID_GRAPH);
    assert!(error.message().contains("MissingListItemNumber"), "the invariant must be named: {}", error.message());
}

#[test]
fn foreign_math_language_fails_closed() {
    let mut graph = DocumentGraph::new(DocumentId(7));
    graph.push_block(Block::Math {
        formula: MathFormula::unsupported("x^2", Some("typst".to_string()), MathDisplay::Display, "typst"),
    });

    let report = conversion_report(&graph);
    assert!(
        report.unsupported.iter().any(|item| item.code == MATH_LANGUAGE),
        "foreign math must be reported: {:#?}",
        report.unsupported
    );
    assert_eq!(report.lowered.handled, 0, "an unsupported formula is not a lowered block");

    let diagnostics = document_input_tex::conversion_diagnostics(&graph, "");
    let mismatch =
        diagnostics.iter().find(|diagnostic| diagnostic.code == MATH_LANGUAGE).expect("foreign math must be diagnosed");
    assert_eq!(mismatch.severity, Severity::Error, "a foreign formula must not pass silently to TeX layout");
}

#[test]
fn tex_math_without_a_language_tag_stays_accepted() {
    let mut graph = DocumentGraph::new(DocumentId(8));
    graph.push_block(Block::Math { formula: MathFormula::unsupported("x^2", None, MathDisplay::Inline, "tex") });

    let report = conversion_report(&graph);
    assert!(report.unsupported.is_empty(), "untagged math belongs to the TeX profile: {:#?}", report.unsupported);
    assert!(document_input_tex::conversion_diagnostics(&graph, "").is_empty());
}

#[test]
fn empty_source_keeps_document_level_losses_unpositioned() {
    let document = import_source("empty.tex", "").expect("an empty source still imports as an empty document");

    assert_eq!(document.report.parsed.considered, 0);
    assert_eq!(document.report.parsed.handled, 0);
    assert!(document.report.lossy.iter().any(|item| item.code == "import.tex.empty_document"), "{:#?}", document.report.lossy);
    assert!(document.diagnostics.is_empty(), "document-level summaries have no source range: {:#?}", document.diagnostics);
}

#[test]
fn renderer_rejection_keeps_its_own_diagnostic_code() {
    // The SVG renderer only rejects a graphic whose canvas size is non-positive,
    // and a TeX source cannot carry a host-registered scene into this entry
    // point. The adapter's only job on that path is passing the renderer
    // diagnostic through unchanged, which this unit-level check pins.
    let diagnostic = document_compose::Diagnostic {
        code: "vutex.render.svg.diagram-invalid".to_string(),
        severity: Severity::Error,
        message: "diagram has a non-positive canvas size".to_string(),
        span: None,
    };
    let error = document_input_tex::TexInputError::from(diagnostic);
    assert_eq!(error.code(), "vutex.render.svg.diagram-invalid");
    assert_eq!(error.diagnostic.severity, Severity::Error);
    assert!(error.message().contains("non-positive canvas size"));
}

#[test]
fn repeated_constructs_keep_their_own_spans() {
    let source = r"\begin{document}
\begin{figure}One\end{figure}
\begin{figure}Two\end{figure}
\end{document}
";
    let imported = import_source("repeated.tex", source).expect("unknown environments are construct-level losses");

    let mut spans: Vec<_> = imported
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "import.tex.unknown_environment")
        .map(|diagnostic| diagnostic.span.expect("each construct must keep its own range"))
        .collect();
    assert_eq!(spans.len(), 2, "{:#?}", imported.diagnostics);
    spans.sort_by_key(|span| span.start);
    assert!(spans[0].end < spans[1].start, "the two ranges must not collapse onto the first occurrence: {spans:?}");
    assert!(source[spans[0].start as usize..spans[0].end as usize].contains("One"), "{spans:?}");
    assert!(source[spans[1].start as usize..spans[1].end as usize].contains("Two"), "{spans:?}");
}

#[test]
fn unreadable_file_fails_closed() {
    let path = std::env::temp_dir().join("vutex-input-tex-missing.tex");
    let _ = std::fs::remove_file(&path);
    let error = import_file(&path).expect_err("a missing source must not import as an empty document");

    assert_eq!(error.code(), IO_FAILED);
    assert!(error.message().contains("failed to read"), "{}", error.message());
}

#[test]
fn source_over_budget_is_rejected_before_parsing() {
    let source = "x".repeat(MAX_SOURCE_BYTES + 1);
    let error = import_source("too-large.tex", &source).expect_err("the byte budget must fail closed");

    assert_eq!(error.code(), SOURCE_TOO_LARGE);
    assert!(error.message().contains(&MAX_SOURCE_BYTES.to_string()), "{}", error.message());
}

#[test]
fn opaque_construct_does_not_stop_the_closed_loop() {
    let source = r"\begin{document}
Body text.

\begin{figure}
\includegraphics{plot.pdf}
\end{figure}
\end{document}
";
    let rendered = render_svg("figure.tex", source).expect("a construct-level loss must not stop render_svg");

    assert_eq!(rendered.report.opaque.len(), 1, "{:#?}", rendered.report.opaque);
    assert!(
        rendered.report.unsupported.iter().any(|item| item.code == "import.tex.unknown_environment"),
        "the loss must stay in the report: {:#?}",
        rendered.report.unsupported
    );
    assert!(rendered.diagnostics.iter().all(|diagnostic| diagnostic.code != PARSE_FAILED), "{:#?}", rendered.diagnostics);
    assert!(rendered.svg.contains("Body text"), "the rest of the document must still render");
}

#[test]
fn file_import_reads_the_same_source() {
    let path = std::env::temp_dir().join("vutex-input-tex-file.tex");
    std::fs::write(&path, "Text $x^2$ in a file.\n").expect("the fixture must be written");
    let document = import_file(&path).expect("the file must import");
    assert_eq!(document.report.parsed.handled, 1);
    std::fs::remove_file(&path).expect("the fixture must be removed");
}
