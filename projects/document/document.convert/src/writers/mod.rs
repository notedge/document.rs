mod docx;
mod epub;
mod html;
mod markdown;
mod markdown_project;
mod pdf;

pub use docx::write_document_docx;
pub use epub::write_document_epub;
pub use html::write_document_html;
pub use markdown::write_document_markdown;
pub use markdown_project::write_markdown_project;
pub(crate) use pdf::missing_body_faces;
pub use pdf::write_document_pdf;

use document_ir::{Block, DocumentGraph, Inline};

/// Image sources whose bound asset carries no bytes, in document order.
///
/// A writer that cannot embed the raster still has to account for the
/// occurrence, so the export reports exactly the images it degraded instead of
/// leaving the report clean.
pub(crate) fn unresolved_image_sources(graph: &DocumentGraph) -> Vec<String> {
    let mut sources = Vec::new();
    for node in &graph.blocks {
        match &node.block {
            Block::Section { title, .. } => collect_unresolved_images(graph, title, &mut sources),
            Block::Paragraph { content } => collect_unresolved_images(graph, content, &mut sources),
            Block::List { items, .. } => {
                for item in items {
                    collect_unresolved_images(graph, &item.content, &mut sources);
                }
            }
            _ => {}
        }
    }
    sources
}

fn collect_unresolved_images(graph: &DocumentGraph, inlines: &[Inline], sources: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            Inline::Image { source, alt, asset, .. } => {
                let resolved = asset
                    .and_then(|id| graph.assets.iter().find(|asset| asset.id == id))
                    .and_then(|asset| asset.bytes.as_ref())
                    .is_some();
                if !resolved {
                    sources.push(source.clone());
                }
                collect_unresolved_images(graph, alt, sources);
            }
            Inline::Link { display, .. } => collect_unresolved_images(graph, display, sources),
            Inline::Styled { children, .. } | Inline::Attributed { children, .. } => {
                collect_unresolved_images(graph, children, sources);
            }
            Inline::Note { display: Some(display), .. } => collect_unresolved_images(graph, display, sources),
            _ => {}
        }
    }
}
