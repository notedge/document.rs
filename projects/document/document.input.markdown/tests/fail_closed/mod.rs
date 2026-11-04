//! Smoke coverage for the Markdown profile: unreadable and unsupported sources
//! must fail closed instead of yielding a partial document.

use document_compose::Severity;
use document_input_markdown::{MAX_SOURCE_BYTES, codes, import_file, import_source};

#[test]
fn front_matter_fails_closed_with_span() {
    let source = "---\ntitle: ignored\n---\n\nBody\n";
    let error = import_source("virtual://front-matter.md", source)
        .expect_err("front matter carries metadata the document model cannot consume");

    assert_eq!(error.code(), codes::OPAQUE_BLOCK);
    let diagnostic = error.diagnostics().first().expect("the rejection must carry its diagnostic");
    let span = diagnostic.span.expect("the front matter block must keep its source span");
    assert_eq!(&source[span.start as usize..span.end as usize], "---\ntitle: ignored\n---");
}

#[test]
fn empty_source_imports_as_a_documented_lossy_document() {
    let document = import_source("virtual://empty.md", "").expect("an empty source is a defined input, not a failure");

    assert!(document.graph.blocks.is_empty(), "{:#?}", document.graph.blocks);
    assert!(
        document.report.lossy.iter().any(|item| item.code == "import.markdown.empty_document"),
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
    let error = import_source("virtual://too-large.md", &source).expect_err("the byte budget must fail closed");

    assert_eq!(error.code(), codes::SOURCE_TOO_LARGE);
    assert!(error.message().contains(&MAX_SOURCE_BYTES.to_string()), "{}", error.message());
    assert_eq!(error.diagnostics().len(), 1, "{:#?}", error.diagnostics());
}

#[test]
fn lossy_construct_stays_importable_while_unsupported_construct_is_rejected() {
    let lossy = import_source("virtual://definition-list.md", "Term\n: definition\n")
        .expect("a reported downgrade must not stop the import");
    assert!(
        lossy.diagnostics.iter().any(|diagnostic| diagnostic.code == "import.markdown.definition_list"),
        "the downgrade must stay visible: {:#?}",
        lossy.diagnostics
    );
    assert!(lossy.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error), "{:#?}", lossy.diagnostics);

    let unsupported = import_source("virtual://front-matter.md", "---\ntitle: ignored\n---\n\nBody\n")
        .expect_err("an unsupported construct must stop the import");
    assert_eq!(unsupported.code(), codes::OPAQUE_BLOCK);
}

#[test]
fn unreadable_file_fails_closed() {
    let path = std::env::temp_dir().join("vutex-input-markdown-missing.md");
    let _ = std::fs::remove_file(&path);
    let error = import_file(&path).expect_err("a missing source must not import as an empty document");

    assert_eq!(error.code(), codes::FILE_READ_FAILED);
    assert_eq!(error.diagnostics().len(), 1, "{:#?}", error.diagnostics());
}
