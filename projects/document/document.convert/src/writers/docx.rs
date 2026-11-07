use document_formats::export::docx::export_docx_bytes;
use document_ir::{DocumentGraph, LossMarker, SemanticStatus};
use document_types::Result;

use crate::{
    format_error::map_format_error,
    writers::{docx_dropped_inline_attributes, raw_html_occurrences, unresolved_image_sources},
};

/// Write `notedown-ir` to DOCX bytes via `notedown-formats`.
///
/// Pictures without materialized bytes cannot become package parts, so the
/// writer keeps a visible `[image unavailable: …]` marker. Each such image is
/// recorded as `export.docx.image_unresolved`: the marker names the gap, the
/// loss code stops the report from calling the export clean.
///
/// Raw HTML has no arbitrary markup slot either. Its authored characters stay
/// visible as literal text, and `export.docx.raw_html_flattened` records how
/// many nodes were projected that way so the report never reads as clean.
pub fn write_document_docx(graph: &mut DocumentGraph) -> Result<Vec<u8>> {
    for source in unresolved_image_sources(graph) {
        graph.push_loss(LossMarker {
            code: "export.docx.image_unresolved".into(),
            message: format!(
                "image `{source}` has no captured bytes. DOCX keeps the marker text instead of an embedded picture"
            ),
            status: SemanticStatus::Partial,
        });
    }
    let raw_html = raw_html_occurrences(graph);
    if raw_html > 0 {
        graph.push_loss(LossMarker {
            code: "export.docx.raw_html_flattened".into(),
            message: format!(
                "the DOCX target has no arbitrary HTML slot, so {raw_html} authored raw HTML node(s) are kept as literal markup text"
            ),
            status: SemanticStatus::Partial,
        });
    }
    let dropped_attributes = docx_dropped_inline_attributes(graph);
    if !dropped_attributes.is_empty() {
        graph.push_loss(LossMarker {
            code: "export.docx.inline_attributes_dropped".into(),
            message: format!(
                "the DOCX target has no slot for inline attribute {}, so the value is dropped while the wrapped text stays",
                dropped_attributes.join(", ")
            ),
            status: SemanticStatus::Partial,
        });
    }
    export_docx_bytes(graph).map_err(map_format_error)
}
