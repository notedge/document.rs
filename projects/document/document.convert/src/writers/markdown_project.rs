use document_formats::export::markdown_project::{MarkdownProject, export_markdown_project};
use document_ir::DocumentGraph;
use document_types::Result;

use crate::format_error::map_format_error;

/// Builds a Markdown project payload from `notedown-ir`.
pub fn write_markdown_project(graph: &DocumentGraph) -> Result<MarkdownProject> {
    export_markdown_project(graph).map_err(map_format_error)
}
