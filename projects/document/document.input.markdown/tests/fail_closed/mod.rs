//! Smoke coverage for the Markdown profile: unreadable and unsupported sources
//! must fail closed instead of yielding a partial document.

use document_input_markdown::{codes, import_file, import_source};

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
fn unreadable_file_fails_closed() {
    let path = std::env::temp_dir().join("vutex-input-markdown-missing.md");
    let _ = std::fs::remove_file(&path);
    let error = import_file(&path).expect_err("a missing source must not import as an empty document");

    assert_eq!(error.code(), codes::FILE_READ_FAILED);
    assert_eq!(error.diagnostics().len(), 1, "{:#?}", error.diagnostics());
}
