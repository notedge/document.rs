//! Fail-closed entry contract: rejected input and unreadable or oversized
//! sources produce structured diagnostics instead of silent degradation.

use document_compose::Severity;
use document_input_tex::{IO_FAILED, MAX_SOURCE_BYTES, PARSE_FAILED, SOURCE_TOO_LARGE, import_file, import_source};

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
