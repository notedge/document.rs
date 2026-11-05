//! XLSX import, IR projection and sheet grid view.
//!
//! Reading produces a format-native [`model::XlsxWorkbook`]. The workbook keeps
//! spreadsheet facts that the semantic IR does not own, such as column widths,
//! fills, borders and image anchors. [`lower`] projects the visible subset into
//! `notedown-ir`, and [`render`] draws the workbook as an A4-paginated grid.

pub mod lower;
pub mod model;
pub mod reader;
pub mod render;

mod reference;
mod styles;
mod value;

pub use lower::{import_xlsx_graph, import_xlsx_graph_bytes, workbook_to_graph, GraphOptions};
pub use model::{
    CellComment, CellFormula, CellRange, CellStyle, CellValue, DateKind, DateSystem, ExcelDate, FormulaKind, FreezePane,
    HorizontalAlign, ImageAnchor, IssueStatus, SheetColumn, SheetImage, SheetRow, SheetVisibility, VerticalAlign,
    WorkbookStyles, XlsxCell, XlsxIssue, XlsxSheet, XlsxStats, XlsxWorkbook,
};
pub use reader::{import_xlsx, import_xlsx_bytes};
pub use render::{render_workbook_html, PageOptions, RenderOptions, XlsxPage, XlsxRender};
