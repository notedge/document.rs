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

/// Authored raw HTML nodes a writer can only keep as literal text.
///
/// A writer without an arbitrary HTML slot still has to keep the authored
/// characters visible, so the occurrence count lets the report name the gap
/// instead of calling the projection clean.
pub(crate) fn raw_html_occurrences(graph: &DocumentGraph) -> usize {
    let mut count = 0;
    for node in &graph.blocks {
        match &node.block {
            Block::Section { title, .. } => count += inline_raw_html_count(title),
            Block::Paragraph { content } => count += inline_raw_html_count(content),
            Block::List { items, .. } => {
                for item in items {
                    count += inline_raw_html_count(&item.content);
                }
            }
            Block::RawHtml { .. } => count += 1,
            _ => {}
        }
    }
    count
}

fn inline_raw_html_count(inlines: &[Inline]) -> usize {
    let mut count = 0;
    for inline in inlines {
        match inline {
            Inline::RawHtml { .. } => count += 1,
            Inline::Link { display, .. } => count += inline_raw_html_count(display),
            Inline::Styled { children, .. } | Inline::Attributed { children, .. } => {
                count += inline_raw_html_count(children);
            }
            Inline::Note { display: Some(display), .. } => count += inline_raw_html_count(display),
            Inline::Citation { display: Some(display), .. } => count += inline_raw_html_count(display),
            _ => {}
        }
    }
    count
}

/// Inline attributes a DOCX run cannot carry.
///
/// A DOCX run maps an authored `id` onto a bookmark and the first class onto a
/// run style. Additional classes and arbitrary pairs have no slot, so their
/// authored names are collected here and named in the report.
pub(crate) fn docx_dropped_inline_attributes(graph: &DocumentGraph) -> Vec<String> {
    let mut dropped = Vec::new();
    for node in &graph.blocks {
        match &node.block {
            Block::Section { title, .. } => collect_docx_attribute_gaps(title, &mut dropped),
            Block::Paragraph { content } => collect_docx_attribute_gaps(content, &mut dropped),
            Block::List { items, .. } => {
                for item in items {
                    collect_docx_attribute_gaps(&item.content, &mut dropped);
                }
            }
            _ => {}
        }
    }
    dropped
}

fn collect_docx_attribute_gaps(inlines: &[Inline], dropped: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            Inline::Attributed { attributes, children } => {
                for pair in &attributes.pairs {
                    dropped.push(format!("pairs:{}", pair.key));
                }
                for class in attributes.classes.iter().skip(1) {
                    dropped.push(format!("classes:{class}"));
                }
                collect_docx_attribute_gaps(children, dropped);
            }
            Inline::Styled { children, .. } | Inline::Link { display: children, .. } => {
                collect_docx_attribute_gaps(children, dropped);
            }
            Inline::Image { alt, .. } => collect_docx_attribute_gaps(alt, dropped),
            Inline::Note { display: Some(display), .. } => collect_docx_attribute_gaps(display, dropped),
            Inline::Citation { display: Some(display), .. } => collect_docx_attribute_gaps(display, dropped),
            _ => {}
        }
    }
}

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
