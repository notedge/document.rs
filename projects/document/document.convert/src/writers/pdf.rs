use std::sync::Arc;

use boost_diagnostic::{Diagnostic as UnifiedDiagnostic, DiagnosticCode, DiagnosticOrigin, DiagnosticSeverity, Message};
use document_compose::diagnostic::Diagnostic as ComposeDiagnostic;
use document_fonts::{BodyFaceSet, BodyMetrics};
use document_ir::{AssetKind, Block, DocumentGraph, LossMarker, SemanticStatus};
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
pub fn write_document_pdf(graph: &mut DocumentGraph, body_set: &Arc<BodyFaceSet>) -> Result<Vec<u8>> {
    for source in crate::writers::unresolved_image_sources(graph) {
        graph.push_loss(LossMarker {
            code: "export.pdf.image_unresolved".into(),
            message: format!(
                "image `{source}` has no captured bytes. The page keeps a dashed placeholder instead of the picture"
            ),
            status: SemanticStatus::Partial,
        });
    }
    let (diagrams, losses) = lower_host_scenes(graph);
    // Lowering warnings and parser errors are environment facts the logger
    // records. A canvas that has no area is a content loss, because the page
    // then has no picture and no scene, so only that code becomes a marker.
    for loss in &losses {
        if loss.fatal && loss.code.starts_with("panduck.") {
            graph.push_loss(LossMarker {
                code: loss.code.clone(),
                message: loss.message.clone(),
                status: SemanticStatus::Unsupported,
            });
        }
    }
    if !losses.is_empty() {
        report_scene_losses(&losses);
    }
    let metrics = BodyMetrics::from_set(body_set);
    let hint = layout_hint(graph);
    let (plan, layout_diagnostics) = document_layout::document::layout_with_metrics(graph, &hint, &diagrams, &metrics);
    for diagnostic in &layout_diagnostics {
        if let Some(loss) = pdf_layout_loss(diagnostic) {
            let span = diagnostic.span.map(|span| document_ir::SourceSpan { start: span.start, end: span.end });
            graph.push_loss_at(loss, span);
        }
    }
    report_compose_diagnostics(&layout_diagnostics);
    let presentation = build_display_presentation(&plan);
    let resources = pdf_resources(graph);
    let outcome =
        render_pdf_with_resources_and_diagnostics(&presentation, &PdfFontPolicy::Provided(Arc::clone(body_set)), &resources)
            .map_err(map_render_diagnostic)?;
    // Render warnings that replace or skip a glyph have no error severity, so
    // the request succeeds and only this report can say the glyph was lost.
    // Every other render diagnostic is either fatal (returned above) or a
    // paint note that changes no authored content.
    for diagnostic in &outcome.diagnostics {
        if matches!(diagnostic.code.as_str(), "vutex.render.pdf.font_unavailable" | "vutex.render.pdf.glyph_missing") {
            graph.push_loss(LossMarker {
                code: diagnostic.code.clone(),
                message: diagnostic.message.clone(),
                status: SemanticStatus::Partial,
            });
        }
    }
    report_compose_diagnostics(&outcome.diagnostics);
    Ok(outcome.pdf)
}

/// Turn a layout diagnostic that dropped document content into a loss marker.
///
/// Only codes whose degradation removes or changes authored content belong in
/// the conversion report. Layout also emits font-metric, typesetting-approximation
/// and conformance-fallback diagnostics, which describe the rendering environment
/// rather than the document, so filtering them keeps a faithful page from being
/// reported as lossy while every content loss stays visible.
fn pdf_layout_loss(diagnostic: &ComposeDiagnostic) -> Option<LossMarker> {
    let code = diagnostic.code.as_str();
    if !matches!(
        code,
        "export.pdf.citation_degraded"
            | "export.pdf.image_title_dropped"
            | "export.pdf.image_alt_dropped"
            | "export.pdf.reference_target_dropped"
            | "export.pdf.inline_attributes_dropped"
            | "export.pdf.code_language_dropped"
            | "export.pdf.opaque_metadata_dropped"
            | "export.pdf.inline_style_dropped"
            | "export.pdf.table_cell_flattened"
            | "export.pdf.raw_html_flattened"
            | "vutex.layout.math.unsupported_language"
            | "vutex.layout.math.not_typeset"
            | "vutex.layout.footnote_definition_unparsed"
            | "vutex.layout.math.outline_missing"
            | "vutex.math.unsupported_expression"
    ) {
        return None;
    }
    Some(LossMarker { code: code.to_string(), message: diagnostic.message.clone(), status: SemanticStatus::Partial })
}

/// Layout intent the graph declared, with A4 filling every gap.
///
/// Page box, margins, body size and line height are presentation facts the
/// source declared, so a PDF written here keeps the source's geometry instead
/// of resetting every document to A4 at 11 pt.
fn layout_hint(graph: &DocumentGraph) -> LayoutHint {
    let mut hint = LayoutHint::a4();
    let Some(page) = graph.page_setup
    else {
        return hint;
    };
    hint = hint.with_page_size(page.page_width.unwrap_or(hint.page_width), page.page_height.unwrap_or(hint.page_height));
    hint = hint.with_margin(document_layout::hint::Margin {
        top: page.margin_top.unwrap_or(hint.margin.top),
        right: page.margin_right.unwrap_or(hint.margin.right),
        bottom: page.margin_bottom.unwrap_or(hint.margin.bottom),
        left: page.margin_left.unwrap_or(hint.margin.left),
    });
    if let Some(size) = page.base_font_size {
        hint.base_font_size = size;
    }
    if let Some(line_height) = page.line_height {
        hint.line_height = line_height;
    }
    hint
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
