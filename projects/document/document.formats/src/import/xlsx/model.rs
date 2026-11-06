//! Format-native read model for an XLSX workbook.
//!
//! The model carries the facts a spreadsheet owns — sheets, cells, formulas,
//! styles, merges, comments and anchored images. It is not a second semantic
//! IR: [`lower`](super::lower) projects the visible subset into
//! `document-ir` for the conversion chain, while the model keeps the geometry
//! and styling that a table grid view needs.

use std::collections::BTreeMap;

/// Workbook date epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSystem {
    /// The 1900 date system used by Windows Excel.
    Date1900,
    /// The 1904 date system used by legacy Mac Excel.
    Date1904,
}

/// Sheet visibility recorded by `workbook.xml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetVisibility {
    /// Normal visible sheet.
    Visible,
    /// Hidden by the author but still part of the workbook.
    Hidden,
    /// Hidden and excluded from the sheet tab context menu.
    VeryHidden,
}

/// One resolved cell value.
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    /// No value was authored.
    Empty,
    /// Numeric value without a date interpretation.
    Number(f64),
    /// Numeric serial interpreted as a date or time.
    Date(ExcelDate),
    /// Boolean value.
    Boolean(bool),
    /// Shared or inline string value.
    Text(String),
    /// Shared string table index plus resolved text.
    SharedString {
        /// Zero-based shared string index.
        index: u32,
        /// Resolved string payload.
        text: String,
    },
    /// Spreadsheet error value such as `#N/A` or `#DIV/0!`.
    Error {
        /// Authored error literal.
        code: String,
    },
}

/// Date or time derived from an Excel serial number.
#[derive(Debug, Clone, PartialEq)]
pub struct ExcelDate {
    /// Serial number exactly as stored in the cell.
    pub serial: f64,
    /// ISO-8601 `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SS` rendering.
    pub iso: String,
    /// Whether the format expressed a date, a time or both.
    pub kind: DateKind,
}

/// Which components a date number format exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateKind {
    /// Date only.
    Date,
    /// Time only.
    Time,
    /// Date and time.
    DateTime,
}

/// Formula stored on a cell.
#[derive(Debug, Clone, PartialEq)]
pub struct CellFormula {
    /// Formula source without the leading `=`.
    pub source: String,
    /// Shared or array formula kind when the workbook declares one.
    pub kind: FormulaKind,
    /// Cached value stored by the writer, when present.
    pub cached: Option<Box<CellValue>>,
}

/// Formula storage kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormulaKind {
    /// Ordinary per-cell formula.
    Normal,
    /// Master cell of a shared formula.
    SharedMaster {
        /// Shared formula index.
        index: u32,
        /// Range the shared formula applies to.
        range: Option<String>,
    },
    /// Cell covered by a shared formula master.
    SharedChild {
        /// Shared formula index.
        index: u32,
    },
    /// Array formula.
    Array {
        /// Range the array formula applies to.
        range: Option<String>,
    },
}

/// Horizontal alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HorizontalAlign {
    /// General or unset alignment.
    #[default]
    General,
    /// Left aligned.
    Left,
    /// Center aligned.
    Center,
    /// Right aligned.
    Right,
    /// Justified.
    Justify,
    /// Distributed.
    Distributed,
    /// Fill continues the cell value across the span.
    Fill,
}

/// Vertical alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VerticalAlign {
    /// Top.
    Top,
    /// Center.
    Center,
    /// Bottom.
    #[default]
    Bottom,
    /// Justified.
    Justify,
    /// Distributed.
    Distributed,
}

/// One border edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorderEdge {
    /// Office border style name such as `thin` or `medium`.
    pub style: String,
    /// ARGB color when resolved.
    pub color: Option<String>,
}

/// Four cell border edges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BorderEdges {
    /// Top edge.
    pub top: Option<BorderEdge>,
    /// Right edge.
    pub right: Option<BorderEdge>,
    /// Bottom edge.
    pub bottom: Option<BorderEdge>,
    /// Left edge.
    pub left: Option<BorderEdge>,
}

impl BorderEdges {
    /// Whether any edge carries a visible border.
    pub fn is_empty(&self) -> bool {
        self.top.is_none() && self.right.is_none() && self.bottom.is_none() && self.left.is_none()
    }
}

/// Resolved visual style of one cell.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellStyle {
    /// Bold font.
    pub bold: bool,
    /// Italic font.
    pub italic: bool,
    /// Underlined font.
    pub underline: bool,
    /// Struck-through font.
    pub strike: bool,
    /// Font family name.
    pub font_name: Option<String>,
    /// Font size in points.
    pub font_size: Option<f32>,
    /// Font color as ARGB or `rgb()`.
    pub font_color: Option<String>,
    /// Solid fill background color.
    pub fill_color: Option<String>,
    /// Built-in or custom number format id.
    pub number_format_id: u32,
    /// Number format code.
    pub number_format: Option<String>,
    /// Horizontal alignment.
    pub horizontal: HorizontalAlign,
    /// Vertical alignment.
    pub vertical: VerticalAlign,
    /// Whether the cell wraps text.
    pub wrap_text: bool,
    /// Border edges.
    pub borders: BorderEdges,
}

impl CellStyle {
    /// A stable key for CSS class deduplication and style comparison.
    pub fn key(&self) -> String {
        format!(
            "b{}i{}u{}s{}f{:?}{:?}n{:?}{:?}a{:?}{:?}w{}b{:?}",
            self.bold as u8,
            self.italic as u8,
            self.underline as u8,
            self.strike as u8,
            self.font_name,
            self.font_size,
            self.number_format_id,
            self.fill_color,
            self.horizontal,
            self.vertical,
            self.wrap_text as u8,
            self.borders
        )
    }
}

/// One cell in a sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct XlsxCell {
    /// A1 reference such as `B4`.
    pub reference: String,
    /// One-based row index.
    pub row: u32,
    /// One-based column index.
    pub column: u32,
    /// Stored value.
    pub value: CellValue,
    /// Formula details when the cell is a formula cell.
    pub formula: Option<CellFormula>,
    /// Raw style index from the `s` attribute.
    pub style_index: Option<u32>,
    /// Resolved style.
    pub style: CellStyle,
}

/// Half-open or inclusive cell range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRange {
    /// One-based first row.
    pub start_row: u32,
    /// One-based last row.
    pub end_row: u32,
    /// One-based first column.
    pub start_column: u32,
    /// One-based last column.
    pub end_column: u32,
}

impl CellRange {
    /// Number of rows covered by the range.
    pub fn rows(&self) -> u32 {
        self.end_row.saturating_sub(self.start_row) + 1
    }

    /// Number of columns covered by the range.
    pub fn columns(&self) -> u32 {
        self.end_column.saturating_sub(self.start_column) + 1
    }
}

/// Column metadata from `<cols>`.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetColumn {
    /// First column covered by this record.
    pub min: u32,
    /// Last column covered by this record.
    pub max: u32,
    /// Width in Excel character units.
    pub width: f64,
    /// Whether the author set an explicit width.
    pub custom: bool,
    /// Whether the column is hidden.
    pub hidden: bool,
}

/// Row metadata from `<row>`.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetRow {
    /// One-based row index.
    pub index: u32,
    /// Height in points.
    pub height: Option<f64>,
    /// Whether the height is custom.
    pub custom_height: bool,
    /// Whether the row is hidden.
    pub hidden: bool,
}

/// Frozen pane state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreezePane {
    /// Number of frozen columns.
    pub columns: u32,
    /// Number of frozen rows.
    pub rows: u32,
}

/// Cell anchor used by drawings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageAnchor {
    /// Zero-based column index.
    pub column: u32,
    /// EMU offset inside the column.
    pub column_offset: i64,
    /// Zero-based row index.
    pub row: u32,
    /// EMU offset inside the row.
    pub row_offset: i64,
}

/// Image anchored to a sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetImage {
    /// Start anchor.
    pub from: ImageAnchor,
    /// End anchor for two-cell anchors.
    pub to: Option<ImageAnchor>,
    /// Package-relative media path.
    pub media_path: String,
    /// Resolved media type.
    pub media_type: Option<String>,
    /// Explicit extent in EMU for one-cell anchors.
    pub extent: Option<(i64, i64)>,
    /// Raw bytes when the package could supply them.
    pub bytes: Option<Vec<u8>>,
}

/// Comment on one cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellComment {
    /// Cell reference.
    pub reference: String,
    /// Author display name.
    pub author: Option<String>,
    /// Comment body.
    pub text: String,
}

/// One worksheet.
#[derive(Debug, Clone, PartialEq)]
pub struct XlsxSheet {
    /// Sheet name.
    pub name: String,
    /// Zero-based workbook order.
    pub index: u32,
    /// Visibility state.
    pub visibility: SheetVisibility,
    /// Declared dimension when the writer stored one.
    pub dimension: Option<CellRange>,
    /// Column metadata.
    pub columns: Vec<SheetColumn>,
    /// Row metadata.
    pub rows: Vec<SheetRow>,
    /// Sparse cells sorted by row and column.
    pub cells: Vec<XlsxCell>,
    /// Merged ranges.
    pub merged: Vec<CellRange>,
    /// Frozen pane.
    pub freeze: Option<FreezePane>,
    /// Anchored images.
    pub images: Vec<SheetImage>,
    /// Cell comments.
    pub comments: Vec<CellComment>,
    /// Whether the sheet declares an autofilter range.
    pub auto_filter: Option<CellRange>,
    /// Whether conditional formatting rules are present.
    pub conditional_formatting: bool,
    /// Whether data validations are present.
    pub data_validations: bool,
    /// Whether the sheet carries a drawing with chart frames.
    pub charts: u32,
    /// Whether the sheet carries embedded OLE objects.
    pub embedded_objects: u32,
    /// Whether the sheet carries a legacy macro sheet payload.
    pub legacy_macro: bool,
    /// Default row height in points declared by `sheetFormatPr`.
    pub default_row_height: f64,
    /// Default column width in character units declared by `sheetFormatPr`.
    pub default_column_width: f64,
}

impl XlsxSheet {
    /// Returns the cell at a one-based row and column.
    pub fn cell(&self, row: u32, column: u32) -> Option<&XlsxCell> {
        self.cells.binary_search_by_key(&(row, column), |cell| (cell.row, cell.column)).ok().map(|index| &self.cells[index])
    }

    /// Largest row index that carries a cell or row metadata.
    pub fn last_row(&self) -> u32 {
        self.cells.iter().map(|cell| cell.row).chain(self.rows.iter().map(|row| row.index)).max().unwrap_or(0)
    }

    /// Largest column index that carries a cell or column metadata.
    pub fn last_column(&self) -> u32 {
        self.cells.iter().map(|cell| cell.column).chain(self.columns.iter().map(|column| column.max)).max().unwrap_or(0)
    }

    /// Width of a column in Excel character units, falling back to the default.
    pub fn column_width(&self, column: u32) -> f64 {
        self.columns
            .iter()
            .rev()
            .find(|record| record.min <= column && column <= record.max)
            .map(|record| record.width)
            .unwrap_or(self.default_column_width)
    }

    /// Height of a row in points, falling back to the default.
    pub fn row_height(&self, row: u32) -> f64 {
        self.rows
            .iter()
            .rev()
            .find(|record| record.index == row)
            .and_then(|record| record.height)
            .unwrap_or(self.default_row_height)
    }

    /// Whether a row is hidden.
    pub fn row_hidden(&self, row: u32) -> bool {
        self.rows.iter().find(|record| record.index == row).map(|record| record.hidden).unwrap_or(false)
    }

    /// Whether a column is hidden.
    pub fn column_hidden(&self, column: u32) -> bool {
        self.columns
            .iter()
            .rev()
            .find(|record| record.min <= column && column <= record.max)
            .map(|record| record.hidden)
            .unwrap_or(false)
    }

    /// Merge range that starts at the given cell.
    pub fn merge_starting_at(&self, row: u32, column: u32) -> Option<CellRange> {
        self.merged.iter().copied().find(|range| range.start_row == row && range.start_column == column)
    }

    /// Merge range that covers the given cell.
    pub fn merge_covering(&self, row: u32, column: u32) -> Option<CellRange> {
        self.merged.iter().copied().find(|range| {
            range.start_row <= row && row <= range.end_row && range.start_column <= column && column <= range.end_column
        })
    }

    /// Comment attached to the given cell.
    pub fn comment_at(&self, reference: &str) -> Option<&CellComment> {
        self.comments.iter().find(|comment| comment.reference == reference)
    }
}

/// Resolved workbook style table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkbookStyles {
    /// Custom number formats keyed by id.
    pub number_formats: BTreeMap<u32, String>,
    /// Resolved cell formats indexed by `s`.
    pub cell_formats: Vec<CellStyle>,
    /// Theme color slots after tint resolution.
    pub theme_colors: BTreeMap<String, String>,
}

/// Non-fatal import issue with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XlsxIssue {
    /// Stable diagnostic code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Whether the construct is partial or refused.
    pub status: IssueStatus,
    /// Sheet name when the issue is sheet-scoped.
    pub sheet: Option<String>,
    /// A1 reference when the issue is cell-scoped.
    pub cell: Option<String>,
}

/// Severity class for a format issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueStatus {
    /// Construct kept with a known fidelity gap.
    Partial,
    /// Construct refused by design.
    Unsupported,
}

/// Aggregate counts used by acceptance reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct XlsxStats {
    /// Number of sheets in the workbook.
    pub sheet_count: usize,
    /// Number of visible sheets.
    pub visible_sheet_count: usize,
    /// Number of cells carrying a value.
    pub cell_count: usize,
    /// Number of cells whose display text is non-empty.
    pub non_empty_cell_count: usize,
    /// Number of non-whitespace characters across display text.
    pub non_whitespace_chars: usize,
    /// Number of merged ranges.
    pub merged_range_count: usize,
    /// Number of anchored images.
    pub image_count: usize,
    /// Number of cell comments.
    pub comment_count: usize,
}

/// Parsed XLSX workbook.
#[derive(Debug, Clone, PartialEq)]
pub struct XlsxWorkbook {
    /// Caller-visible label such as a file path.
    pub label: String,
    /// Workbook date epoch.
    pub date_system: DateSystem,
    /// Sheets in workbook order.
    pub sheets: Vec<XlsxSheet>,
    /// Resolved styles.
    pub styles: WorkbookStyles,
    /// Defined names as authored.
    pub defined_names: Vec<String>,
    /// Whether the package carries a VBA project.
    pub macro_enabled: bool,
    /// Non-fatal issues collected during import.
    pub issues: Vec<XlsxIssue>,
    /// Aggregate counts.
    pub stats: XlsxStats,
}

impl XlsxWorkbook {
    /// Pushes one issue and keeps the vector stable.
    pub fn push_issue(&mut self, issue: XlsxIssue) {
        self.issues.push(issue);
    }

    /// Sheet inside the workbook by name.
    pub fn sheet(&self, name: &str) -> Option<&XlsxSheet> {
        self.sheets.iter().find(|sheet| sheet.name == name)
    }
}

/// Stable codes emitted by the XLSX reader and view projection.
pub mod codes {
    /// Charts are detected and kept out of the grid view.
    pub const CHART: &str = "import.xlsx.chart";
    /// A chart type or graphic frame is not modeled.
    pub const DRAWING_UNSUPPORTED: &str = "import.xlsx.drawing.unsupported";
    /// Conditional formatting rules are detected but not evaluated.
    pub const CONDITIONAL_FORMATTING: &str = "import.xlsx.conditional-formatting";
    /// Data validations are detected but not enforced.
    pub const DATA_VALIDATION: &str = "import.xlsx.data-validation";
    /// A defined name is kept as text only.
    pub const DEFINED_NAME: &str = "import.xlsx.defined-name";
    /// A worksheet table object is detected but not expanded.
    pub const TABLE: &str = "import.xlsx.table";
    /// A pivot table is detected but not read.
    pub const PIVOT: &str = "import.xlsx.pivot";
    /// A shared formula child lost its master source.
    pub const SHARED_FORMULA: &str = "import.xlsx.shared-formula";
    /// A shared string index was outside the shared string table.
    pub const SHARED_STRING: &str = "import.xlsx.shared-string";
    /// A cell style index was outside the style table.
    pub const STYLE_INDEX: &str = "import.xlsx.style-index";
    /// Array formulas are carried as their authored source only.
    pub const ARRAY_FORMULA: &str = "import.xlsx.array-formula";
    /// A macro or VBA project is refused.
    pub const MACRO: &str = "import.xlsx.macro";
    /// An embedded OLE object is refused.
    pub const EMBEDDED_OBJECT: &str = "import.xlsx.embedded-object";
    /// A comment is carried without its cell anchor in the IR projection.
    pub const COMMENT_ANCHOR: &str = "import.xlsx.comment-anchor";
    /// An image anchor has no IR carrier.
    pub const IMAGE_ANCHOR: &str = "import.xlsx.image-anchor";
    /// A theme color could not be resolved.
    pub const THEME_COLOR: &str = "import.xlsx.theme-color";
    /// A number format was approximated for display.
    pub const NUMBER_FORMAT: &str = "import.xlsx.number-format";
    /// A hidden or very hidden sheet was excluded from the IR projection.
    pub const HIDDEN_SHEET: &str = "import.xlsx.hidden-sheet";
    /// Hidden rows or columns are carried as ordinary table rows.
    pub const HIDDEN_ROW_COLUMN: &str = "import.xlsx.hidden-row-column";
    /// A sheet exceeded the view cell budget.
    pub const VIEW_CELL_BUDGET: &str = "render.xlsx.cell-budget";
    /// A column was wider than the printable page and was clamped.
    pub const VIEW_COLUMN_CLAMPED: &str = "render.xlsx.column-clamped";
    /// A row was taller than the printable page and was clamped.
    pub const VIEW_ROW_CLAMPED: &str = "render.xlsx.row-clamped";
    /// A merged range crossed a page boundary and was clipped.
    pub const VIEW_MERGE_SPLIT: &str = "render.xlsx.merge-split";
    /// A frozen pane was repeated on every page instead of pinned on one page.
    pub const VIEW_FROZEN_REPEATED: &str = "render.xlsx.frozen-repeated";
}
