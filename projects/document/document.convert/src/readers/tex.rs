use std::path::Path;

use document_formats::import::tex::{import_tex, import_tex_bytes};
use document_ir::DocumentGraph;
use document_types::Result;

use crate::format_error::map_format_error;

/// Reads a TeX source file from disk into `DocumentGraph`.
pub fn read_tex(path: impl AsRef<Path>) -> Result<DocumentGraph> {
    import_tex(path).map_err(map_format_error)
}

/// Reads TeX bytes as UTF-8 into `DocumentGraph`.
pub fn read_tex_bytes(label: impl Into<String>, text: String) -> Result<DocumentGraph> {
    let label = label.into();
    import_tex_bytes(&label, &text).map_err(map_format_error)
}
