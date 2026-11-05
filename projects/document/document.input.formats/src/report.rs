//! Conversion report projection for imported container documents.

use document_compose::{
    ConversionReport, CoverageSummary, Diagnostic, LossyItem, OpaqueItem, PreservedItem, Severity, SourceSpan,
    UnsupportedItem,
};
use document_ir::{Block, DocumentGraph, SemanticStatus};

pub(crate) fn analyze(graph: &DocumentGraph) -> (ConversionReport, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut preserved = Vec::new();
    let mut unsupported = Vec::new();
    let mut opaque = Vec::new();
    let mut lossy = Vec::new();

    for (index, marker) in graph.coverage.loss.iter().enumerate() {
        let span = graph.loss_span(index).map(|span| SourceSpan { start: span.start, end: span.end });
        match marker.status {
            SemanticStatus::Unsupported => {
                unsupported.push(UnsupportedItem { code: marker.code.clone(), detail: marker.message.clone() });
                diagnostics.push(Diagnostic {
                    code: marker.code.clone(),
                    severity: Severity::Error,
                    message: marker.message.clone(),
                    span,
                });
            }
            SemanticStatus::Unresolved | SemanticStatus::Partial | SemanticStatus::Inferred | SemanticStatus::Lossy => {
                lossy.push(LossyItem { code: marker.code.clone(), detail: marker.message.clone() });
                diagnostics.push(Diagnostic {
                    code: marker.code.clone(),
                    severity: Severity::Warning,
                    message: marker.message.clone(),
                    span,
                });
            }
            SemanticStatus::Resolved => {}
        }
    }

    let mut considered = 0_u32;
    let mut lowered = 0_u32;
    for node in &graph.blocks {
        considered += 1;
        match &node.block {
            Block::Opaque { kind, status, .. } => {
                let span = graph.block_span(node.id).map(|span| SourceSpan { start: span.start, end: span.end });
                let detail = format!("opaque `{kind}` block retained without semantic lowering");
                opaque.push(OpaqueItem { code: crate::codes::OPAQUE_BLOCK.to_owned(), detail: detail.clone() });
                diagnostics.push(Diagnostic {
                    code: crate::codes::OPAQUE_BLOCK.to_owned(),
                    severity: Severity::Info,
                    message: detail.clone(),
                    span,
                });
                if *status == SemanticStatus::Unsupported {
                    unsupported.push(UnsupportedItem { code: crate::codes::OPAQUE_BLOCK.to_owned(), detail });
                    diagnostics.push(Diagnostic {
                        code: crate::codes::OPAQUE_BLOCK.to_owned(),
                        severity: Severity::Error,
                        message: format!("unsupported opaque `{kind}` block"),
                        span,
                    });
                }
            }
            Block::RawHtml { html } => {
                preserved.push(PreservedItem {
                    code: "import.html.raw_html_preserved".to_owned(),
                    detail: format!("raw HTML block preserved verbatim ({} bytes)", html.len()),
                });
                lowered += 1;
            }
            _ => lowered += 1,
        }
    }

    let report = ConversionReport {
        parsed: CoverageSummary { considered, handled: considered },
        lowered: CoverageSummary { considered, handled: lowered },
        preserved,
        unsupported,
        opaque,
        lossy,
    };
    (report, diagnostics)
}
