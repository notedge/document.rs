//! PDF import via lightweight object and content-stream parsing.

mod content;
mod encoding;
mod fonts;
mod images;
mod layout;
mod metadata;
mod objects;
mod streams;
mod syntax;
#[cfg(test)]
mod tests;
mod types;

#[doc(hidden)]
pub mod test_support;

use std::collections::{BTreeSet, HashMap};

use document_ir::{
    Block, DocumentGraph, DocumentId, Inline, ListItem, LossMarker, NodeId, SemanticStatus, TableCell, TableRow,
};

use crate::{FormatError, import::common::cell_children};

use self::{
    content::{PageSink, extract_text_operators},
    fonts::page_font_maps,
    images::page_images,
    layout::{FlowBlock, FlowList, StyleProfile, page_flow, page_setup},
    metadata::{append_image_asset, pdf_metadata, report_unsupported_pdf_structures},
    objects::{page_content_groups, page_media_boxes, scan_object_bodies},
    streams::stream_contents,
};

/// Import basic page text from a PDF into semantic paragraphs.
pub fn import_pdf_bytes(label: &str, bytes: &[u8]) -> Result<DocumentGraph, FormatError> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(FormatError::invalid_input("input is not a PDF file"));
    }
    let mut graph = DocumentGraph::new(DocumentId(1));
    if let Some(metadata) = pdf_metadata(bytes) {
        graph.metadata = metadata;
    }
    report_unsupported_pdf_structures(bytes, &mut graph);
    let scan = scan_object_bodies(bytes);
    for failure in &scan.failures {
        graph.push_loss(LossMarker {
            code: "import.pdf.object_stream_unreadable".into(),
            message: format!("object stream `{}` could not be expanded: {}", failure.object_id, failure.reason),
            status: SemanticStatus::Unresolved,
        });
    }
    let page_groups = page_content_groups(&scan.bodies);
    let streams = stream_contents(bytes);
    let font_maps = page_font_maps(&scan.bodies, &streams)?;
    let images = page_images(&scan.bodies, &streams)?;
    let use_page_filter = !page_groups.is_empty();
    let page_streams = if use_page_filter {
        page_groups
            .iter()
            .map(|ids| {
                ids.iter().filter_map(|id| streams.iter().find(|stream| stream.object_id == Some(*id))).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    }
    else {
        vec![streams.iter().collect::<Vec<_>>()]
    };
    // Resource names are page-scoped. A single unscoped stream group has no
    // page boundary left, so its resources are merged instead of taking page 0.
    let (page_font_maps, page_images) = if use_page_filter {
        (font_maps, images)
    }
    else {
        (
            vec![font_maps.into_iter().flatten().collect::<HashMap<_, _>>()],
            vec![images.into_iter().flatten().collect::<HashMap<_, _>>()],
        )
    };
    let mut unresolved_fonts = BTreeSet::new();
    let mut spacing_adjustments = false;
    let mut unsupported_inline_images = false;
    let mut pages = Vec::new();
    for (index, streams_in_page) in page_streams.into_iter().enumerate() {
        let mut sink = PageSink::new();
        let empty_fonts = HashMap::new();
        let fonts = page_font_maps.get(index).unwrap_or(&empty_fonts);
        for stream in streams_in_page {
            let decoded = streams::decode_stream(stream.dictionary, stream.data)?;
            let extracted = extract_text_operators(&decoded, fonts, &mut sink);
            unresolved_fonts.extend(extracted.unresolved_fonts);
            spacing_adjustments |= extracted.spacing_adjustments;
            unsupported_inline_images |= extracted.unsupported_inline_images;
        }
        pages.push(sink.finish());
    }
    if spacing_adjustments {
        graph.push_loss(LossMarker {
            code: "import.pdf.text_spacing".into(),
            message: "TJ numeric spacing adjustments narrower than the word-gap threshold are kerning and are not turned into whitespace".into(),
            status: SemanticStatus::Partial,
        });
    }
    if unsupported_inline_images {
        graph.push_loss(LossMarker {
            code: "import.pdf.unsupported_inline_image".into(),
            message: "inline images that are not JPEG payloads are skipped as whole `BI`/`EI` blocks".into(),
            status: SemanticStatus::Unsupported,
        });
    }
    let profile = StyleProfile::from_pages(&pages);
    let page_setup = page_setup(&pages, &profile, &page_media_boxes(&scan.bodies));
    if page_setup.has_values() {
        graph.page_setup = Some(page_setup);
    }
    let mut inferred = InferredStructure::default();
    let empty_images = HashMap::new();
    for (index, page) in pages.iter().enumerate() {
        let images = page_images.get(index).unwrap_or(&empty_images);
        for block in page_flow(page, &profile) {
            emit_flow_block(&mut graph, images, block, &mut inferred);
        }
    }
    for font in unresolved_fonts {
        graph.push_loss(LossMarker {
            code: "import.pdf.font_encoding_unresolved".into(),
            message: format!(
                "font `/{font}` has neither a usable `/ToUnicode` map nor `/WinAnsiEncoding`, text drawn with it is omitted"
            ),
            status: SemanticStatus::Unresolved,
        });
    }
    if graph.blocks.is_empty() {
        graph.push_loss(LossMarker {
            code: "import.pdf.no_extractable_text".into(),
            message: format!("{label}: no supported text operators were found"),
            status: SemanticStatus::Partial,
        });
    }
    if inferred.headings {
        graph.push_loss(LossMarker {
            code: "import.pdf.heading_level_inferred".into(),
            message:
                "heading levels are inferred from font-size rank, the PDF text layer carries no authored heading semantics"
                    .into(),
            status: SemanticStatus::Partial,
        });
    }
    if inferred.lists {
        graph.push_loss(LossMarker {
            code: "import.pdf.list_structure_inferred".into(),
            message: "list items and nesting are inferred from bullet or numeral markers and indentation".into(),
            status: SemanticStatus::Partial,
        });
    }
    if inferred.tables {
        graph.push_loss(LossMarker {
            code: "import.pdf.table_structure_inferred".into(),
            message: "table rows are detected from column gaps, header rows and cell spans are not recoverable".into(),
            status: SemanticStatus::Partial,
        });
    }
    graph.push_loss(LossMarker {
        code: "import.pdf.footnote_semantics".into(),
        message: "the PDF text layer carries no footnote references or bodies, so footnote semantics cannot be linked".into(),
        status: SemanticStatus::Partial,
    });
    graph.push_loss(LossMarker { code: "import.pdf.partial_coverage".into(), message: "PDF import rebuilds paragraphs, headings, lists, tables and image order from glyph geometry and font sizes, but not authored structure, general layout, fonts, indexed or alpha images, annotations, forms, or encryption".into(), status: SemanticStatus::Partial });
    Ok(graph)
}

#[derive(Default)]
struct InferredStructure {
    headings: bool,
    lists: bool,
    tables: bool,
}

fn emit_flow_block(
    graph: &mut DocumentGraph,
    images: &std::collections::HashMap<String, types::PdfImage>,
    block: FlowBlock,
    inferred: &mut InferredStructure,
) {
    match block {
        FlowBlock::Section { level, text } => {
            inferred.headings = true;
            graph.push_block(Block::Section { level, title: vec![Inline::Text { text }], children: Vec::new() });
        }
        FlowBlock::Paragraph { text } => {
            graph.push_block(Block::Paragraph { content: vec![Inline::Text { text }] });
        }
        FlowBlock::List(list) => {
            inferred.lists = true;
            emit_list(graph, list);
        }
        FlowBlock::Table { rows } => {
            inferred.tables = true;
            let body = rows
                .into_iter()
                .map(|cells| TableRow {
                    cells: cells
                        .into_iter()
                        .map(|text| TableCell {
                            children: cell_children(graph, vec![Inline::Text { text }]),
                            colspan: 1,
                            rowspan: 1,
                        })
                        .collect(),
                })
                .collect();
            graph.push_block(Block::Table { caption: None, head: Vec::new(), body, foot: Vec::new() });
        }
        FlowBlock::NamedImage(name) => match images.get(&name) {
            Some(image) => append_image_asset(graph, image),
            None => graph.push_loss(LossMarker {
                code: "import.pdf.unresolved_xobject".into(),
                message: format!("content stream references named XObject `/{name}` that is not a supported image"),
                status: SemanticStatus::Unresolved,
            }),
        },
        FlowBlock::InlineImage(image) => append_image_asset(graph, &image),
    }
}

/// Emit a list after its nested lists, so each item can reference them.
fn emit_list(graph: &mut DocumentGraph, list: FlowList) -> NodeId {
    let mut items = Vec::new();
    for item in list.items {
        let mut children = Vec::new();
        for child in item.children {
            children.push(emit_list(graph, child));
        }
        items.push(ListItem {
            content: vec![Inline::Text { text: item.text }],
            children,
            number: item.number,
            is_task: false,
            is_checked: None,
        });
    }
    graph.push_block(Block::List { ordered: list.ordered, items })
}

/// Import a PDF file from disk.
pub fn import_pdf(path: impl AsRef<std::path::Path>) -> Result<DocumentGraph, FormatError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)
        .map_err(|error| FormatError::invalid_input(format!("failed to read {}: {error}", path.display())))?;
    import_pdf_bytes(&path.display().to_string(), &bytes)
}
