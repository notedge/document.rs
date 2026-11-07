use document_formats::export::docx::export_docx_bytes;
use document_ir::{DocumentGraph, LossMarker, SemanticStatus};
use document_types::Result;

use crate::{format_error::map_format_error, writers::unresolved_image_sources};

/// Write `notedown-ir` to DOCX bytes via `notedown-formats`.
///
/// Pictures without materialized bytes cannot become package parts, so the
/// writer keeps a visible `[image unavailable: …]` marker. Each such image is
/// recorded as `export.docx.image_unresolved`: the marker names the gap, the
/// loss code stops the report from calling the export clean.
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
    export_docx_bytes(graph).map_err(map_format_error)
}
