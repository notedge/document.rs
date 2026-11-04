//! 冒烟覆盖：不可读文件、空源、超预算源以及“构造级不中断”与“未闭合组中断”的边界。

use document_compose::Severity;
use document_input_typst::{MAX_SOURCE_BYTES, SOURCE_TOO_LARGE, UNTERMINATED_GROUP, import_path, import_source};

#[test]
fn unreadable_file_fails_closed() {
    let path = std::env::temp_dir().join("vutex-input-typst-missing.typ");
    let _ = std::fs::remove_file(&path);
    let error = import_path(&path).expect_err("a missing source must not import as an empty document");

    assert_eq!(error.code(), document_input_typst::IMPORT_FAILED);
    assert!(error.message().contains("failed to read"), "{}", error.message());
}

#[test]
fn empty_source_imports_as_a_documented_lossy_document() {
    let document = import_source("virtual://empty.typ", "").expect("an empty source is a defined input, not a failure");

    assert!(document.graph.blocks.is_empty(), "{:#?}", document.graph.blocks);
    assert!(
        document.report.lossy.iter().any(|item| item.code == "import.typst.empty_document"),
        "the empty document must be reported as a loss: {:#?}",
        document.report.lossy
    );
    assert!(
        document.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error),
        "a documented empty input must not raise an error: {:#?}",
        document.diagnostics
    );
}

#[test]
fn source_over_budget_is_rejected_before_parsing() {
    let source = "x".repeat(MAX_SOURCE_BYTES + 1);
    let error = import_source("virtual://too-large.typ", &source).expect_err("the byte budget must fail closed");

    assert_eq!(error.code(), SOURCE_TOO_LARGE);
    assert!(error.message().contains(&MAX_SOURCE_BYTES.to_string()), "{}", error.message());
}

#[test]
fn opaque_construct_stays_importable_while_unclosed_group_is_rejected() {
    let closed = import_source("virtual://closed.typ", "Before\n\n#set page(width: 10cm)\n\nAfter\n")
        .expect("a balanced construct must stay opaque without stopping the import");
    assert!(
        closed.report.unsupported.iter().any(|item| item.code == "import.typst.dynamic_content"),
        "the opaque construct must stay visible: {:#?}",
        closed.report.unsupported
    );
    assert_eq!(closed.report.opaque.len(), 1, "{:#?}", closed.report.opaque);

    let unclosed = import_source("virtual://unclosed.typ", "Before\n\n#set page(width: 10cm\n\nAfter\n")
        .expect_err("an unclosed group cannot be delimited and must fail closed");
    assert_eq!(unclosed.code(), UNTERMINATED_GROUP);
    assert_eq!(unclosed.diagnostic.severity, Severity::Error);
    assert!(unclosed.diagnostic.span.is_some(), "the unclosed construct must keep its range");
}
