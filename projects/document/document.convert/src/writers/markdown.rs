use document_formats::export::markdown::export_markdown_report;
use document_ir::DocumentGraph;
use document_types::Result;

use crate::format_error::map_format_error;

/// Serializes a `DocumentGraph` to Markdown text.
///
/// Declared losses are pushed onto the graph so the report can name them.
pub fn write_document_markdown(graph: &mut DocumentGraph) -> Result<String> {
    let export = export_markdown_report(graph).map_err(map_format_error)?;
    for loss in export.losses {
        graph.push_loss(loss);
    }
    Ok(export.text)
}
