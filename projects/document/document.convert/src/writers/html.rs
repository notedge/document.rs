use std::collections::HashMap;

use document_formats::export::html::{RenderedDiagram, export_html_report_with_diagrams};
use document_ir::{DocumentGraph, NodeId};
use document_types::Result;

use crate::format_error::map_format_error;

/// Serializes a document graph to semantic HTML, embedding host-rendered
/// diagram scenes keyed by the authoring block. Diagram blocks without a
/// rendered scene keep the visible source fallback.
///
/// The HTML projection reports every construct it had to degrade, and those
/// markers join the graph loss set so the conversion report never reads as
/// clean while the markup is lossy.
pub fn write_document_html(graph: &mut DocumentGraph, diagrams: &HashMap<NodeId, RenderedDiagram>) -> Result<String> {
    let export = export_html_report_with_diagrams(graph, diagrams).map_err(map_format_error)?;
    for loss in export.losses {
        graph.push_loss(loss);
    }
    Ok(export.html)
}
