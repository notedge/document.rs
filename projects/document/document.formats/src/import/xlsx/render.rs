//! Sheet grid view with A4 pagination, produced as a self-contained HTML page.
//!
//! The view is a format-native projection: it uses the same workbook model as
//! the reader and never lowers through the semantic IR, so number formats,
//! column widths and frozen panes stay available. Pagination follows print
//! semantics — frozen rows and columns are repeated on every page — and every
//! clamp is reported with a stable code instead of being silently cut.

use super::{
    model::{CellRange, IssueStatus, SheetVisibility, XlsxIssue, XlsxSheet, XlsxWorkbook, codes},
    reference::{column_letters, reference},
    styles::resolve_color,
    value::display_cell,
};
use crate::import::office::escape;

const A4_WIDTH_PX: f64 = 794.0;
const A4_HEIGHT_PX: f64 = 1123.0;
const MARGIN_PX: f64 = 36.0;
const HEADER_HEIGHT_PX: f64 = 20.0;
const ROW_GUTTER_PX: f64 = 36.0;
const DEFAULT_CELL_BUDGET: usize = 12_000;

/// Page geometry for the sheet view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageOptions {
    /// Paper width in CSS pixels at 96 dpi.
    pub width: f64,
    /// Paper height in CSS pixels at 96 dpi.
    pub height: f64,
    /// Page margin in CSS pixels.
    pub margin: f64,
}

impl PageOptions {
    /// A4 portrait at 96 dpi.
    pub fn a4_portrait() -> Self {
        Self { width: A4_WIDTH_PX, height: A4_HEIGHT_PX, margin: MARGIN_PX }
    }

    /// A4 landscape at 96 dpi.
    pub fn a4_landscape() -> Self {
        Self { width: A4_HEIGHT_PX, height: A4_WIDTH_PX, margin: MARGIN_PX }
    }
}

impl Default for PageOptions {
    fn default() -> Self {
        Self::a4_portrait()
    }
}

/// Render options for the workbook view.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    /// Page geometry.
    pub page: PageOptions,
    /// Maximum cells drawn on one page before the page is split again.
    pub cell_budget: usize,
    /// Whether hidden and very hidden sheets are rendered.
    pub include_hidden: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { page: PageOptions::default(), cell_budget: DEFAULT_CELL_BUDGET, include_hidden: false }
    }
}

/// One rendered page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XlsxPage {
    /// Sheet name.
    pub sheet: String,
    /// One-based page number inside the sheet.
    pub number: usize,
    /// Number of pages for the sheet.
    pub total: usize,
    /// First rendered row, including repeated frozen rows.
    pub first_row: u32,
    /// Last rendered row.
    pub last_row: u32,
    /// First rendered column.
    pub first_column: u32,
    /// Last rendered column.
    pub last_column: u32,
    /// Cells drawn on this page.
    pub cell_count: usize,
    /// DOM id used by screenshot tooling.
    pub id: String,
}

/// Result of a workbook sheet view render.
#[derive(Debug, Clone, PartialEq)]
pub struct XlsxRender {
    /// Self-contained HTML document.
    pub html: String,
    /// Page list in document order.
    pub pages: Vec<XlsxPage>,
    /// Non-fatal view issues.
    pub issues: Vec<XlsxIssue>,
}

/// Renders every visible sheet as an A4-paginated grid view.
pub fn render_workbook_html(workbook: &XlsxWorkbook, options: &RenderOptions) -> XlsxRender {
    let mut issues = workbook.issues.clone();
    let mut pages = Vec::new();
    let mut html = String::new();
    html.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><title>");
    html.push_str(&escape(&workbook.label));
    html.push_str("</title><style>");
    html.push_str(STYLE);
    html.push_str("</style></head><body>");
    for sheet in &workbook.sheets {
        if sheet.visibility != SheetVisibility::Visible && !options.include_hidden {
            continue;
        }
        let sheet_pages = render_sheet(sheet, workbook, options, &mut issues, &mut html);
        pages.extend(sheet_pages);
    }
    html.push_str("</body></html>");
    XlsxRender { html, pages, issues }
}

fn render_sheet(
    sheet: &XlsxSheet,
    workbook: &XlsxWorkbook,
    options: &RenderOptions,
    issues: &mut Vec<XlsxIssue>,
    html: &mut String,
) -> Vec<XlsxPage> {
    let max_row = sheet
        .dimension
        .map(|range| range.end_row)
        .unwrap_or_else(|| sheet.last_row())
        .max(sheet.images.iter().map(|image| image.from.row + 1).max().unwrap_or(0))
        .max(1);
    let max_column = sheet
        .dimension
        .map(|range| range.end_column)
        .unwrap_or_else(|| sheet.last_column())
        .max(sheet.images.iter().map(|image| image.from.column + 1).max().unwrap_or(0))
        .max(1);
    let rows: Vec<u32> = (1..=max_row).filter(|row| !sheet.row_hidden(*row)).collect();
    let columns: Vec<u32> = (1..=max_column).filter(|column| !sheet.column_hidden(*column)).collect();
    if rows.is_empty() || columns.is_empty() {
        html.push_str(&format!(
            "<div class=\"page empty\" data-sheet=\"{}\"><div class=\"page-label\">{} · empty sheet</div><p class=\"empty-note\">No visible cells.</p></div>",
            escape(&sheet.name),
            escape(&sheet.name)
        ));
        return vec![XlsxPage {
            sheet: sheet.name.clone(),
            number: 1,
            total: 1,
            first_row: 0,
            last_row: 0,
            first_column: 0,
            last_column: 0,
            cell_count: 0,
            id: format!("sheet-{}-page-1", slug(&sheet.name)),
        }];
    }

    let frozen_rows =
        rows.iter().copied().filter(|row| *row <= sheet.freeze.map(|freeze| freeze.rows).unwrap_or(0)).collect::<Vec<_>>();
    let frozen_columns = columns
        .iter()
        .copied()
        .filter(|column| *column <= sheet.freeze.map(|freeze| freeze.columns).unwrap_or(0))
        .collect::<Vec<_>>();
    let body_rows: Vec<u32> = rows.iter().copied().filter(|row| !frozen_rows.contains(row)).collect();
    let body_columns: Vec<u32> = columns.iter().copied().filter(|column| !frozen_columns.contains(column)).collect();
    if !frozen_rows.is_empty() || !frozen_columns.is_empty() {
        issues.push(issue(
            codes::VIEW_FROZEN_REPEATED,
            format!("sheet `{}` repeats frozen rows and columns on every page", sheet.name),
            IssueStatus::Partial,
            Some(sheet.name.clone()),
            None,
        ));
    }

    let content_width = (options.page.width - options.page.margin * 2.0 - ROW_GUTTER_PX).max(120.0);
    let content_height = (options.page.height - options.page.margin * 2.0 - HEADER_HEIGHT_PX).max(120.0);
    let frozen_width: f64 = frozen_columns.iter().map(|column| column_px(sheet, *column)).sum();
    let frozen_height: f64 = frozen_rows.iter().map(|row| row_px(sheet, *row)).sum();
    let body_width = (content_width - frozen_width).max(120.0);
    let body_height = (content_height - frozen_height).max(120.0);

    let column_groups = group_columns(sheet, &body_columns, body_width, &sheet.name, issues);
    let row_groups = group_rows(sheet, &body_rows, body_height, &sheet.name, issues);

    // Split row groups again when one page would exceed the cell budget.
    let mut jobs: Vec<(Vec<u32>, Vec<u32>)> = Vec::new();
    for columns_group in &column_groups {
        let total_columns = frozen_columns.len() + columns_group.len();
        let allowed_rows = if total_columns == 0 { usize::MAX } else { (options.cell_budget / total_columns).max(1) };
        let allowed_body = allowed_rows.saturating_sub(frozen_rows.len()).max(1);
        for rows_group in &row_groups {
            for chunk in rows_group.chunks(allowed_body) {
                jobs.push((chunk.to_vec(), columns_group.clone()));
            }
        }
    }
    if jobs.is_empty() {
        jobs.push((Vec::new(), Vec::new()));
    }

    let total = jobs.len();
    let mut pages = Vec::new();
    for (job_index, (body_row_group, body_column_group)) in jobs.iter().enumerate() {
        let page_number = job_index + 1;
        let mut page_rows = frozen_rows.clone();
        page_rows.extend(body_row_group.iter().copied());
        let mut page_columns = frozen_columns.clone();
        page_columns.extend(body_column_group.iter().copied());
        let first_row = page_rows.first().copied().unwrap_or(0);
        let last_row = page_rows.last().copied().unwrap_or(0);
        let first_column = page_columns.first().copied().unwrap_or(0);
        let last_column = page_columns.last().copied().unwrap_or(0);
        let id = format!("sheet-{}-page-{page_number}", slug(&sheet.name));
        html.push_str(&format!(
            "<div class=\"page\" id=\"{}\" data-sheet=\"{}\" data-page=\"{}\" data-page-count=\"{}\" data-range=\"{}:{}\" style=\"width:{:.0}px;height:{:.0}px;padding:{:.0}px\">",
            id,
            escape(&sheet.name),
            page_number,
            total,
            reference(first_row, first_column),
            reference(last_row, last_column),
            options.page.width,
            options.page.height,
            options.page.margin
        ));
        html.push_str(&format!(
            "<div class=\"page-label\">{} · page {}/{} · {}:{}</div>",
            escape(&sheet.name),
            page_number,
            total,
            reference(first_row, first_column),
            reference(last_row, last_column)
        ));
        html.push_str("<table class=\"grid\"><colgroup><col style=\"width:");
        html.push_str(&format!("{ROW_GUTTER_PX}px\">"));
        for column in &page_columns {
            html.push_str(&format!(
                "<col style=\"width:{:.1}px\">",
                column_px(sheet, *column).min(body_width.max(content_width))
            ));
        }
        html.push_str("</colgroup><thead><tr><th class=\"gutter\"></th>");
        for column in &page_columns {
            html.push_str(&format!("<th class=\"colhead\">{}</th>", column_letters(*column)));
        }
        html.push_str("</tr></thead><tbody>");
        for row in &page_rows {
            html.push_str(&format!("<tr style=\"height:{:.1}px\">", row_px(sheet, *row).min(content_height)));
            html.push_str(&format!("<th class=\"rowhead\">{row}</th>"));
            let mut column_index = 0usize;
            while column_index < page_columns.len() {
                let column = page_columns[column_index];
                if let Some(merge) = sheet.merge_covering(*row, column) {
                    if merge.start_row == *row && merge.start_column == column {
                        let span_rows = contiguous_span(&page_rows, *row, merge.end_row);
                        let span_columns = contiguous_span(&page_columns, column, merge.end_column);
                        let clamped = span_rows < (merge.end_row - merge.start_row + 1) as usize
                            || span_columns < (merge.end_column - merge.start_column + 1) as usize;
                        if clamped {
                            issues.push(issue(
                                codes::VIEW_MERGE_SPLIT,
                                format!(
                                    "sheet `{}` merge {}:{} is clipped at page {}",
                                    sheet.name,
                                    reference(merge.start_row, merge.start_column),
                                    reference(merge.end_row, merge.end_column),
                                    page_number
                                ),
                                IssueStatus::Partial,
                                Some(sheet.name.clone()),
                                Some(reference(merge.start_row, merge.start_column)),
                            ));
                        }
                        html.push_str("<td");
                        if span_columns > 1 {
                            html.push_str(&format!(" colspan=\"{span_columns}\""));
                        }
                        if span_rows > 1 {
                            html.push_str(&format!(" rowspan=\"{span_rows}\""));
                        }
                        html.push_str(&format!(" style=\"{}\">", cell_css(sheet, workbook, *row, column, issues)));
                        append_cell(html, sheet, workbook, *row, column, issues);
                        html.push_str("</td>");
                        column_index += span_columns;
                        continue;
                    }
                    // Covered by a merge whose origin is not on this page fragment.
                    html.push_str("<td class=\"covered\"></td>");
                    column_index += 1;
                    continue;
                }
                html.push_str(&format!("<td style=\"{}\">", cell_css(sheet, workbook, *row, column, issues)));
                append_cell(html, sheet, workbook, *row, column, issues);
                html.push_str("</td>");
                column_index += 1;
            }
            html.push_str("</tr>");
        }
        html.push_str("</tbody></table>");
        append_images(html, sheet, &page_rows, &page_columns);
        html.push_str("</div>");
        pages.push(XlsxPage {
            sheet: sheet.name.clone(),
            number: page_number,
            total,
            first_row,
            last_row,
            first_column,
            last_column,
            cell_count: page_rows.len() * page_columns.len(),
            id,
        });
    }
    pages
}

fn group_columns(sheet: &XlsxSheet, columns: &[u32], budget: f64, name: &str, issues: &mut Vec<XlsxIssue>) -> Vec<Vec<u32>> {
    if columns.is_empty() {
        return vec![Vec::new()];
    }
    let mut groups = Vec::new();
    let mut current = Vec::new();
    let mut width = 0.0;
    for column in columns {
        let column_width = column_px(sheet, *column);
        if current.is_empty() && column_width > budget {
            issues.push(issue(
                codes::VIEW_COLUMN_CLAMPED,
                format!(
                    "sheet `{name}` column {} is {column_width:.0}px wide and is clamped to the {budget:.0}px page",
                    column_letters(*column)
                ),
                IssueStatus::Partial,
                Some(name.to_owned()),
                None,
            ));
        }
        if !current.is_empty() && width + column_width > budget {
            groups.push(std::mem::take(&mut current));
            width = 0.0;
        }
        width += column_width;
        current.push(*column);
    }
    if !current.is_empty() {
        groups.push(current);
    }
    if groups.is_empty() {
        groups.push(Vec::new());
    }
    groups
}

fn group_rows(sheet: &XlsxSheet, rows: &[u32], budget: f64, name: &str, issues: &mut Vec<XlsxIssue>) -> Vec<Vec<u32>> {
    if rows.is_empty() {
        return vec![Vec::new()];
    }
    let mut groups = Vec::new();
    let mut current = Vec::new();
    let mut height = 0.0;
    for row in rows {
        let row_height = row_px(sheet, *row);
        if current.is_empty() && row_height > budget {
            issues.push(issue(
                codes::VIEW_ROW_CLAMPED,
                format!("sheet `{name}` row {row} is {row_height:.0}px tall and is clamped to the {budget:.0}px page"),
                IssueStatus::Partial,
                Some(name.to_owned()),
                None,
            ));
        }
        if !current.is_empty() && height + row_height > budget {
            groups.push(std::mem::take(&mut current));
            height = 0.0;
        }
        height += row_height;
        current.push(*row);
    }
    if !current.is_empty() {
        groups.push(current);
    }
    if groups.is_empty() {
        groups.push(Vec::new());
    }
    groups
}

fn contiguous_span(values: &[u32], start: u32, end: u32) -> usize {
    let mut span = 0usize;
    for value in values.iter().copied().skip_while(|value| *value < start) {
        if value > end {
            break;
        }
        if value != start + span as u32 {
            break;
        }
        span += 1;
    }
    span.max(1)
}

fn column_px(sheet: &XlsxSheet, column: u32) -> f64 {
    (sheet.column_width(column) * 7.0 + 5.0).max(12.0)
}

fn row_px(sheet: &XlsxSheet, row: u32) -> f64 {
    (sheet.row_height(row) * 96.0 / 72.0).max(8.0)
}

fn cell_css(sheet: &XlsxSheet, workbook: &XlsxWorkbook, row: u32, column: u32, issues: &mut Vec<XlsxIssue>) -> String {
    let style = sheet.cell(row, column).map(|cell| cell.style.clone()).unwrap_or_default();
    let mut css = String::new();
    if style.bold {
        css.push_str("font-weight:700;");
    }
    if style.italic {
        css.push_str("font-style:italic;");
    }
    if style.underline {
        css.push_str("text-decoration:underline;");
    }
    if let Some(name) = style.font_name.as_deref() {
        css.push_str(&format!("font-family:'{}',Calibri,Arial,sans-serif;", name.replace('\'', "")));
    }
    if let Some(size) = style.font_size {
        css.push_str(&format!("font-size:{size}pt;"));
    }
    if let Some(color) = style.font_color.as_deref().and_then(|color| resolve_color(color, &workbook.styles)) {
        css.push_str(&format!("color:{color};"));
    }
    else if style.font_color.as_deref().is_some_and(|color| color.starts_with("theme:")) {
        issues.push(issue(
            codes::THEME_COLOR,
            format!("sheet `{}` cell {} uses an unresolved theme font color", sheet.name, reference(row, column)),
            IssueStatus::Partial,
            Some(sheet.name.clone()),
            Some(reference(row, column)),
        ));
    }
    if let Some(fill) = style.fill_color.as_deref().and_then(|color| resolve_color(color, &workbook.styles)) {
        css.push_str(&format!("background:{fill};"));
    }
    css.push_str(match style.horizontal {
        super::model::HorizontalAlign::Left | super::model::HorizontalAlign::Fill => "text-align:left;",
        super::model::HorizontalAlign::Center => "text-align:center;",
        super::model::HorizontalAlign::Right => "text-align:right;",
        super::model::HorizontalAlign::Justify | super::model::HorizontalAlign::Distributed => "text-align:justify;",
        super::model::HorizontalAlign::General => "",
    });
    css.push_str(match style.vertical {
        super::model::VerticalAlign::Top => "vertical-align:top;",
        super::model::VerticalAlign::Center => "vertical-align:middle;",
        _ => "vertical-align:bottom;",
    });
    if style.wrap_text {
        css.push_str("white-space:normal;word-break:break-word;");
    }
    else {
        css.push_str("white-space:nowrap;overflow:hidden;");
    }
    for (side, edge) in [
        ("top", &style.borders.top),
        ("right", &style.borders.right),
        ("bottom", &style.borders.bottom),
        ("left", &style.borders.left),
    ] {
        if let Some(edge) = edge {
            let width = match edge.style.as_str() {
                "medium" => "2px",
                "thick" => "3px",
                _ => "1px",
            };
            let line = match edge.style.as_str() {
                "dotted" => "dotted",
                "dashed" => "dashed",
                "double" => "double",
                _ => "solid",
            };
            let color = edge
                .color
                .as_deref()
                .and_then(|color| resolve_color(color, &workbook.styles))
                .unwrap_or_else(|| "#808080".to_owned());
            css.push_str(&format!("border-{side}:{width} {line} {color};"));
        }
    }
    css
}

fn append_cell(
    html: &mut String,
    sheet: &XlsxSheet,
    workbook: &XlsxWorkbook,
    row: u32,
    column: u32,
    issues: &mut Vec<XlsxIssue>,
) {
    let Some(cell) = sheet.cell(row, column)
    else {
        return;
    };
    if cell.formula.is_some() && !cell.value_is_cached() {
        html.push_str("<span class=\"formula-mark\">ƒ</span>");
    }
    let displayed = display_cell(&cell.value, &cell.style, &workbook.styles);
    if displayed.approximated {
        issues.push(issue(
            codes::NUMBER_FORMAT,
            format!("sheet `{}` cell {} uses a number format that is approximated", sheet.name, cell.reference),
            IssueStatus::Partial,
            Some(sheet.name.clone()),
            Some(cell.reference.clone()),
        ));
    }
    html.push_str(&escape(&displayed.text));
}

fn append_images(html: &mut String, sheet: &XlsxSheet, page_rows: &[u32], page_columns: &[u32]) {
    if sheet.images.is_empty() {
        return;
    }
    html.push_str("<div class=\"image-layer\">");
    for image in &sheet.images {
        let column = image.from.column + 1;
        let row = image.from.row + 1;
        if !page_columns.contains(&column) || !page_rows.contains(&row) {
            continue;
        }
        let mut x = ROW_GUTTER_PX;
        for candidate in page_columns {
            if *candidate >= column {
                break;
            }
            x += column_px(sheet, *candidate);
        }
        x += image.from.column_offset as f64 / 9525.0;
        let mut y = HEADER_HEIGHT_PX;
        for candidate in page_rows {
            if *candidate >= row {
                break;
            }
            y += row_px(sheet, *candidate);
        }
        y += image.from.row_offset as f64 / 9525.0;
        let (width, height) = image_size(image);
        let (Some(bytes), Some(media_type)) = (image.bytes.as_deref(), image.media_type.as_deref())
        else {
            continue;
        };
        let source = format!("data:{media_type};base64,{}", base64(bytes));
        html.push_str(&format!(
            "<img src=\"{}\" alt=\"anchored image\" style=\"position:absolute;left:{x:.1}px;top:{y:.1}px;width:{width:.1}px;height:{height:.1}px\">",
            escape(&source)
        ));
    }
    html.push_str("</div>");
}

fn image_size(image: &super::model::SheetImage) -> (f64, f64) {
    if let Some(to) = image.to {
        let width = (to.column.saturating_sub(image.from.column) as f64 * 64.0)
            + (to.column_offset - image.from.column_offset) as f64 / 9525.0;
        let height =
            (to.row.saturating_sub(image.from.row) as f64 * 20.0) + (to.row_offset - image.from.row_offset) as f64 / 9525.0;
        if width > 4.0 && height > 4.0 {
            return (width, height);
        }
    }
    image.extent.map(|(cx, cy)| (cx as f64 / 9525.0, cy as f64 / 9525.0)).unwrap_or((96.0, 48.0))
}

fn slug(value: &str) -> String {
    let mut slug = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        }
        else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len() * 2);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(TABLE[(first >> 2) as usize] as char);
        output.push(TABLE[(((first & 0b11) << 4) | (second >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 { TABLE[(((second & 0b1111) << 2) | (third >> 6)) as usize] as char } else { '=' });
        output.push(if chunk.len() > 2 { TABLE[(third & 0b111111) as usize] as char } else { '=' });
    }
    output
}

fn issue(
    code: &str,
    message: impl Into<String>,
    status: IssueStatus,
    sheet: Option<String>,
    cell: Option<String>,
) -> XlsxIssue {
    XlsxIssue { code: code.to_owned(), message: message.into(), status, sheet, cell }
}

/// Re-exported range helper used by tests to describe page ranges.
pub fn page_range(page: &XlsxPage) -> Option<CellRange> {
    (page.first_row > 0 && page.first_column > 0).then(|| CellRange {
        start_row: page.first_row,
        end_row: page.last_row,
        start_column: page.first_column,
        end_column: page.last_column,
    })
}

trait CachedValue {
    fn value_is_cached(&self) -> bool;
}

impl CachedValue for super::model::XlsxCell {
    fn value_is_cached(&self) -> bool {
        !matches!(self.value, super::model::CellValue::Empty)
    }
}

const STYLE: &str = r#"
@page { size: A4; margin: 0; }
html, body { margin: 0; padding: 0; background: #ffffff; }
body { font-family: Calibri, "Segoe UI", Arial, sans-serif; color: #000000; }
.page { position: relative; box-sizing: border-box; page-break-after: always; overflow: hidden; background: #ffffff; }
.page-label { font: 9px/1.4 "Segoe UI", Arial, sans-serif; color: #5f6368; margin: 0 0 6px 0; letter-spacing: .02em; }
table.grid { border-collapse: collapse; table-layout: fixed; }
table.grid th.gutter { width: 36px; }
table.grid th.colhead, table.grid th.rowhead { background: #f1f3f4; color: #5f6368; font: 9px/1.2 "Segoe UI", Arial, sans-serif; font-weight: 400; border: 1px solid #d0d4d8; text-align: center; }
table.grid td { border: 1px solid #d0d4d8; padding: 1px 3px; font-size: 11pt; overflow: hidden; }
table.grid td.covered { background: #ffffff; }
.formula-mark { float: right; color: #1a73e8; font-size: 8px; margin-left: 2px; }
.empty-note { color: #5f6368; font-size: 12px; }
.image-layer { position: absolute; left: 36px; top: 36px; right: 36px; bottom: 36px; }
"#;
