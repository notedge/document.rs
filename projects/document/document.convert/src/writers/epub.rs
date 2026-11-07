use document_formats::export::epub::export_epub_report;
use document_ir::DocumentGraph;
use document_types::Result;

use crate::format_error::map_format_error;

/// Write `notedown-ir` to EPUB bytes via `notedown-formats`.
///
/// Declared losses are pushed onto the graph so the report can name them.
pub fn write_document_epub(graph: &mut DocumentGraph) -> Result<Vec<u8>> {
    let export = export_epub_report(graph).map_err(map_format_error)?;
    for loss in export.losses {
        graph.push_loss(loss);
    }
    Ok(export.bytes)
}
