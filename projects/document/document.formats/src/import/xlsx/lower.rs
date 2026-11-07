//! Projection of the workbook model into `notedown-ir`.
//!
//! The projection is deliberately a subset: values become table cells, bold
//! and italic become semantic styles, and spreadsheet-only facts such as
//! column widths, fills, borders and anchors stay in the native model with a
//! loss marker. The reader never invents a second semantic graph.

use std::{collections::BTreeSet, fs, path::Path};

use document_ir::{
    Asset, AssetKind, AttributePair, Attributes, Block, BreakKind, DocumentGraph, DocumentId, DocumentMetadata, IdAllocator,
    Inline, LossMarker, SemanticStatus, TableCell, TableRow,
};

use super::{
    super::common::cell_children,
    model::{CellRange, CellStyle, CellValue, IssueStatus, SheetVisibility, XlsxIssue, XlsxSheet, XlsxWorkbook, codes},
    reader::import_xlsx_bytes,
    value::display_cell,
};
use crate::FormatError;

/// Options for the IR projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphOptions {
    /// Whether hidden sheets become sections.
    pub include_hidden: bool,
}

impl Default for GraphOptions {
    fn default() -> Self {
        Self { include_hidden: false }
    }
}

/// Imports an XLSX file and projects it into `notedown-ir`.
pub fn import_xlsx_graph(path: impl AsRef<Path>) -> Result<DocumentGraph, FormatError> {
    let path = path.as_ref();
    let bytes =
        fs::read(path).map_err(|error| FormatError::invalid_input(format!("failed to read {}: {error}", path.display())))?;
    import_xlsx_graph_bytes(&path.display().to_string(), &bytes)
}

/// Imports XLSX bytes and projects them into `notedown-ir`.
pub fn import_xlsx_graph_bytes(label: &str, bytes: &[u8]) -> Result<DocumentGraph, FormatError> {
    let workbook = import_xlsx_bytes(label, bytes)?;
    Ok(workbook_to_graph(&workbook, &GraphOptions::default()))
}

/// Projects a parsed workbook into `notedown-ir`.
pub fn workbook_to_graph(workbook: &XlsxWorkbook, options: &GraphOptions) -> DocumentGraph {
    let mut ids = IdAllocator::default();
    let mut graph = DocumentGraph::new(DocumentId(ids.document_id().0));
    graph.set_metadata(DocumentMetadata {
        title: Some(workbook.label.clone()),
        language: None,
        authors: Vec::new(),
        tags: vec!["xlsx".to_owned()],
        properties: Vec::new(),
    });

    for issue in &workbook.issues {
        graph.push_loss(to_loss(issue));
    }

    for sheet in &workbook.sheets {
        if sheet.visibility != SheetVisibility::Visible && !options.include_hidden {
            graph.push_loss(LossMarker {
                code: codes::HIDDEN_SHEET.into(),
                message: format!("sheet `{}` is hidden and is excluded from the IR projection", sheet.name),
                status: SemanticStatus::Partial,
            });
            continue;
        }
        let section_id = graph.push_block(Block::Section {
            level: 1,
            title: vec![Inline::Text { text: sheet.name.clone() }],
            children: Vec::new(),
        });
        let mut children = Vec::new();

        if let Some(table_id) = push_sheet_table(&mut graph, sheet, workbook) {
            children.push(table_id);
        }
        for comment in &sheet.comments {
            let text = format!("[{}] {}: {}", comment.reference, comment.author.as_deref().unwrap_or("comment"), comment.text);
            children.push(graph.push_block(Block::Paragraph { content: vec![Inline::Text { text }] }));
        }
        for image in &sheet.images {
            let asset_id = graph.push_asset(Asset {
                id: ids.asset_id(),
                kind: AssetKind::Image,
                content_identity: None,
                source: Some(image.media_path.clone()),
                media_type: image.media_type.clone(),
                status: SemanticStatus::Partial,
                bytes: image.bytes.clone(),
            });
            children.push(graph.push_block(Block::Paragraph {
                content: vec![Inline::Image {
                    source: image.media_path.clone(),
                    alt: vec![Inline::Text {
                        text: format!("Image anchored at {}", anchor_label(image.from.column, image.from.row)),
                    }],
                    title: None,
                    asset: Some(asset_id),
                }],
            }));
            graph.push_loss(LossMarker {
                code: codes::IMAGE_ANCHOR.into(),
                message: format!(
                    "image `{}` is projected after the sheet table because the IR has no cell anchor",
                    image.media_path
                ),
                status: SemanticStatus::Partial,
            });
        }
        if !sheet.comments.is_empty() {
            graph.push_loss(LossMarker {
                code: codes::COMMENT_ANCHOR.into(),
                message: format!("sheet `{}` comments are projected as paragraphs and lose their cell anchor", sheet.name),
                status: SemanticStatus::Partial,
            });
        }
        if let Some(parent) = graph.blocks.iter_mut().find(|node| node.id == section_id) {
            if let Block::Section { children: owned, .. } = &mut parent.block {
                *owned = children;
            }
        }
    }

    graph.push_loss(LossMarker {
        code: "import.xlsx.partial-coverage".into(),
        message: "XLSX import maps sheet values, shared and inline strings, dates, booleans, errors, formulas with cached values, merged ranges, column widths, row heights, frozen panes, styles, comments and anchored images. Charts, conditional formatting, data validation, pivot tables, OLE objects, macros and table objects remain detected-but-refused or projected without their spreadsheet geometry".into(),
        status: SemanticStatus::Partial,
    });
    graph
}

fn push_sheet_table(graph: &mut DocumentGraph, sheet: &XlsxSheet, workbook: &XlsxWorkbook) -> Option<document_ir::NodeId> {
    let range = used_range(sheet)?;
    let has_hidden = (range.start_row..=range.end_row).any(|row| sheet.row_hidden(row))
        || (range.start_column..=range.end_column).any(|column| sheet.column_hidden(column));
    if has_hidden {
        graph.push_loss(LossMarker {
            code: codes::HIDDEN_ROW_COLUMN.into(),
            message: format!("sheet `{}` hidden rows or columns are carried as ordinary table rows", sheet.name),
            status: SemanticStatus::Partial,
        });
    }
    let mut head = Vec::new();
    let mut body = Vec::new();
    for row in range.start_row..=range.end_row {
        let mut cells = Vec::new();
        let mut column = range.start_column;
        while column <= range.end_column {
            if let Some(merge) = sheet.merge_covering(row, column) {
                if merge.start_row == row && merge.start_column == column {
                    cells.push(TableCell {
                        children: cell_children(graph, cell_inlines(sheet, workbook, row, column)),
                        colspan: merge.end_column - merge.start_column + 1,
                        rowspan: merge.end_row - merge.start_row + 1,
                    });
                }
                column += 1;
                continue;
            }
            cells.push(TableCell {
                children: cell_children(graph, cell_inlines(sheet, workbook, row, column)),
                colspan: 1,
                rowspan: 1,
            });
            column += 1;
        }
        let is_header = row == range.start_row
            && (range.start_column..=range.end_column)
                .any(|column| sheet.cell(row, column).map(|cell| cell.style.bold).unwrap_or(false));
        let table_row = TableRow { cells };
        if is_header {
            head.push(table_row);
        }
        else {
            body.push(table_row);
        }
    }
    let caption = if sheet.name.is_empty() { None } else { Some(sheet.name.clone()) };
    Some(graph.push_block(Block::Table { caption, head, body, foot: Vec::new() }))
}

fn used_range(sheet: &XlsxSheet) -> Option<CellRange> {
    let mut last_row = sheet.last_row();
    let mut last_column = sheet.last_column();
    if let Some(dimension) = sheet.dimension {
        last_row = last_row.max(dimension.end_row);
        last_column = last_column.max(dimension.end_column);
    }
    if last_row == 0 || last_column == 0 {
        return None;
    }
    Some(CellRange { start_row: 1, end_row: last_row, start_column: 1, end_column: last_column })
}

fn cell_inlines(sheet: &XlsxSheet, workbook: &XlsxWorkbook, row: u32, column: u32) -> Vec<Inline> {
    let Some(cell) = sheet.cell(row, column)
    else {
        return Vec::new();
    };
    let displayed = display_cell(&cell.value, &cell.style, &workbook.styles);
    let mut content: Vec<Inline> = Vec::new();
    let mut first_line = true;
    for line in displayed.text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if !first_line {
            content.push(Inline::Break { kind: BreakKind::Hard });
        }
        content.push(styled_cell_text(line.to_owned(), &cell.style));
        first_line = false;
    }
    let mut pairs = Vec::new();
    if let Some(formula) = &cell.formula {
        if !formula.source.is_empty() {
            pairs.push(AttributePair { key: "xlsx.formula".into(), value: formula.source.clone() });
        }
    }
    if let Some(format) = cell.style.number_format.as_deref() {
        pairs.push(AttributePair { key: "xlsx.number-format".into(), value: format.to_owned() });
    }
    if let Some(fill) = cell.style.fill_color.as_deref() {
        pairs.push(AttributePair { key: "xlsx.fill".into(), value: fill.to_owned() });
    }
    if let Some(color) = cell.style.font_color.as_deref() {
        pairs.push(AttributePair { key: "xlsx.font-color".into(), value: color.to_owned() });
    }
    if !pairs.is_empty() {
        content =
            vec![Inline::Attributed { attributes: Attributes { id: None, classes: Vec::new(), pairs }, children: content }];
    }
    content
}

/// Wraps one cell text line in the cell-level semantic styles.
fn styled_cell_text(text: String, style: &CellStyle) -> Inline {
    let mut inline = Inline::Text { text };
    if style.bold {
        inline = Inline::Styled { style: "strong".into(), children: vec![inline] };
    }
    if style.italic {
        inline = Inline::Styled { style: "italic".into(), children: vec![inline] };
    }
    if style.underline {
        inline = Inline::Styled { style: "underline".into(), children: vec![inline] };
    }
    if style.strike {
        inline = Inline::Styled { style: "strike".into(), children: vec![inline] };
    }
    inline
}

fn anchor_label(column: u32, row: u32) -> String {
    format!("column {} row {}", column + 1, row + 1)
}

fn to_loss(issue: &XlsxIssue) -> LossMarker {
    LossMarker {
        code: issue.code.clone(),
        message: issue.message.clone(),
        status: match issue.status {
            IssueStatus::Partial => SemanticStatus::Partial,
            IssueStatus::Unsupported => SemanticStatus::Unsupported,
        },
    }
}

/// Distinct sheet names used by tests and callers that need a stable index.
pub fn sheet_names(workbook: &XlsxWorkbook) -> Vec<String> {
    let mut names = BTreeSet::new();
    for sheet in &workbook.sheets {
        names.insert(sheet.name.clone());
    }
    names.into_iter().collect()
}

/// Whether a cell carries a value rather than only a style.
pub fn cell_has_value(cell: &super::model::XlsxCell) -> bool {
    !matches!(cell.value, CellValue::Empty)
}
