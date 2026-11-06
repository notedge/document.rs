//! Conservative DOCX export from `notedown-ir`.

mod footnotes;
mod numbering;
mod package;
mod page;
mod rels;
mod writer;

use document_ir::{Block, DocumentGraph};

use crate::FormatError;

pub use page::DocxPageSetup;

/// Export a document graph to DOCX bytes with the graph's page setup.
///
/// A graph that records no page setup exports as A4, matching the previous
/// default.
pub fn export_docx_bytes(graph: &DocumentGraph) -> Result<Vec<u8>, FormatError> {
    export_docx_bytes_with_page_setup(graph, &DocxPageSetup::from_graph(graph))
}

/// Export a document graph to DOCX bytes with an explicit page setup.
///
/// The page setup is written into `w:sectPr` and `word/styles.xml`, so page
/// size, margins, columns, body size and line spacing are declared by the
/// package instead of being left to the consumer's defaults.
pub fn export_docx_bytes_with_page_setup(graph: &DocumentGraph, setup: &DocxPageSetup) -> Result<Vec<u8>, FormatError> {
    let rendered = writer::render_document_xml(graph)?;
    let document_xml = page::apply_page_setup(&rendered.document_xml, setup);
    let bytes = package::build_minimal_opc_package(
        &document_xml,
        rendered.include_numbering,
        &rendered.document_rels_xml,
        &rendered.media_parts,
        &rendered.image_extensions,
        rendered.footnotes_xml.as_deref(),
        metadata_xml(graph).as_deref(),
        &page::styles_xml(setup, &diagram_style_ids(graph)),
    );
    let verification = crate::inspect::verify_docx_bytes("export.docx", &bytes, crate::InspectBudget::default())?;
    if !verification.main_part_present {
        return Err(FormatError::parse("docx", "written package is missing word/document.xml"));
    }
    Ok(bytes)
}

/// Paragraph style ids the body references for diagrams and charts.
///
/// The writer carries the diagram/chart language in the paragraph style id, so
/// the styles part has to declare each id the graph actually uses. Declaring
/// them here keeps every `w:pStyle` resolvable instead of relying on Word's
/// silent fallback to `Normal`.
fn diagram_style_ids(graph: &DocumentGraph) -> Vec<String> {
    let mut ids = Vec::new();
    for node in &graph.blocks {
        let id = match &node.block {
            Block::Diagram { language, .. } => Some(format!("Diagram:{language}")),
            Block::Chart { language, .. } => Some(format!("Chart:{language}")),
            _ => None,
        };
        if let Some(id) = id {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

fn metadata_xml(graph: &DocumentGraph) -> Option<String> {
    let metadata = &graph.metadata;
    if metadata.title.is_none() && metadata.language.is_none() && metadata.authors.is_empty() && metadata.tags.is_empty() {
        return None;
    }
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
    );
    if let Some(title) = &metadata.title {
        xml.push_str("<dc:title>");
        xml.push_str(&escape_xml(title));
        xml.push_str("</dc:title>");
    }
    if let Some(author) = metadata.authors.first() {
        xml.push_str("<dc:creator>");
        xml.push_str(&escape_xml(author));
        xml.push_str("</dc:creator>");
    }
    if let Some(language) = &metadata.language {
        xml.push_str("<dc:language>");
        xml.push_str(&escape_xml(language));
        xml.push_str("</dc:language>");
    }
    if !metadata.tags.is_empty() {
        xml.push_str("<cp:keywords>");
        xml.push_str(&escape_xml(&metadata.tags.join(", ")));
        xml.push_str("</cp:keywords>");
    }
    xml.push_str("</cp:coreProperties>");
    Some(xml)
}

fn escape_xml(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// Export a document graph to DOCX bytes on disk.
pub fn export_docx(graph: &DocumentGraph, path: impl AsRef<std::path::Path>) -> Result<(), FormatError> {
    let bytes = export_docx_bytes(graph)?;
    std::fs::write(path.as_ref(), bytes)
        .map_err(|error| FormatError::invalid_input(format!("failed to write {}: {error}", path.as_ref().display())))
}
