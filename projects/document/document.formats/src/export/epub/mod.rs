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

use document_ir::{Block, DocumentGraph, LossMarker, NodeId, SemanticStatus};

use crate::FormatError;

/// EPUB bytes plus the semantics the export explicitly declares as lost.
///
/// The channel is not a diagnostic sink. Only codes written down here, at a
/// known degradation point, may be pushed. Render and layout diagnostics stay
/// with the logger and never arrive through this path.
///
/// Declared so far: creators after the first, tags, typed properties, the
/// table foot band flattening, and image sources that no package member
/// resolves.
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

    Ok(EpubExport { bytes: package::build_epub_package(&entries), losses: declared_losses(graph) })
}

/// Losses this exporter declares explicitly.
///
/// Every entry is a deliberate degradation in `content.opf` or the chapter
/// XHTML. The list is not populated from diagnostics.
fn declared_losses(graph: &DocumentGraph) -> Vec<LossMarker> {
    let mut losses = Vec::new();

    for author in graph.metadata.authors.iter().skip(1) {
        losses.push(LossMarker {
            code: "export.epub.creator_dropped".into(),
            message: format!("author `{author}` is not written because `OEBPS/content.opf` carries only the first creator"),
            status: SemanticStatus::Partial,
        });
    }
    for tag in &graph.metadata.tags {
        losses.push(LossMarker {
            code: "export.epub.tag_dropped".into(),
            message: format!("tag `{tag}` is not written because `OEBPS/content.opf` emits no `dc:subject`"),
            status: SemanticStatus::Partial,
        });
    }
    for property in &graph.metadata.properties {
        losses.push(LossMarker {
            code: "export.epub.property_dropped".into(),
            message: format!(
                "metadata property `{}` is not written because `OEBPS/content.opf` emits only title, language and creator",
                property.key
            ),
            status: SemanticStatus::Partial,
        });
    }

    for node in &graph.blocks {
        let Block::Table { foot, .. } = &node.block
        else {
            continue;
        };
        if foot.is_empty() {
            continue;
        }
        // Header rows survive because the chapter XHTML writes them as `<th>`
        // and the read side rebuilds the leading `<th>` run as the head band.
        // Footer rows are written as `<td>` inside the same `<tr>` sequence,
        // so they come back as body rows and the band is gone for good.
        losses.push(LossMarker {
            code: "export.epub.table_foot_band_flattened".into(),
            message: format!(
                "table `{:?}` keeps {} foot row(s) as body rows because the chapter XHTML emits one `<tr>` sequence without `<tfoot>` and the reader has no foot band",
                node.id,
                foot.len()
            ),
            status: SemanticStatus::Partial,
        });
    }

    for source in unresolved_image_sources(graph) {
        losses.push(LossMarker {
            code: "export.epub.image_asset_unresolved".into(),
            message: format!(
                "image `{source}` has no captured bytes, so `OEBPS/content.opf` carries no asset for it and the chapter keeps a `src` that no package member resolves"
            ),
            status: SemanticStatus::Unresolved,
        });
    }

    losses
}

/// Image sources whose bound asset carries no bytes, in document order.
///
/// `plan_assets` skips every asset without bytes, so an occurrence of such an
/// image keeps its source path in the chapter XHTML even though no package
/// member answers it. Reporting each occurrence is what stops the package from
/// reading as intact.
fn unresolved_image_sources(graph: &DocumentGraph) -> Vec<String> {
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

fn collect_unresolved_images(graph: &DocumentGraph, inlines: &[document_ir::Inline], sources: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            document_ir::Inline::Image { source, alt, asset, .. } => {
                let resolved = asset
                    .and_then(|id| graph.assets.iter().find(|asset| asset.id == id))
                    .and_then(|asset| asset.bytes.as_ref())
                    .is_some();
                if !resolved {
                    sources.push(source.clone());
                }
                collect_unresolved_images(graph, alt, sources);
            }
            document_ir::Inline::Link { display, .. } => collect_unresolved_images(graph, display, sources),
            document_ir::Inline::Styled { children, .. } | document_ir::Inline::Attributed { children, .. } => {
                collect_unresolved_images(graph, children, sources);
            }
            document_ir::Inline::Note { display: Some(display), .. } => {
                collect_unresolved_images(graph, display, sources);
            }
            _ => {}
        }
    }
}

/// Export a document graph to an EPUB file on disk.
pub fn export_epub(graph: &DocumentGraph, path: impl AsRef<Path>) -> Result<(), FormatError> {
    let bytes = export_epub_bytes(graph)?;
    std::fs::write(path.as_ref(), bytes)
        .map_err(|error| FormatError::invalid_input(format!("failed to write {}: {error}", path.as_ref().display())))
}
