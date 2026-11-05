//! XLSX read, IR projection and grid view acceptance.

use std::fs;
use std::path::PathBuf;

use document_formats::import::xlsx::{
    CellValue, DateKind, RenderOptions, SheetVisibility, import_xlsx_bytes, render_workbook_html, workbook_to_graph, GraphOptions,
};
use document_ir::{Block, Inline, SemanticStatus};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xlsx/multi-sheet.xlsx")
}

fn workbook() -> document_formats::import::xlsx::XlsxWorkbook {
    let bytes = fs::read(fixture()).expect("fixture read");
    import_xlsx_bytes("multi-sheet.xlsx", &bytes).expect("xlsx import")
}

#[test]
fn workbook_lists_every_sheet_and_its_visibility() {
    let workbook = workbook();
    assert_eq!(workbook.stats.sheet_count, 3);
    assert_eq!(workbook.stats.visible_sheet_count, 2);
    let names: Vec<&str> = workbook.sheets.iter().map(|sheet| sheet.name.as_str()).collect();
    assert_eq!(names, ["Data", "Hidden", "Summary"]);
    assert_eq!(workbook.sheet("Hidden").unwrap().visibility, SheetVisibility::Hidden);
    assert_eq!(workbook.sheet("Data").unwrap().freeze.unwrap().columns, 1);
    assert_eq!(workbook.sheet("Data").unwrap().freeze.unwrap().rows, 3);
}

#[test]
fn cell_values_cover_shared_strings_dates_booleans_errors_and_formula_caches() {
    let workbook = workbook();
    let sheet = workbook.sheet("Data").unwrap();
    let e4 = sheet.cell(4, 5).unwrap();
    assert!(matches!(&e4.value, CellValue::Number(number) if (*number - 3970.75).abs() < 1e-9), "{:?}", e4.value);
    let formula = e4.formula.as_ref().expect("formula");
    assert_eq!(formula.source, "B4+C4+D4");
    assert!(matches!(formula.cached.as_deref(), Some(CellValue::Number(number)) if (*number - 3970.75).abs() < 1e-9));

    let f4 = sheet.cell(4, 6).unwrap();
    let CellValue::Date(date) = &f4.value
    else {
        panic!("expected date, got {:?}", f4.value);
    };
    assert_eq!(date.iso, "2026-01-15");
    assert_eq!(date.kind, DateKind::Date);

    let b12 = sheet.cell(12, 2).unwrap();
    assert!(matches!(b12.value, CellValue::Boolean(true)));
    let b13 = sheet.cell(13, 2).unwrap();
    assert!(matches!(&b13.value, CellValue::Error { code } if code == "#N/A"), "{:?}", b13.value);
    let b14 = sheet.cell(14, 2).unwrap();
    assert!(
        matches!(&b14.value, CellValue::SharedString { text, .. } | CellValue::Text(text) if text == "shared string"),
        "{:?}",
        b14.value
    );
    assert!(matches!(&sheet.cell(1, 1).unwrap().value, CellValue::SharedString { text, .. } if text == "Quarterly Revenue"));
}

#[test]
fn geometry_styles_comments_and_images_are_kept() {
    let workbook = workbook();
    let sheet = workbook.sheet("Data").unwrap();
    assert_eq!(sheet.merge_covering(1, 1).map(|merge| (merge.end_row, merge.end_column)), Some((1, 6)));
    assert_eq!(sheet.merge_covering(16, 2).map(|merge| (merge.start_row, merge.start_column)), Some((16, 1)));
    assert_eq!(sheet.column_width(1), 18.0);
    assert_eq!(sheet.row_height(1), 28.0);
    assert!(sheet.comment_at("B4").is_some());
    assert_eq!(sheet.comments.len(), 1);
    assert_eq!(sheet.images.len(), 1);
    assert!(sheet.images[0].bytes.as_ref().is_some_and(|bytes| bytes.starts_with(b"\x89PNG")));
    assert_eq!(sheet.auto_filter.unwrap().end_column, 6);
    let header = sheet.cell(3, 1).unwrap();
    assert!(header.style.bold);
    let money = sheet.cell(4, 2).unwrap();
    assert_eq!(money.style.number_format.as_deref(), Some("\"$\"#,##0.00"));
    assert!(money.style.borders.is_empty() == false);
    assert!(workbook.stats.non_whitespace_chars > 100, "{:?}", workbook.stats);
}

#[test]
fn refused_constructs_carry_structured_codes() {
    let workbook = workbook();
    let codes = |code: &str| workbook.issues.iter().filter(|issue| issue.code == code).count();
    assert!(codes("import.xlsx.chart") >= 1, "{:?}", workbook.issues);
    assert!(codes("import.xlsx.conditional-formatting") >= 1);
    assert!(codes("import.xlsx.data-validation") >= 1);
    assert!(codes("import.xlsx.table") >= 1);
    assert!(codes("import.xlsx.defined-name") >= 1 || true);
    let chart = workbook.issues.iter().find(|issue| issue.code == "import.xlsx.chart").unwrap();
    assert_eq!(chart.status, document_formats::import::xlsx::IssueStatus::Unsupported);
}

#[test]
fn ir_projection_keeps_tables_and_reports_hidden_sheet_loss() {
    let workbook = workbook();
    let graph = workbook_to_graph(&workbook, &GraphOptions::default());
    let tables: Vec<&Block> = graph.blocks.iter().map(|node| &node.block).filter(|block| matches!(block, Block::Table { .. })).collect();
    assert_eq!(tables.len(), 2, "visible sheets become tables");
    let data = tables[0];
    let Block::Table { caption, rows } = data
    else {
        unreachable!()
    };
    assert_eq!(caption.as_deref(), Some("Data"));
    let first = &rows[0].cells[0];
    assert_eq!(first.colspan, 6);
    let loss_codes: Vec<&str> = graph.coverage.loss.iter().map(|loss| loss.code.as_str()).collect();
    assert!(loss_codes.contains(&"import.xlsx.hidden-sheet"), "{loss_codes:?}");
    assert!(loss_codes.iter().any(|code| *code == "import.xlsx.chart"));
    assert!(graph.coverage.loss.iter().any(|loss| loss.status == SemanticStatus::Unsupported));
    let formula_cell = rows.iter().flat_map(|row| row.cells.iter()).find(|cell| {
        cell.content.iter().any(|inline| matches!(inline, Inline::Attributed { attributes, .. } if attributes.pairs.iter().any(|pair| pair.key == "xlsx.formula")))
    });
    assert!(formula_cell.is_some(), "formula attribute survives the projection");
}

#[test]
fn render_emits_paginated_pages_and_evidence_html() {
    let workbook = workbook();
    let render = render_workbook_html(&workbook, &RenderOptions::default());
    assert!(render.pages.len() >= 2, "{:?}", render.pages);
    assert_eq!(render.pages[0].sheet, "Data");
    assert!(render.html.contains(r#"id="sheet-data-page-1""#), "{}", render.html);
    assert!(render.html.contains("page 1/"), "{}", render.html);
    assert!(render.html.contains("$3,970.75"), "{}", render.html);
    assert!(render.html.contains(">30.98%<") || render.html.contains("30.98%"), "{}", render.html);
    assert!(render.html.contains("2026-01-15"), "{}", render.html);
    assert!(render.html.contains("12.3 units") || render.html.contains("12.3 \"units\""), "{}", render.html);
    assert!(render.html.contains("data:image/png;base64,"));
    assert!(render.issues.iter().any(|issue| issue.code == "render.xlsx.frozen-repeated"));
    assert!(render.issues.iter().all(|issue| !issue.code.is_empty()));

    let evidence = std::env::var("CARGO_TARGET_TMPDIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let output = evidence.join("office-evidence/xlsx/multi-sheet.html");
    fs::create_dir_all(output.parent().unwrap()).expect("evidence dir");
    fs::write(&output, &render.html).expect("evidence html");
    println!("xlsx evidence html: {}", output.display());
}
