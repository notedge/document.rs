//! Smoke coverage for the Markdown profile: an unreadable source must fail
//! closed instead of yielding an empty document.

use document_input_markdown::{codes, import_file};

#[test]
fn unreadable_file_fails_closed() {
    let path = std::env::temp_dir().join("vutex-input-markdown-missing.md");
    let _ = std::fs::remove_file(&path);
    let error = import_file(&path).expect_err("a missing source must not import as an empty document");

    assert_eq!(error.code(), codes::FILE_READ_FAILED);
    assert_eq!(error.diagnostics().len(), 1, "{:#?}", error.diagnostics());
}
