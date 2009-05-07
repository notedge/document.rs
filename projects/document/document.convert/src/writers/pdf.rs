use document_formats::export::pdf::export_pdf_bytes;
use document_ir::DocumentGraph;
use document_types::Result;

use crate::format_error::map_format_error;

/// Writes `notedown-ir` to PDF bytes.
pub fn write_document_pdf(graph: &DocumentGraph) -> Result<Vec<u8>> {
    export_pdf_bytes(graph).map_err(map_format_error)
}
