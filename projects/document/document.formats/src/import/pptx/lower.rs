//! Projection of the presentation model into `notedown-ir`.
//!
//! Slides become level-one sections, text shapes become paragraphs, tables
//! become tables and pictures become image inlines. Geometry, fills, rotation,
//! placeholder inheritance and notes stay in the native model with loss
//! markers, so the projection never invents a second semantic graph.

use std::{fs, path::Path};

use document_ir::{
    Asset, AssetKind, AttributePair, Attributes, Block, DocumentGraph, DocumentId, DocumentMetadata, IdAllocator, Inline,
    LossMarker, SemanticStatus, TableCell, TableRow,
};

use super::{
    model::{IssueStatus, Paragraph, PptxIssue, PptxPresentation, ShapeKind, TextFrame, codes},
    reader::import_pptx_bytes,
};
use crate::FormatError;

/// Options for the IR projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphOptions {
    /// Whether hidden slides become sections.
    pub include_hidden: bool,
}

impl Default for GraphOptions {
    fn default() -> Self {
        Self { include_hidden: false }
    }
}

/// Imports a PPTX file and projects it into `notedown-ir`.
pub fn import_pptx_graph(path: impl AsRef<Path>) -> Result<DocumentGraph, FormatError> {
    let path = path.as_ref();
    let bytes =
        fs::read(path).map_err(|error| FormatError::invalid_input(format!("failed to read {}: {error}", path.display())))?;
    import_pptx_graph_bytes(&path.display().to_string(), &bytes)
}

/// Imports PPTX bytes and projects them into `notedown-ir`.
pub fn import_pptx_graph_bytes(label: &str, bytes: &[u8]) -> Result<DocumentGraph, FormatError> {
    let presentation = import_pptx_bytes(label, bytes)?;
    Ok(presentation_to_graph(&presentation, &GraphOptions::default()))
}

/// Projects a parsed presentation into `notedown-ir`.
pub fn presentation_to_graph(presentation: &PptxPresentation, options: &GraphOptions) -> DocumentGraph {
    let mut ids = IdAllocator::default();
    let mut graph = DocumentGraph::new(DocumentId(ids.document_id().0));
    graph.set_metadata(DocumentMetadata {
        title: presentation.title.clone().or_else(|| Some(presentation.label.clone())),
        language: None,
        authors: Vec::new(),
        tags: vec!["pptx".to_owned()],
        properties: Vec::new(),
    });
    for issue in &presentation.issues {
        graph.push_loss(to_loss(issue));
    }

    for slide in &presentation.slides {
        if slide.hidden && !options.include_hidden {
            graph.push_loss(LossMarker {
                code: codes::HIDDEN_SLIDE.into(),
                message: format!("slide {} is hidden and is excluded from the IR projection", slide.index),
                status: SemanticStatus::Partial,
            });
            continue;
        }
        let title = slide_title(slide);
        let section_id = graph.push_block(Block::Section {
            level: 1,
            title: vec![Inline::Text { text: title.clone() }],
            children: Vec::new(),
        });
        let mut children = Vec::new();

        for shape in &slide.shapes {
            let is_title = shape.placeholder.as_ref().map(|placeholder| placeholder.kind.is_title()).unwrap_or(false);
            if is_title {
                continue;
            }
            match shape.kind {
                ShapeKind::Shape => {
                    if let Some(text) = &shape.text {
                        for paragraph in &text.paragraphs {
                            let content = paragraph_inlines(paragraph);
                            if content.is_empty() {
                                continue;
                            }
                            children.push(graph.push_block(Block::Paragraph { content }));
                        }
                    }
                }
                ShapeKind::Picture => {
                    if let Some(image) = &shape.image {
                        let asset_id = graph.push_asset(Asset {
                            id: ids.asset_id(),
                            kind: AssetKind::Image,
                            content_identity: None,
                            source: Some(image.part.clone()),
                            media_type: Some(image.media_type.clone()),
                            status: if image.bytes.is_some() { SemanticStatus::Partial } else { SemanticStatus::Unsupported },
                            bytes: image.bytes.clone(),
                        });
                        children.push(graph.push_block(Block::Paragraph {
                            content: vec![Inline::Image {
                                source: image.part.clone(),
                                alt: vec![Inline::Text { text: shape.name.clone() }],
                                title: None,
                                asset: Some(asset_id),
                            }],
                        }));
                        graph.push_loss(LossMarker {
                            code: "import.pptx.image-frame".into(),
                            message: format!(
                                "slide {} picture `{}` keeps no frame geometry in the IR",
                                slide.index, shape.name
                            ),
                            status: SemanticStatus::Partial,
                        });
                    }
                }
                ShapeKind::Table => {
                    if let Some(table) = &shape.table {
                        let rows = table
                            .rows
                            .iter()
                            .map(|row| TableRow {
                                is_header: false,
                                cells: row
                                    .cells
                                    .iter()
                                    .map(|cell| TableCell {
                                        content: cell.text.paragraphs.iter().flat_map(paragraph_inlines).collect(),
                                        colspan: cell.colspan,
                                        rowspan: cell.rowspan,
                                    })
                                    .collect(),
                            })
                            .collect();
                        children.push(graph.push_block(Block::Table { caption: Some(shape.name.clone()), rows }));
                    }
                }
                ShapeKind::Chart => {
                    if let Some(chart) = &shape.chart {
                        let kind = chart.kind.as_deref().unwrap_or("chart");
                        let values = chart.series.iter().map(|entry| entry.values.len()).sum::<usize>();
                        children.push(graph.push_block(Block::Paragraph {
                            content: vec![Inline::Text {
                                text: format!(
                                    "[{kind} chart: {} categories, {} series, {values} values]",
                                    chart.categories.len(),
                                    chart.series_count
                                ),
                            }],
                        }));
                        if !chart.categories.is_empty() || !chart.series.is_empty() {
                            graph.push_loss(LossMarker {
                                code: codes::CHART.into(),
                                message: format!(
                                    "slide {} chart `{}` is projected as text and loses its plot geometry",
                                    slide.index, shape.name
                                ),
                                status: SemanticStatus::Partial,
                            });
                        }
                    }
                }
                ShapeKind::Group | ShapeKind::Other => {}
            }
        }

        if let Some(notes) = &slide.notes {
            children
                .push(graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: format!("Notes: {notes}") }] }));
        }
        if let Some(parent) = graph.blocks.iter_mut().find(|node| node.id == section_id) {
            if let Block::Section { children: owned, .. } = &mut parent.block {
                *owned = children;
            }
        }
    }

    graph.push_loss(LossMarker {
        code: "import.pptx.partial-coverage".into(),
        message: "PPTX import maps slides, layouts, masters, placeholder inheritance, text, shapes, fills, lines, pictures, tables, chart data and notes. Chart visuals, SmartArt, OLE objects, media and group shapes stay refused or projected without their presentation geometry".into(),
        status: SemanticStatus::Partial,
    });
    graph
}

fn slide_title(slide: &super::model::Slide) -> String {
    for shape in &slide.shapes {
        if shape.placeholder.as_ref().map(|placeholder| placeholder.kind.is_title()).unwrap_or(false) {
            if let Some(text) = &shape.text {
                let title = text.text().trim().to_owned();
                if !title.is_empty() {
                    return title;
                }
            }
        }
    }
    format!("Slide {}", slide.index)
}

fn paragraph_inlines(paragraph: &Paragraph) -> Vec<Inline> {
    let mut content = Vec::new();
    for run in &paragraph.runs {
        if run.text.is_empty() {
            continue;
        }
        let mut inline = Inline::Text { text: run.text.clone() };
        for (flag, style) in [
            (run.style.bold, "strong"),
            (run.style.italic, "italic"),
            (run.style.underline, "underline"),
            (run.style.strike, "strike"),
        ] {
            if flag {
                inline = Inline::Styled { style: style.to_owned(), children: vec![inline] };
            }
        }
        let mut pairs = vec![AttributePair { key: "pptx.level".into(), value: paragraph.level.to_string() }];
        if let Some(size) = run.style.size_pt {
            pairs.push(AttributePair { key: "pptx.font-size".into(), value: format!("{size:.1}pt") });
        }
        if let Some(color) = run.style.color.as_deref() {
            pairs.push(AttributePair { key: "pptx.color".into(), value: color.to_owned() });
        }
        if let Some(link) = run.hyperlink.as_deref() {
            pairs.push(AttributePair { key: "pptx.link".into(), value: link.to_owned() });
        }
        content.push(Inline::Attributed {
            attributes: Attributes { id: None, classes: Vec::new(), pairs },
            children: vec![inline],
        });
    }
    content
}

fn to_loss(issue: &PptxIssue) -> LossMarker {
    LossMarker {
        code: issue.code.clone(),
        message: issue.message.clone(),
        status: match issue.status {
            IssueStatus::Partial => SemanticStatus::Partial,
            IssueStatus::Unsupported => SemanticStatus::Unsupported,
        },
    }
}

/// Concatenated text of a frame, used by tests and callers that need a stable
/// plain-text projection.
pub fn text_of(frame: &TextFrame) -> String {
    frame.text()
}
