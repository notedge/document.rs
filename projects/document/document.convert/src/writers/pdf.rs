use std::sync::Arc;

use boost_diagnostic::{Diagnostic as UnifiedDiagnostic, DiagnosticCode, DiagnosticOrigin, DiagnosticSeverity, Message};
use document_compose::diagnostic::Diagnostic as ComposeDiagnostic;
use document_fonts::{BodyFaceSet, BodyMetrics};
use document_ir::{AssetKind, Block, DocumentGraph};
use document_layout::{Diagrams, LayoutHint};
use document_presentation::build_display_presentation;
use document_renderer_pdf::{PdfFontPolicy, PdfResources, render_pdf_with_resources_and_diagnostics};
use document_types::{AdapterError, Result};

/// Non-fatal degradation reported while assembling the PDF input.
struct LossNotice {
    code: String,
    message: String,
    fatal: bool,
}

/// Writes `notedown-ir` to PDF bytes through the deliverable
/// `layout → presentation → renderer-pdf` chain.
///
/// `body_set` is injected by the host. A delivered PDF embeds every body face
/// the host supplied, so it never depends on the faces the reader happens to
/// have installed.
pub fn write_document_pdf(graph: &DocumentGraph, body_set: &Arc<BodyFaceSet>) -> Result<Vec<u8>> {
    let (diagrams, losses) = lower_host_scenes(graph);
    if !losses.is_empty() {
        report_scene_losses(&losses);
    }
    let metrics = BodyMetrics::from_set(body_set);
    let (plan, layout_diagnostics) = document_layout::document::layout_with_metrics(graph, &LayoutHint::a4(), &diagrams, &metrics);
    report_compose_diagnostics(&layout_diagnostics);
    let presentation = build_display_presentation(&plan);
    let resources = pdf_resources(graph);
    let outcome = render_pdf_with_resources_and_diagnostics(&presentation, &PdfFontPolicy::Provided(Arc::clone(body_set)), &resources)
        .map_err(map_render_diagnostic)?;
    report_compose_diagnostics(&outcome.diagnostics);
    Ok(outcome.pdf)
}

/// Lower every host-owned diagram and chart block to the display scene the
/// layout pass places. Blocks the host cannot lower keep no scene, so the
/// layout falls back to the authored source, and the failure is reported here.
fn lower_host_scenes(graph: &DocumentGraph) -> (Diagrams, Vec<LossNotice>) {
    let mut diagrams = Diagrams::new();
    let mut losses = Vec::new();
    for node in &graph.blocks {
        match &node.block {
            Block::Diagram { language, source } => {
                let language = language.trim();
                let lowered = if language.eq_ignore_ascii_case("mermaid") {
                    plotter_input_mermaid::mermaid_to_scene(source)
                        .map_err(|error| (error.code().to_string(), error.message().to_string()))
                }
                else if language.eq_ignore_ascii_case("dot") {
                    plotter_input_dot::dot_to_scene(source)
                        .map_err(|error| (error.code().to_string(), error.message().to_string()))
                }
                else if language.eq_ignore_ascii_case("plantuml") || language.eq_ignore_ascii_case("puml") {
                    plotter_input_plantuml::plantuml_to_scene(source)
                        .map_err(|error| (error.code().to_string(), error.message().to_string()))
                }
                else {
                    continue;
                };
                match lowered {
                    Ok(lowered) => {
                        for diagnostic in &lowered.diagnostics {
                            losses.push(LossNotice {
                                code: diagnostic.code().as_str().to_string(),
                                message: diagnostic.message().to_string(),
                                fatal: false,
                            });
                        }
                        let size = lowered.scene.size;
                        if size.width <= 0.0 || size.height <= 0.0 {
                            losses.push(LossNotice {
                                code: "panduck.diagram.invalid-canvas".to_string(),
                                message: format!("Diagram lowered to a non-positive canvas: {size:?}"),
                                fatal: true,
                            });
                            continue;
                        }
                        diagrams.insert(node.id, lowered.scene, format!("{language} diagram"));
                    }
                    Err((code, message)) => losses.push(LossNotice { code, message, fatal: true }),
                }
            }
            Block::Chart { language, source } if language.trim().eq_ignore_ascii_case("vega-lite") => {
                match plotter_chart::vega_lite::lower(source) {
                    Ok(chart) => {
                        for diagnostic in &chart.diagnostics {
                            losses.push(LossNotice {
                                code: diagnostic.code().as_str().to_string(),
                                message: diagnostic.message().to_string(),
                                fatal: false,
                            });
                        }
                        let scene = chart.scene.resolve(&Default::default());
                        let size = scene.size;
                        if size.width <= 0.0 || size.height <= 0.0 {
                            losses.push(LossNotice {
                                code: "panduck.chart.invalid-canvas".to_string(),
                                message: format!("Chart lowered to a non-positive canvas: {size:?}"),
                                fatal: true,
                            });
                            continue;
                        }
                        diagrams.insert(node.id, scene, "vega-lite chart");
                    }
                    Err(error) => losses.push(LossNotice {
                        code: error.code().to_string(),
                        message: error.message().to_string(),
                        fatal: true,
                    }),
                }
            }
            _ => {}
        }
    }
    (diagrams, losses)
}

/// Image bytes the graph already materialized travel with the graph, so the
/// renderer never has to guess a storage location for an imported asset.
fn pdf_resources(graph: &DocumentGraph) -> PdfResources {
    let mut resources = PdfResources::new();
    for asset in graph.assets.iter().filter(|asset| asset.kind == AssetKind::Image) {
        if let (Some(source), Some(bytes)) = (&asset.source, &asset.bytes) {
            resources = resources.with_supplied(source, bytes.clone());
        }
    }
    resources
}

/// Error for a PDF request that arrived without host-supplied body faces.
pub(crate) fn missing_body_faces() -> AdapterError {
    let unified = UnifiedDiagnostic::new(
        DiagnosticCode::new("notedown.convert.pdf-fonts-missing".to_string()),
        DiagnosticSeverity::Error,
        DiagnosticOrigin::new("notedown", "convert").with_stage("pdf"),
        Message::new("notedown.convert.pdf-fonts-missing").with_fallback("PDF output needs host-supplied body faces"),
    );
    AdapterError::diagnostic(unified)
}

fn report_scene_losses(losses: &[LossNotice]) {
    for loss in losses {
        let severity = if loss.fatal { DiagnosticSeverity::Error } else { DiagnosticSeverity::Warning };
        let unified = UnifiedDiagnostic::new(
            DiagnosticCode::new(loss.code.clone()),
            severity,
            DiagnosticOrigin::new("davinci", "convert").with_stage("pdf"),
            Message::new("notedown.convert.pdf-scene-degraded").with_fallback(loss.message.clone()),
        );
        emit_diagnostic(&unified);
    }
}

fn report_compose_diagnostics(diagnostics: &[ComposeDiagnostic]) {
    for diagnostic in diagnostics {
        let severity = match diagnostic.severity {
            document_compose::Severity::Info => DiagnosticSeverity::Info,
            document_compose::Severity::Warning => DiagnosticSeverity::Warning,
            document_compose::Severity::Error => DiagnosticSeverity::Error,
        };
        let unified = UnifiedDiagnostic::new(
            DiagnosticCode::new(diagnostic.code.clone()),
            severity,
            DiagnosticOrigin::new("notedown", "convert").with_stage("pdf"),
            Message::new("notedown.convert.pdf-degraded").with_fallback(diagnostic.message.clone()),
        );
        emit_diagnostic(&unified);
    }
}

#[cfg(feature = "logger")]
fn emit_diagnostic(diagnostic: &UnifiedDiagnostic) {
    boost_diagnostic::emit_diagnostic(diagnostic);
}

#[cfg(not(feature = "logger"))]
fn emit_diagnostic(_diagnostic: &UnifiedDiagnostic) {}

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
