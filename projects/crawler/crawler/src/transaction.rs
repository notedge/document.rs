use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_types::{CrawlError, ExtractStatus, PageTransaction, Result, diagnostics as pd};

/// Validate and commit a page transaction.
pub fn commit_page(transaction: PageTransaction) -> Result<String> {
    match transaction.extract.status {
        ExtractStatus::Failed | ExtractStatus::Unsupported => {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::EXTRACT_PARSE_FAILED),
                DiagnosticSeverity::Error,
                pd::origin_extract(),
                Message::new("crawler.diagnostic.pageCommitFailed")
                    .with_arg("page", MessageArg::Text(transaction.page_identity.clone()))
                    .with_arg("status", MessageArg::Text(status_label(transaction.extract.status).to_string()))
                    .with_fallback(format!(
                        "page `{}` cannot be committed with status `{}`",
                        transaction.page_identity,
                        status_label(transaction.extract.status)
                    )),
            );
            return Err(CrawlError::from(diag));
        }
        ExtractStatus::Complete | ExtractStatus::Partial => {}
    }

    let document = transaction.extract.document.as_ref().ok_or_else(|| {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::EXTRACT_PARSE_FAILED),
            DiagnosticSeverity::Error,
            pd::origin_extract(),
            Message::new("crawler.diagnostic.missingDocumentGraph").with_fallback("missing document graph".to_string()),
        );
        CrawlError::from(diag)
    })?;
    let validation = document.validate();
    if !validation.is_valid() {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::EXTRACT_PARSE_FAILED),
            DiagnosticSeverity::Error,
            pd::origin_extract(),
            Message::new("crawler.diagnostic.documentValidationFailed")
                .with_arg("page", MessageArg::Text(transaction.page_identity.clone()))
                .with_fallback(format!("document validation failed for `{}`", transaction.page_identity)),
        );
        return Err(CrawlError::from(diag));
    }

    Ok(transaction.page_identity)
}

fn status_label(status: ExtractStatus) -> &'static str {
    match status {
        ExtractStatus::Complete => "complete",
        ExtractStatus::Partial => "partial",
        ExtractStatus::Unsupported => "unsupported",
        ExtractStatus::Failed => "failed",
    }
}
