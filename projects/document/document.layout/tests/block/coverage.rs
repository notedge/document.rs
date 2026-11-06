//! Layout coverage for non-paragraph blocks and inline images.

use document_ir::{Asset, AssetId, AssetKind, Block, DocumentGraph, DocumentId, Inline, SemanticStatus, TableCell, TableRow};
use document_layout::{
    layout,
    plan::{LayoutItem, LayoutPage, TextRun},
};

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn text_runs(page: &LayoutPage) -> Vec<&TextRun> {
    page.items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::TextRun(run) => Some(run),
            _ => None,
        })
        .collect()
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 2, 0, 0, 0]);
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes
}

#[test]
fn layout_keeps_the_intrinsic_image_aspect_ratio() {
    let mut graph = DocumentGraph::new(DocumentId(5));
    let wide = graph.push_asset(Asset {
        id: AssetId(1),
        kind: AssetKind::Image,
        content_identity: None,
        source: Some("wide.png".to_string()),
        media_type: Some("image/png".to_string()),
        status: SemanticStatus::Partial,
        bytes: Some(png_bytes(640, 320)),
    });
    graph.push_block(Block::Paragraph {
        content: vec![Inline::Image {
            source: "wide.png".to_string(),
            alt: vec![text("Wide figure")],
            title: None,
            asset: Some(wide),
        }],
    });
    graph.push_block(Block::Paragraph {
        content: vec![Inline::Image {
            source: "unknown.png".to_string(),
            alt: vec![text("No payload")],
            title: None,
            asset: None,
        }],
    });

    let plan = layout(&graph);
    let images: Vec<_> = plan
        .pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            LayoutItem::ImagePlaceholder(image) => Some(image),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 2);
    let wide = images[0].width / images[0].height;
    assert!((wide - 2.0).abs() < 1e-6, "640x320 keeps its 2:1 ratio, got {}x{}", images[0].width, images[0].height);
    let fallback = images[1].width / images[1].height;
    assert!((fallback - 1.6).abs() < 1e-6, "a missing payload keeps the 0.625 fallback, got {fallback}");
}

#[test]
fn lays_out_quote_code_table_break_and_image() {
    let mut graph = DocumentGraph::new(DocumentId(4));
    graph.push_block(Block::Quote { content: vec![text("Quoted line")], children: Vec::new() });
    graph.push_block(Block::Code { language: Some("rust".to_string()), content: "fn main() {}".to_string() });
    graph.push_block(Block::Table {
        caption: Some("Table 1".to_string()),
        rows: vec![
            TableRow {
                is_header: true,
                cells: vec![
                    TableCell { content: vec![text("Metric")], colspan: 1, rowspan: 1 },
                    TableCell { content: vec![text("Value")], colspan: 1, rowspan: 1 },
                ],
            },
            TableRow {
                is_header: false,
                cells: vec![
                    TableCell { content: vec![text("Accuracy")], colspan: 1, rowspan: 1 },
                    TableCell { content: vec![text("0.9")], colspan: 1, rowspan: 1 },
                ],
            },
        ],
    });
    graph.push_block(Block::Opaque {
        kind: "thematic_break".to_string(),
        payload_hint: "---".to_string(),
        status: SemanticStatus::Partial,
    });
    graph.push_block(Block::Paragraph {
        content: vec![Inline::Image {
            source: "figure.png".to_string(),
            alt: vec![text("Figure alt")],
            title: None,
            asset: None,
        }],
    });

    let plan = layout(&graph);
    let page = &plan.pages[0];
    let runs = text_runs(page);

    assert!(runs.iter().any(|run| run.text == "Quoted line"));
    assert!(runs.iter().any(|run| run.font_family == "monospace" && run.text == "fn main() {}"));
    assert!(runs.iter().any(|run| run.text == "Metric"));
    assert!(runs.iter().any(|run| run.text == "0.9"));
    assert!(page.items.iter().any(|item| matches!(item, LayoutItem::Rule(_))), "table and thematic break rules");
    assert!(page.items.iter().any(|item| matches!(item, LayoutItem::Rect(rect) if rect.filled)), "code block frame");
    assert!(
        page.items.iter().any(|item| matches!(
            item,
            LayoutItem::ImagePlaceholder(image)
                if image.alt == "Figure alt" && image.source.as_deref() == Some("figure.png")
        )),
        "inline images reserve a placeholder and keep their source"
    );
}
