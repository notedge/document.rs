//! OPC and SpreadsheetML reading.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use acorn_core::{Budget, BudgetTracker};
use acorn_docx::{OpcPackage, OpcRelationship, resolve_opc_target};
use oak_xml::ast::XmlElement;

use super::model::{
    CellComment, CellFormula, CellStyle, CellValue, DateKind, DateSystem, FormulaKind, FreezePane, ImageAnchor,
    IssueStatus, SheetColumn, SheetImage, SheetRow, SheetVisibility, WorkbookStyles, XlsxCell, XlsxIssue, XlsxSheet,
    XlsxStats, XlsxWorkbook, codes,
};
use super::reference::{parse_range, parse_reference, reference};
use super::styles::parse_styles;
use super::value::{date_from_iso, detect_date_kind, display_cell, excel_date, is_date_format};
use crate::import::office::{
    attribute_value, bool_attribute, child_named, children_named, descendants_named, document_root, element_text,
    element_text_without, f64_attribute, local_name, parse_xml, u32_attribute,
};
use crate::FormatError;

const WORKBOOK_PART: &str = "xl/workbook.xml";
const SHARED_STRINGS_PART: &str = "xl/sharedStrings.xml";
const STYLES_PART: &str = "xl/styles.xml";
const THEME_PART: &str = "xl/theme/theme1.xml";

/// Imports an XLSX workbook from disk.
pub fn import_xlsx(path: impl AsRef<Path>) -> Result<XlsxWorkbook, FormatError> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|error| FormatError::invalid_input(format!("failed to read {}: {error}", path.display())))?;
    import_xlsx_bytes(&path.display().to_string(), &bytes)
}

/// Imports XLSX bytes into the format-native workbook model.
pub fn import_xlsx_bytes(label: &str, bytes: &[u8]) -> Result<XlsxWorkbook, FormatError> {
    if !looks_like_zip(bytes) {
        return Err(FormatError::invalid_input("input is not a ZIP-based XLSX package"));
    }
    let package = OpcPackage::open(label.to_string(), bytes.to_vec()).map_err(map_opc_error)?;
    let mut ledger = BudgetTracker::new(Budget::default());
    let mut issues = Vec::new();

    let workbook_part =
        package.main_document_part_path(&mut ledger).map_err(map_opc_error)?.unwrap_or_else(|| WORKBOOK_PART.to_owned());
    let workbook_xml = package.read_part(&workbook_part, &mut ledger).map_err(map_opc_error)?;
    let workbook_value = parse_xml(&workbook_xml, "xlsx")?;
    let workbook_root = document_root(&workbook_value, "xlsx")?;

    let workbook_rels = read_rels(&package, &mut ledger, &workbook_part);
    let date_system = bool_attribute_of(book_workbook_prop(workbook_root), "date1904").map_or(DateSystem::Date1900, |enabled| {
        if enabled {
            DateSystem::Date1904
        }
        else {
            DateSystem::Date1900
        }
    });

    let shared_strings = read_shared_strings(&package, &mut ledger, &workbook_rels, &workbook_part);
    let styles_part = find_part(&workbook_rels, "/styles", &workbook_part).unwrap_or_else(|| STYLES_PART.to_owned());
    let theme_part = find_part(&workbook_rels, "/theme", &workbook_part).unwrap_or_else(|| THEME_PART.to_owned());
    let theme = read_part_optional(&package, &mut ledger, &theme_part);
    let (styles, style_issues) =
        parse_styles(&read_part_or_default(&package, &mut ledger, &styles_part), theme.as_deref())?;
    issues.extend(style_issues);

    let macro_enabled = package.part_paths().iter().any(|path| local_name(path).eq_ignore_ascii_case("vbaProject.bin"));
    if macro_enabled {
        issues.push(issue(codes::MACRO, "VBA project detected and refused by the XLSX reader", IssueStatus::Unsupported, None, None));
    }
    if package.part_paths().iter().any(|path| path.starts_with("xl/externalLinks/")) {
        issues.push(issue(
            codes::DEFINED_NAME,
            "external workbook links are carried as names only",
            IssueStatus::Partial,
            None,
            None,
        ));
    }

    let mut sheets = Vec::new();
    let mut sheet_number = 0u32;
    let sheet_entries = child_named(workbook_root, "sheets").map(|sheets| children_named(sheets, "sheet")).unwrap_or_default();
    for sheet in sheet_entries {
        let name = attribute_value(sheet, "name").unwrap_or_else(|| format!("Sheet{}", sheet_number + 1));
        let visibility = match attribute_value(sheet, "state").as_deref() {
            Some("hidden") => SheetVisibility::Hidden,
            Some("veryHidden") => SheetVisibility::VeryHidden,
            _ => SheetVisibility::Visible,
        };
        let relation_id = attribute_value(sheet, "id");
        let target = relation_id
            .as_deref()
            .and_then(|id| workbook_rels.get(id))
            .map(|relationship| resolve_target(&workbook_part, &relationship.target))
            .unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", sheet_number + 1));
        let relationship_type = relation_id.as_deref().and_then(|id| workbook_rels.get(id)).map(|relationship| relationship.relationship_type.clone());
        let is_worksheet = relationship_type.as_deref().map(|kind| kind.ends_with("/worksheet")).unwrap_or(true);
        if !is_worksheet {
            issues.push(issue(
                codes::DRAWING_UNSUPPORTED,
                format!("sheet `{name}` uses an unsupported sheet part type"),
                IssueStatus::Unsupported,
                Some(name.clone()),
                None,
            ));
            sheet_number += 1;
            continue;
        }
        let Ok(sheet_xml) = package.read_part(&target, &mut ledger)
        else {
            issues.push(issue(
                codes::DRAWING_UNSUPPORTED,
                format!("sheet `{name}` part `{target}` is missing"),
                IssueStatus::Unsupported,
                Some(name.clone()),
                None,
            ));
            sheet_number += 1;
            continue;
        };
        let mut parsed = parse_sheet(
            &package,
            &mut ledger,
            &target,
            &sheet_xml,
            &name,
            sheet_number,
            visibility,
            &styles,
            &shared_strings,
            date_system,
            &mut issues,
        )?;
        parsed.index = sheet_number;
        parsed.visibility = visibility;
        sheets.push(parsed);
        sheet_number += 1;
    }

    let mut defined_names = Vec::new();
    if let Some(block) = descendant_named(workbook_root, "definedNames") {
        for name in children_named(block, "definedName") {
            defined_names.push(element_text(name).trim().to_owned());
        }
    }
    if !defined_names.is_empty() {
        issues.push(issue(
            codes::DEFINED_NAME,
            format!("{} defined names read as text", defined_names.len()),
            IssueStatus::Partial,
            None,
            None,
        ));
    }

    let stats = compute_stats(&sheets, &styles);
    Ok(XlsxWorkbook {
        label: label.to_owned(),
        date_system,
        sheets,
        styles,
        defined_names,
        macro_enabled,
        issues,
        stats,
    })
}

fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") || bytes.starts_with(b"PK\x07\x08")
}

fn map_opc_error(error: acorn_docx::OpcError) -> FormatError {
    FormatError::parse("xlsx", error.to_string())
}

fn read_rels(package: &OpcPackage, ledger: &mut BudgetTracker, part: &str) -> HashMap<String, OpcRelationship> {
    let rels_path = rels_part_path(part);
    package
        .read_relationships(&rels_path, ledger)
        .unwrap_or_default()
        .into_iter()
        .map(|relationship| (relationship.id.clone(), relationship))
        .collect()
}

fn resolve_target(owner: &str, target: &str) -> String {
    match target.strip_prefix('/') {
        Some(absolute) => absolute.to_owned(),
        None => resolve_opc_target(owner, target),
    }
}

fn rels_part_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((directory, file)) => format!("{directory}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

fn find_part(relationships: &HashMap<String, OpcRelationship>, kind_suffix: &str, owner: &str) -> Option<String> {
    relationships
        .values()
        .find(|relationship| relationship.relationship_type.ends_with(kind_suffix))
        .map(|relationship| resolve_target(owner, &relationship.target))
}

fn read_part_optional(package: &OpcPackage, ledger: &mut BudgetTracker, part: &str) -> Option<Vec<u8>> {
    package.read_part(part, ledger).ok()
}

fn read_part_or_default(package: &OpcPackage, ledger: &mut BudgetTracker, part: &str) -> Vec<u8> {
    read_part_optional(package, ledger, part).unwrap_or_else(|| b"<styleSheet/>".to_vec())
}

fn read_shared_strings(
    package: &OpcPackage,
    ledger: &mut BudgetTracker,
    relationships: &HashMap<String, OpcRelationship>,
    owner: &str,
) -> Vec<String> {
    let part = find_part(relationships, "/sharedStrings", owner).unwrap_or_else(|| SHARED_STRINGS_PART.to_owned());
    let Some(xml) = read_part_optional(package, ledger, &part)
    else {
        return Vec::new();
    };
    let Ok(value) = parse_xml(&xml, "xlsx")
    else {
        return Vec::new();
    };
    let Some(root) = document_root(&value, "xlsx").ok()
    else {
        return Vec::new();
    };
    children_named(root, "si")
        .into_iter()
        .map(|item| element_text_without(item, &["rPh"]))
        .collect()
}

fn book_workbook_prop<'a>(root: &'a XmlElement) -> Option<&'a XmlElement> {
    child_named(root, "workbookPr")
}

fn bool_attribute_of(element: Option<&XmlElement>, name: &str) -> Option<bool> {
    element.and_then(|element| bool_attribute(element, name))
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

#[allow(clippy::too_many_arguments)]
fn parse_sheet(
    package: &OpcPackage,
    ledger: &mut BudgetTracker,
    part: &str,
    xml: &[u8],
    name: &str,
    index: u32,
    visibility: SheetVisibility,
    styles: &WorkbookStyles,
    shared_strings: &[String],
    date_system: DateSystem,
    issues: &mut Vec<XlsxIssue>,
) -> Result<XlsxSheet, FormatError> {
    let value = parse_xml(xml, "xlsx")?;
    let root = document_root(&value, "xlsx")?;

    let (default_row_height, default_column_width) = child_named(root, "sheetFormatPr")
        .map(|format| {
            (
                f64_attribute(format, "defaultRowHeight").unwrap_or(15.0),
                f64_attribute(format, "defaultColWidth").unwrap_or(8.43),
            )
        })
        .unwrap_or((15.0, 8.43));
    let dimension = child_named(root, "dimension").and_then(|dimension| attribute_value(dimension, "ref")).and_then(|text| parse_range(&text));

    let columns = child_named(root, "cols")
        .map(|cols| {
            children_named(cols, "col")
                .into_iter()
                .filter_map(|column| {
                    Some(SheetColumn {
                        min: u32_attribute(column, "min")?,
                        max: u32_attribute(column, "max")?,
                        width: f64_attribute(column, "width").unwrap_or(default_column_width),
                        custom: bool_attribute(column, "customWidth").unwrap_or(false),
                        hidden: bool_attribute(column, "hidden").unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let mut rows = Vec::new();
    let mut cells: Vec<XlsxCell> = Vec::new();
    if let Some(data) = child_named(root, "sheetData") {
        for row in children_named(data, "row") {
            let row_index = u32_attribute(row, "r").unwrap_or(rows.last().map(|row: &SheetRow| row.index + 1).unwrap_or(1));
            let hidden = bool_attribute(row, "hidden").unwrap_or(false);
            let height = f64_attribute(row, "ht");
            let custom_height = bool_attribute(row, "customHeight").unwrap_or(false);
            rows.push(SheetRow { index: row_index, height, custom_height, hidden });
            let mut next_column = 1;
            for cell in children_named(row, "c") {
                let (cell_row, cell_column, cell_reference) = match attribute_value(cell, "r").and_then(|text| parse_reference(&text)) {
                    Some((cell_row, cell_column)) => (cell_row, cell_column, reference(cell_row, cell_column)),
                    None => {
                        let cell_reference = reference(row_index, next_column);
                        (row_index, next_column, cell_reference)
                    }
                };
                next_column = cell_column + 1;
                let style_index = u32_attribute(cell, "s");
                let style = style_index
                    .and_then(|style_index| styles.cell_formats.get(style_index as usize).cloned())
                    .unwrap_or_default();
                if let Some(style_index) = style_index {
                    if styles.cell_formats.get(style_index as usize).is_none() {
                        issues.push(issue(
                            codes::STYLE_INDEX,
                            format!("cell `{name}!{cell_reference}` references style {style_index}, which is outside the style table"),
                            IssueStatus::Partial,
                            Some(name.to_owned()),
                            Some(cell_reference.clone()),
                        ));
                    }
                }
                let parsed = parse_cell_value(cell, shared_strings, &style, date_system, name, &cell_reference, issues);
                let formula = parse_formula(cell, &parsed, name, &cell_reference, issues);
                cells.push(XlsxCell {
                    reference: cell_reference,
                    row: cell_row,
                    column: cell_column,
                    value: parsed,
                    formula,
                    style_index,
                    style,
                });
            }
        }
    }
    cells.sort_by_key(|cell| (cell.row, cell.column));

    let merged = child_named(root, "mergeCells")
        .map(|block| children_named(block, "mergeCell").into_iter().filter_map(|cell| attribute_value(cell, "ref").and_then(|text| parse_range(&text))).collect())
        .unwrap_or_default();

    let freeze = child_named(root, "sheetViews")
        .and_then(|views| child_named(views, "sheetView"))
        .and_then(|view| child_named(view, "pane"))
        .and_then(|pane| {
            let state = attribute_value(pane, "state").unwrap_or_default();
            if state != "frozen" && state != "frozenSplit" {
                return None;
            }
            Some(FreezePane {
                columns: f64_attribute(pane, "xSplit").unwrap_or(0.0).floor() as u32,
                rows: f64_attribute(pane, "ySplit").unwrap_or(0.0).floor() as u32,
            })
        });

    let auto_filter = child_named(root, "autoFilter").and_then(|filter| attribute_value(filter, "ref")).and_then(|text| parse_range(&text));
    let conditional_formatting = child_named(root, "conditionalFormatting").is_some();
    let data_validations = child_named(root, "dataValidations").is_some();

    if conditional_formatting {
        issues.push(issue(
            codes::CONDITIONAL_FORMATTING,
            format!("sheet `{name}` has conditional formatting rules that are not evaluated"),
            IssueStatus::Partial,
            Some(name.to_owned()),
            None,
        ));
    }
    if data_validations {
        issues.push(issue(
            codes::DATA_VALIDATION,
            format!("sheet `{name}` has data validations that are not enforced"),
            IssueStatus::Partial,
            Some(name.to_owned()),
            None,
        ));
    }
    let table_parts = child_named(root, "tableParts").and_then(|parts| u32_attribute(parts, "count")).unwrap_or(0);
    if table_parts > 0 {
        issues.push(issue(
            codes::TABLE,
            format!("sheet `{name}` declares {table_parts} table objects that are kept as cell values only"),
            IssueStatus::Partial,
            Some(name.to_owned()),
            None,
        ));
    }
    if child_named(root, "pivotTableParts").is_some() {
        issues.push(issue(
            codes::PIVOT,
            format!("sheet `{name}` declares a pivot table that is not read"),
            IssueStatus::Unsupported,
            Some(name.to_owned()),
            None,
        ));
    }
    let embedded_objects = child_named(root, "oleObjects").and_then(|objects| u32_attribute(objects, "count")).unwrap_or(0);
    if embedded_objects > 0 {
        issues.push(issue(
            codes::EMBEDDED_OBJECT,
            format!("sheet `{name}` embeds {embedded_objects} OLE objects that are refused"),
            IssueStatus::Unsupported,
            Some(name.to_owned()),
            None,
        ));
    }

    let sheet_rels = read_rels(package, ledger, part);
    let mut images = Vec::new();
    let mut comments = Vec::new();
    let mut charts = 0u32;
    if let Some(drawing_part) = child_named(root, "drawing")
        .and_then(|drawing| attribute_value(drawing, "id"))
        .and_then(|id| sheet_rels.get(&id))
        .map(|relationship| resolve_target(part, &relationship.target))
    {
        if let Some(drawing_xml) = read_part_optional(package, ledger, &drawing_part) {
            let (read_images, chart_count, drawing_issues) =
                parse_drawing(package, ledger, &drawing_part, &drawing_xml, &sheet_rels, name);
            images.extend(read_images);
            charts = chart_count;
            issues.extend(drawing_issues);
        }
    }
    if let Some(comments_part) = sheet_rels.values().find(|relationship| relationship.relationship_type.ends_with("/comments")).map(|relationship| resolve_target(part, &relationship.target)) {
        if let Some(comments_xml) = read_part_optional(package, ledger, &comments_part) {
            comments = parse_comments(&comments_xml, name, issues);
        }
    }
    if sheet_rels.values().any(|relationship| relationship.relationship_type.ends_with("/threadedComment")) {
        issues.push(issue(
            codes::COMMENT_ANCHOR,
            format!("sheet `{name}` uses threaded comments that are not read"),
            IssueStatus::Partial,
            Some(name.to_owned()),
            None,
        ));
    }

    Ok(XlsxSheet {
        name: name.to_owned(),
        index,
        visibility,
        dimension,
        columns,
        rows,
        cells,
        merged,
        freeze,
        images,
        comments,
        auto_filter,
        conditional_formatting,
        data_validations,
        charts,
        embedded_objects,
        legacy_macro: false,
        default_row_height,
        default_column_width,
    })
}

fn parse_cell_value(
    cell: &XmlElement,
    shared_strings: &[String],
    style: &CellStyle,
    date_system: DateSystem,
    sheet: &str,
    cell_reference: &str,
    issues: &mut Vec<XlsxIssue>,
) -> CellValue {
    let kind = attribute_value(cell, "t").unwrap_or_else(|| "n".to_owned());
    if kind == "inlineStr" {
        return child_named(cell, "is")
            .map(|inline| textual_value(element_text(inline).trim(), style, date_system))
            .unwrap_or(CellValue::Empty);
    }
    let Some(value) = child_named(cell, "v").map(element_text)
    else {
        return CellValue::Empty;
    };
    let value = value.trim();
    match kind.as_str() {
        "s" => {
            let Ok(index) = value.parse::<u32>()
            else {
                return CellValue::Empty;
            };
            match shared_strings.get(index as usize) {
                Some(text) => CellValue::SharedString { index, text: text.clone() },
                None => {
                    issues.push(issue(
                        codes::SHARED_STRING,
                        format!("cell `{sheet}!{cell_reference}` references shared string {index}, which is outside the shared string table"),
                        IssueStatus::Partial,
                        Some(sheet.to_owned()),
                        Some(cell_reference.to_owned()),
                    ));
                    CellValue::Text(String::new())
                }
            }
        }
        "b" => CellValue::Boolean(value == "1" || value.eq_ignore_ascii_case("true")),
        "e" => CellValue::Error { code: value.to_owned() },
        "str" => textual_value(value, style, date_system),
        "d" => {
            let kind = style.number_format.as_deref().and_then(detect_date_kind).unwrap_or(DateKind::DateTime);
            CellValue::Date(super::model::ExcelDate { serial: 0.0, iso: value.to_owned(), kind })
        }
        _ => {
            let Ok(number) = value.parse::<f64>()
            else {
                return CellValue::Text(value.to_owned());
            };
            match style.number_format.as_deref() {
                Some(format) if is_date_format(format) => CellValue::Date(excel_date(number, date_system, format)),
                _ => CellValue::Number(number),
            }
        }
    }
}

fn textual_value(text: &str, style: &CellStyle, date_system: DateSystem) -> CellValue {
    match style.number_format.as_deref() {
        Some(format) if is_date_format(format) => match date_from_iso(text, format, date_system) {
            Some(date) => CellValue::Date(date),
            None => CellValue::Text(text.to_owned()),
        },
        _ => CellValue::Text(text.to_owned()),
    }
}

fn parse_formula(
    cell: &XmlElement,
    parsed: &CellValue,
    sheet: &str,
    cell_reference: &str,
    issues: &mut Vec<XlsxIssue>,
) -> Option<CellFormula> {
    let formula = child_named(cell, "f")?;
    let source = element_text(formula).trim().to_owned();
    let kind = attribute_value(formula, "t").unwrap_or_else(|| "normal".to_owned());
    let shared_index = u32_attribute(formula, "si");
    let range = attribute_value(formula, "ref");
    let formula_kind = match kind.as_str() {
        "shared" => match shared_index {
            Some(index) if source.is_empty() => {
                issues.push(issue(
                    codes::SHARED_FORMULA,
                    format!("cell `{sheet}!{cell_reference}` inherits shared formula {index} without a master source"),
                    IssueStatus::Partial,
                    Some(sheet.to_owned()),
                    Some(cell_reference.to_owned()),
                ));
                FormulaKind::SharedChild { index }
            }
            Some(index) => FormulaKind::SharedMaster { index, range },
            None => FormulaKind::Normal,
        },
        "array" => {
            issues.push(issue(
                codes::ARRAY_FORMULA,
                format!("cell `{sheet}!{cell_reference}` uses an array formula carried as source text only"),
                IssueStatus::Partial,
                Some(sheet.to_owned()),
                Some(cell_reference.to_owned()),
            ));
            FormulaKind::Array { range }
        }
        _ => FormulaKind::Normal,
    };
    let cached = child_named(cell, "v").map(|_| Box::new(parsed.clone()));
    Some(CellFormula { source, kind: formula_kind, cached })
}

fn parse_drawing(
    package: &OpcPackage,
    ledger: &mut BudgetTracker,
    part: &str,
    xml: &[u8],
    _sheet_rels: &HashMap<String, OpcRelationship>,
    sheet: &str,
) -> (Vec<SheetImage>, u32, Vec<XlsxIssue>) {
    let mut images = Vec::new();
    let mut charts = 0u32;
    let mut issues = Vec::new();
    let Ok(value) = parse_xml(xml, "xlsx")
    else {
        issues.push(issue(codes::DRAWING_UNSUPPORTED, format!("sheet `{sheet}` drawing part could not be parsed"), IssueStatus::Unsupported, Some(sheet.to_owned()), None));
        return (images, charts, issues);
    };
    let Ok(root) = document_root(&value, "xlsx")
    else {
        return (images, charts, issues);
    };
    let drawing_rels = read_rels(package, ledger, part);
    for anchor in children_named(root, "oneCellAnchor")
        .into_iter()
        .chain(children_named(root, "twoCellAnchor"))
        .chain(children_named(root, "absoluteAnchor"))
    {
        let from = child_named(anchor, "from").map(parse_anchor).unwrap_or(ImageAnchor { column: 0, column_offset: 0, row: 0, row_offset: 0 });
        let to = child_named(anchor, "to").map(parse_anchor);
        let extent = child_named(anchor, "ext").and_then(|ext| {
            let cx = i64::from(u32_attribute(ext, "cx").unwrap_or(0));
            let cy = i64::from(u32_attribute(ext, "cy").unwrap_or(0));
            Some((cx, cy))
        });
        if descendants_named(anchor, "chart").len() > 0 {
            charts += 1;
            let target = descendants_named(anchor, "chart")
                .first()
                .and_then(|chart| attribute_value(chart, "id"))
                .and_then(|id| drawing_rels.get(&id))
                .map(|relationship| resolve_target(part, &relationship.target))
                .unwrap_or_else(|| "unknown chart part".to_owned());
            issues.push(issue(
                codes::CHART,
                format!("sheet `{sheet}` chart `{target}` is detected and refused by the grid view"),
                IssueStatus::Unsupported,
                Some(sheet.to_owned()),
                None,
            ));
            continue;
        }
        let blips = descendants_named(anchor, "blip");
        let Some(blip) = blips.first()
        else {
            if !descendants_named(anchor, "sp").is_empty() || !descendants_named(anchor, "grpSp").is_empty() {
                issues.push(issue(
                    codes::DRAWING_UNSUPPORTED,
                    format!("sheet `{sheet}` drawing contains shapes that are not read"),
                    IssueStatus::Partial,
                    Some(sheet.to_owned()),
                    None,
                ));
            }
            continue;
        };
        let Some(embed) = attribute_value(blip, "embed")
        else {
            continue;
        };
        let Some(relationship) = drawing_rels.get(&embed)
        else {
            continue;
        };
        let media_path = resolve_target(part, &relationship.target);
        let bytes = read_part_optional(package, ledger, &media_path);
        let media_type = media_type_for(&media_path);
        images.push(SheetImage { from, to, media_path, media_type, extent, bytes });
    }
    (images, charts, issues)
}

fn parse_anchor(element: &XmlElement) -> ImageAnchor {
    let number = |name: &str| child_named(element, name).map(|child| element_text(child).trim().to_owned());
    ImageAnchor {
        column: number("col").and_then(|value| value.parse().ok()).unwrap_or(0),
        column_offset: number("colOff").and_then(|value| value.parse().ok()).unwrap_or(0),
        row: number("row").and_then(|value| value.parse().ok()).unwrap_or(0),
        row_offset: number("rowOff").and_then(|value| value.parse().ok()).unwrap_or(0),
    }
}

fn media_type_for(path: &str) -> Option<String> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        _ => return None,
    }
    .to_owned())
}

fn parse_comments(xml: &[u8], sheet: &str, issues: &mut Vec<XlsxIssue>) -> Vec<CellComment> {
    let Ok(value) = parse_xml(xml, "xlsx")
    else {
        issues.push(issue(codes::COMMENT_ANCHOR, format!("sheet `{sheet}` comments part could not be parsed"), IssueStatus::Partial, Some(sheet.to_owned()), None));
        return Vec::new();
    };
    let Ok(root) = document_root(&value, "xlsx")
    else {
        return Vec::new();
    };
    let authors: Vec<String> = child_named(root, "authors")
        .map(|block| children_named(block, "author").into_iter().map(element_text).collect())
        .unwrap_or_default();
    let Some(list) = child_named(root, "commentList")
    else {
        return Vec::new();
    };
    children_named(list, "comment")
        .into_iter()
        .filter_map(|comment| {
            let reference = attribute_value(comment, "ref")?;
            let author = u32_attribute(comment, "authorId").and_then(|index| authors.get(index as usize).cloned());
            let text = child_named(comment, "text").map(|text| element_text_without(text, &["rPh"])).unwrap_or_default();
            Some(CellComment { reference, author, text: text.trim().to_owned() })
        })
        .collect()
}

fn descendant_named<'a>(element: &'a XmlElement, name: &str) -> Option<&'a XmlElement> {
    if local_name(&element.name) == name {
        return Some(element);
    }
    for child in &element.children {
        if let oak_xml::ast::XmlValue::Element(child) = child {
            if let Some(found) = descendant_named(child, name) {
                return Some(found);
            }
        }
    }
    None
}

fn compute_stats(sheets: &[XlsxSheet], styles: &WorkbookStyles) -> XlsxStats {
    let mut stats = XlsxStats { sheet_count: sheets.len(), ..XlsxStats::default() };
    for sheet in sheets {
        if sheet.visibility == SheetVisibility::Visible {
            stats.visible_sheet_count += 1;
        }
        stats.merged_range_count += sheet.merged.len();
        stats.image_count += sheet.images.len();
        stats.comment_count += sheet.comments.len();
        for cell in &sheet.cells {
            stats.cell_count += 1;
            let displayed = display_cell(&cell.value, &cell.style, styles);
            if !displayed.text.trim().is_empty() {
                stats.non_empty_cell_count += 1;
            }
            stats.non_whitespace_chars += displayed.text.chars().filter(|character| !character.is_whitespace()).count();
        }
    }
    stats
}

/// Whether a declared number format is a date or time format.
pub fn number_format_is_date(format: &str) -> bool {
    is_date_format(format)
}

/// Converts a serial number with the workbook epoch into a date value.
pub fn date_from_serial(serial: f64, system: DateSystem, format: &str) -> super::model::ExcelDate {
    excel_date(serial, system, format)
}
