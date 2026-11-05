use std::path::Path;

use document_formats::import::typst::{import_typst, import_typst_bytes};
use document_ir::DocumentGraph;
use document_types::Result;

use crate::format_error::map_format_error;

/// Reads a Typst source file from disk into `DocumentGraph`.
pub fn read_typst(path: impl AsRef<Path>) -> Result<DocumentGraph> {
    import_typst(path).map_err(map_format_error)
}

/// Reads Typst bytes as UTF-8 into `DocumentGraph`.
pub fn read_typst_bytes(label: impl Into<String>, text: String) -> Result<DocumentGraph> {
    let label = label.into();
    import_typst_bytes(&label, &text).map_err(map_format_error)
}
