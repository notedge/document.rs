use boost_diagnostic::{Diagnostic as UnifiedDiagnostic, DiagnosticCode, DiagnosticOrigin, DiagnosticSeverity, Message};
use document_compose::diagnostic::Diagnostic as ComposeDiagnostic;
use document_ir::DocumentGraph;
use document_layout::{Diagrams, LayoutHint};
use document_presentation::build_display_presentation;
use document_renderer_pdf::{PdfFontPolicy, PdfResources, render_pdf_with_resources_and_diagnostics};
use document_types::{AdapterError, Result};

/// Writes `notedown-ir` to PDF bytes through the deliverable
/// `layout → presentation → renderer-pdf` chain.
pub fn write_document_pdf(graph: &DocumentGraph) -> Result<Vec<u8>> {
    let (plan, _diagnostics) =
        document_layout::document::layout_with_diagnostics(graph, &LayoutHint::a4(), &Diagrams::default());
    let presentation = build_display_presentation(&plan);
    // A delivered PDF must not depend on the faces the reader happens to have
    // installed, so every run embeds a bundled body face here.
    let outcome = render_pdf_with_resources_and_diagnostics(
        &presentation,
        &PdfFontPolicy::Bundled,
        &PdfResources::default(),
    )
    .map_err(map_render_diagnostic)?;
    Ok(outcome.pdf)
}

fn map_render_diagnostic(diagnostic: ComposeDiagnostic) -> AdapterError {
    let severity = match diagnostic.severity {
        document_compose::Severity::Info => DiagnosticSeverity::Info,
        document_compose::Severity::Warning => DiagnosticSeverity::Warning,
        document_compose::Severity::Error => DiagnosticSeverity::Error,
    };
    let unified = UnifiedDiagnostic::new(
        DiagnosticCode::new(diagnostic.code),
        severity,
        DiagnosticOrigin::new("notedown", "convert").with_stage("pdf"),
        Message::new("notedown.convert.pdf-render-failed").with_fallback(diagnostic.message),
    );
    AdapterError::diagnostic(unified)
}
