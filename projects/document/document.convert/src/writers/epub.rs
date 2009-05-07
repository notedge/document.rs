use document_formats::export::epub::export_epub_bytes;
use document_ir::DocumentGraph;
use document_types::Result;

use crate::format_error::map_format_error;

/// Write `notedown-ir` to EPUB bytes via `notedown-formats`.
pub fn write_document_epub(graph: &DocumentGraph) -> Result<Vec<u8>> {
    export_epub_bytes(graph).map_err(map_format_error)
}
