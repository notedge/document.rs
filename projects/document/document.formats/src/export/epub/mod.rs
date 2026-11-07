//! EPUB 3 export from `notedown-ir`.
//!
//! Conservative exporter that renders a document graph into a minimal EPUB 3
//! OCF package: `mimetype`, `META-INF/container.xml`, `OEBPS/content.opf`,
//! `OEBPS/nav.xhtml`, and one XHTML chapter per spine group. Image assets with
//! materialized bytes are written under `OEBPS/assets/`.
//!
//! Known semantic gaps (why this is `Partial`):
//!
//! * `Block::Diagram` source is wrapped in a `<pre data-diagram-*>` placeholder
//!   and not converted to an image.
//! * `Block::Math` is emitted as a `<div class="math">`; no MathML output.
//! * `Inline::Citation` and unresolved references return an unsupported error
//!   rather than a graceful fallback.
//! * Opaque blocks beyond thematic breaks fail the export.
//! * No embedded CSS stylesheet — EPUB readers fall back to their default.

mod chapter;
mod container;
mod nav;
mod opf;
mod package;
mod xhtml;

use std::{collections::HashMap, path::Path};

use document_ir::{DocumentGraph, LossMarker, NodeId};

use crate::FormatError;

/// EPUB bytes plus the semantics the export explicitly declares as lost.
///
/// The channel is not a diagnostic sink. Only codes written down here, at a
/// known degradation point, may be pushed. Render and layout diagnostics stay
/// with the logger and never arrive through this path.
///
/// No EPUB code is declared yet: metadata beyond the first author and the
/// table head/foot band flattening are still silent, so the list is empty.
pub struct EpubExport {
    /// The finished OCF package.
    pub bytes: Vec<u8>,
    /// Declared losses, one marker per known degradation.
    pub losses: Vec<LossMarker>,
}

/// Export a document graph to EPUB 3 bytes.
pub fn export_epub_bytes(graph: &DocumentGraph) -> Result<Vec<u8>, FormatError> {
    Ok(export_epub_report(graph)?.bytes)
}

/// Export a document graph to EPUB 3 bytes and report declared losses.
pub fn export_epub_report(graph: &DocumentGraph) -> Result<EpubExport, FormatError> {
    let validation = graph.validate();
    if !validation.is_valid() {
        return Err(FormatError::invalid_input(format!("invalid document graph: {:?}", validation.issues)));
    }

    let chapters = chapter::plan_chapters(graph);
    if chapters.is_empty() {
        return Err(FormatError::unsupported("epub", "document graph has no blocks to export"));
    }
    let assets = chapter::plan_assets(graph);

    let mut opf_items = Vec::with_capacity(chapters.len() + 3);
    let mut spine_items = Vec::with_capacity(chapters.len());
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    // 1. mimetype — MUST be the first STORED entry in the ZIP.
    entries.push(("mimetype".to_string(), b"application/epub+zip".to_vec()));

    // 2. container.xml.
    let container_xml = container::container_xml();
    entries.push(("META-INF/container.xml".to_string(), container_xml.as_bytes().to_vec()));

    // 3. nav.xhtml (EPUB3 navigation document).
    let nav_xhtml = nav::nav_xhtml(graph, &chapters)?;
    entries.push(("OEBPS/nav.xhtml".to_string(), nav_xhtml.as_bytes().to_vec()));
    opf_items.push(opf::OpfManifestItem {
        id: "nav".into(),
        href: "nav.xhtml".into(),
        media_type: "application/xhtml+xml".into(),
        properties: Some("nav".into()),
    });

    // 4. XHTML chapters.
    let mut block_chapters: HashMap<NodeId, String> = HashMap::new();
    for (index, plan) in chapters.iter().enumerate() {
        let href = chapter::chapter_href(index);
        for block_id in &plan.blocks {
            block_chapters.insert(*block_id, href.clone());
            for nested in xhtml::nested_block_ids(graph, *block_id) {
                block_chapters.insert(nested, href.clone());
            }
        }
    }
    for (index, plan) in chapters.iter().enumerate() {
        let href = chapter::chapter_href(index);
        let links = xhtml::ChapterLinks { current: &href, chapter_of: &block_chapters };
        let xhtml = xhtml::chapter_xhtml(graph, plan, &links)?;
        entries.push((format!("OEBPS/{href}"), xhtml.as_bytes().to_vec()));
        let id = format!("ch{index:03}");
        opf_items.push(opf::OpfManifestItem {
            id: id.clone(),
            href: href.clone(),
            media_type: "application/xhtml+xml".into(),
            properties: None,
        });
        spine_items.push(opf::OpfSpineItem { idref: id });
    }

    // 5. Materialized image assets.
    for asset in &assets {
        let href = asset.relative_path.clone();
        let entry_path = format!("OEBPS/{href}");
        entries.push((entry_path, asset.bytes.clone()));
        let id = format!("asset-{}", asset.asset_id.0);
        opf_items.push(opf::OpfManifestItem { id, href, media_type: asset.media_type.clone(), properties: None });
    }

    // 6. content.opf — must come after all members it references are known.
    let opf_xml = opf::content_opf(graph, &opf_items, &spine_items)?;
    entries.push(("OEBPS/content.opf".to_string(), opf_xml.as_bytes().to_vec()));

    Ok(EpubExport { bytes: package::build_epub_package(&entries), losses: Vec::new() })
}

/// Export a document graph to an EPUB file on disk.
pub fn export_epub(graph: &DocumentGraph, path: impl AsRef<Path>) -> Result<(), FormatError> {
    let bytes = export_epub_bytes(graph)?;
    std::fs::write(path.as_ref(), bytes)
        .map_err(|error| FormatError::invalid_input(format!("failed to write {}: {error}", path.as_ref().display())))
}
