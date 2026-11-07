use std::collections::{BTreeSet, HashMap, HashSet};

use document_ir::{
    Asset, AssetId, AssetKind, Block, BreakKind, DocumentGraph, DocumentId, Inline, ListItem, LossMarker, MathDisplay, NodeId,
    PageSetup, RelationEndpoint, SemanticStatus, TableCell,
};
use oak_xml::ast::{XmlElement, XmlValue};

use super::{
    footnotes::FootnoteCatalog,
    math::{lower_omml, omath_children},
    numbering::{NumberingCatalog, marker_format_is_representable},
    styles::StyleDefaults,
    table::{TableState, push_table_block},
    xml_extensions::{
        attribute_value, bool_from_element, child_element, document_root, element_is, element_text, elements_by_local_name,
        parse_xml_bytes, u32_attribute,
    },
};
use crate::{FormatError, import::math_latex::record_math_unsupported};

/// Parses `word/document.xml` body content into a flat `DocumentGraph`.
pub fn parse_document_xml(
    xml: &[u8],
    rels: &HashMap<String, String>,
    numbering: &NumberingCatalog,
    footnotes: &FootnoteCatalog,
    defaults: &StyleDefaults,
    graph: &mut DocumentGraph,
) -> Result<HashSet<u32>, FormatError> {
    let value = parse_xml_bytes(xml)?;
    let root = document_root(&value)?;
    let body = child_element(root, "body").ok_or_else(|| FormatError::parse("docx", "word/document.xml is missing w:body"))?;

    if !elements_by_local_name(root, "ins")?.is_empty() || !elements_by_local_name(root, "del")?.is_empty() {
        graph.push_loss(LossMarker {
            code: "reader.docx.tracked_changes".into(),
            message: "tracked changes are projected to current visible text where possible. Deleted runs are omitted and revision identity is not retained".into(),
            status: SemanticStatus::Partial,
        });
    }
    if !elements_by_local_name(root, "commentReference")?.is_empty()
        || !elements_by_local_name(root, "commentRangeStart")?.is_empty()
    {
        graph.push_loss(LossMarker {
            code: "reader.docx.comments".into(),
            message: "comment markers are detected but comment bodies and range identity are not represented in notedown-ir"
                .into(),
            status: SemanticStatus::Partial,
        });
    }
    let has_any = |names: &[&str]| -> Result<bool, FormatError> {
        for name in names {
            if !elements_by_local_name(root, name)?.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    };
    if has_any(&["fldChar", "instrText", "delInstrText"])? || fld_simple_carries_an_unmapped_field(root)? {
        graph.push_loss(LossMarker {
            code: "reader.docx.fields".into(),
            message: "field boundaries and field instructions are detected but dynamic field semantics are not represented in notedown-ir"
                .into(),
            status: SemanticStatus::Partial,
        });
    }
    if has_any(&["object", "oleObject", "pict", "AlternateContent"])? {
        graph.push_loss(LossMarker {
            code: "reader.docx.embedded_objects".into(),
            message:
                "embedded OLE objects, legacy VML drawings, or DrawingML alternate content are detected but are not represented in notedown-ir"
                    .into(),
            status: SemanticStatus::Partial,
        });
    }
    let unsupported_run_properties = elements_by_local_name(root, "rPr")?
        .into_iter()
        .flat_map(|r_pr| {
            r_pr.children.iter().filter_map(XmlValue::as_element).filter(|child| {
                !["b", "i", "u", "strike", "vertAlign", "color", "sz", "szCs", "rFonts", "highlight"]
                    .contains(&super::xml_extensions::local_name(&child.name))
            })
        })
        .next()
        .is_some();
    if unsupported_run_properties {
        graph.push_loss(LossMarker {
            code: "reader.docx.character_properties".into(),
            message:
                "DOCX run properties beyond the mapped character styles are detected but are not represented in notedown-ir"
                    .into(),
            status: SemanticStatus::Partial,
        });
    }
    if elements_by_local_name(root, "br")?
        .iter()
        .any(|element| matches!(attribute_value(element, "type").as_deref(), Some("page" | "column")))
    {
        graph.push_loss(LossMarker {
            code: "reader.docx.page_breaks".into(),
            message: "page and column breaks are projected to ordinary line breaks because notedown-ir has no page-break node"
                .into(),
            status: SemanticStatus::Partial,
        });
    }
    if !elements_by_local_name(root, "pageBreakBefore")?.is_empty()
        || !elements_by_local_name(root, "lastRenderedPageBreak")?.is_empty()
    {
        graph.push_loss(LossMarker {
            code: "reader.docx.page_breaks".into(),
            message: "paragraph and layout page-break markers are projected to ordinary content because notedown-ir has no page-break node"
                .into(),
            status: SemanticStatus::Partial,
        });
    }
    let mut layout_loss = false;
    for element in elements_by_local_name(root, "sectPr")? {
        for name in ["docGrid", "headerReference", "footerReference"] {
            if !elements_by_local_name(element, name)?.is_empty() {
                layout_loss = true;
            }
        }
    }
    if layout_loss {
        graph.push_loss(LossMarker {
            code: "reader.docx.section_layout".into(),
            message:
                "document grid and header/footer references are detected but their layout is not represented in notedown-ir"
                    .into(),
            status: SemanticStatus::Partial,
        });
    }
    let page = section_page_setup(body, defaults);
    if page.has_values() {
        graph.page_setup = Some(page);
    }

    let mut referenced_footnotes = HashSet::new();
    let mut body_state = BodyState::default();

    for child in body.children.iter().filter_map(XmlValue::as_element) {
        if element_is(child, "tbl") {
            flush_pending_list(graph, &mut body_state, numbering);
            let table = parse_table_element(child, rels, graph, footnotes, &mut referenced_footnotes)?;
            push_table_block(graph, table);
        }
        else if element_is(child, "p") {
            let paragraph = parse_paragraph_element(child, rels, graph, footnotes, &mut referenced_footnotes)?;
            finish_paragraph(graph, &mut body_state, &paragraph, numbering);
        }
    }

    flush_pending_list(graph, &mut body_state, numbering);

    if graph.blocks.is_empty() {
        graph.push_loss(LossMarker {
            code: "reader.docx.empty_body".into(),
            message: "word/document.xml contained no paragraphs".into(),
            status: SemanticStatus::Partial,
        });
    }

    Ok(referenced_footnotes)
}

/// Reads the section properties declared at the end of `w:body`.
///
/// Word stores the last section's page size, margins and column count in the
/// body-level `w:sectPr`. Those facts are presentation, so they travel on
/// [`DocumentGraph::page_setup`] instead of a loss marker. The document
/// defaults from `word/styles.xml` fill the body size and line height the
/// section does not declare.
fn section_page_setup(body: &XmlElement, defaults: &StyleDefaults) -> PageSetup {
    let section = child_element(body, "sectPr")
        .or_else(|| elements_by_local_name(body, "sectPr").ok().and_then(|elements| elements.last().copied()));
    let mut page = PageSetup::default();
    if let Some(section) = section {
        if let Some(size) = child_element(section, "pgSz") {
            page.page_width = points_from_twips(attribute_value(size, "w"));
            page.page_height = points_from_twips(attribute_value(size, "h"));
        }
        if let Some(margins) = child_element(section, "pgMar") {
            page.margin_top = points_from_twips(attribute_value(margins, "top"));
            page.margin_right = points_from_twips(attribute_value(margins, "right"));
            page.margin_bottom = points_from_twips(attribute_value(margins, "bottom"));
            page.margin_left = points_from_twips(attribute_value(margins, "left"));
        }
        if let Some(columns) = child_element(section, "cols") {
            page.columns = u32_attribute(columns, "num").and_then(|count| u16::try_from(count).ok()).filter(|count| *count > 0);
        }
    }
    page.base_font_size = defaults.base_font_size;
    page.line_height = defaults.line_height;
    page
}

/// Converts a WordprocessingML twip length to points.
///
/// Word declares page dimensions and margins in twentieths of a point, and
/// treats a zero or negative length as unset.
fn points_from_twips(value: Option<String>) -> Option<f64> {
    value?.trim().parse::<f64>().ok().map(|twips| twips / 20.0).filter(|points| *points > 0.0)
}

/// Vertical merge state of a `w:tc`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VerticalMerge {
    None,
    Restart,
    Continue,
}

fn cell_vertical_merge(cell: &XmlElement) -> VerticalMerge {
    let Some(properties) = child_element(cell, "tcPr")
    else {
        return VerticalMerge::None;
    };
    let Some(merge) = child_element(properties, "vMerge")
    else {
        return VerticalMerge::None;
    };
    match attribute_value(merge, "val").as_deref() {
        Some(value) if value.eq_ignore_ascii_case("restart") => VerticalMerge::Restart,
        _ => VerticalMerge::Continue,
    }
}

fn cell_grid_span(cell: &XmlElement) -> u32 {
    child_element(cell, "tcPr")
        .and_then(|properties| child_element(properties, "gridSpan"))
        .and_then(|span| u32_attribute(span, "val"))
        .filter(|span| *span > 0)
        .unwrap_or(1)
}

/// Marks `column..column + colspan` as covered by `origin` in the next row.
fn mark_open(open: &mut Vec<Option<(usize, usize)>>, column: usize, colspan: usize, origin: Option<(usize, usize)>) {
    let width = column + colspan;
    if open.len() < width {
        open.resize(width, None);
    }
    for slot in &mut open[column..width] {
        *slot = origin;
    }
}

fn parse_table_element(
    table_element: &XmlElement,
    rels: &HashMap<String, String>,
    graph: &mut DocumentGraph,
    footnotes: &FootnoteCatalog,
    referenced_footnotes: &mut HashSet<u32>,
) -> Result<TableState, FormatError> {
    let mut table = TableState::default();
    // Grid columns whose `rowspan` the previous row still covers. A
    // continuation cell extends that origin cell instead of becoming its own.
    let mut open: Vec<Option<(usize, usize)>> = Vec::new();
    for row in table_element.children.iter().filter_map(XmlValue::as_element).filter(|child| element_is(child, "tr")) {
        let mut current_row: Vec<TableCell> = Vec::new();
        let mut next_open: Vec<Option<(usize, usize)>> = Vec::new();
        let mut column = 0usize;
        for cell in row.children.iter().filter_map(XmlValue::as_element).filter(|child| element_is(child, "tc")) {
            let colspan = cell_grid_span(cell) as usize;
            let merge = cell_vertical_merge(cell);
            let origin = match merge {
                VerticalMerge::Continue => open.get(column).copied().flatten(),
                VerticalMerge::None | VerticalMerge::Restart => None,
            };
            if let Some((origin_row, origin_cell)) = origin {
                table.rows[origin_row][origin_cell].rowspan += 1;
                mark_open(&mut next_open, column, colspan, Some((origin_row, origin_cell)));
            }
            else {
                let mut current_cell: Vec<NodeId> = Vec::new();
                for paragraph in cell.children.iter().filter_map(XmlValue::as_element).filter(|child| element_is(child, "p")) {
                    let paragraph = parse_paragraph_element(paragraph, rels, graph, footnotes, referenced_footnotes)?;
                    let mut content = paragraph_inlines(&paragraph).unwrap_or_default();
                    content.extend(math_block_inlines(&paragraph));
                    if !content.is_empty() {
                        current_cell.push(graph.push_block(Block::Paragraph { content }));
                    }
                }
                let cell_index = current_row.len();
                current_row.push(TableCell { children: current_cell, colspan: colspan as u32, rowspan: 1 });
                let origin = (merge == VerticalMerge::Restart).then_some((table.rows.len(), cell_index));
                mark_open(&mut next_open, column, colspan, origin);
            }
            column += colspan;
        }
        if !current_row.is_empty() {
            table.rows.push(current_row);
        }
        open = next_open;
    }
    Ok(table)
}

fn parse_paragraph_element(
    paragraph_element: &XmlElement,
    rels: &HashMap<String, String>,
    graph: &mut DocumentGraph,
    footnotes: &FootnoteCatalog,
    referenced_footnotes: &mut HashSet<u32>,
) -> Result<ParagraphState, FormatError> {
    let mut paragraph = ParagraphState::default();
    if let Some(p_pr) = child_element(paragraph_element, "pPr") {
        if let Some(p_style) = child_element(p_pr, "pStyle") {
            paragraph.style = attribute_value(p_style, "val");
        }
        if let Some(num_pr) = child_element(p_pr, "numPr") {
            paragraph.is_list_item = true;
            if let Some(num_id) = child_element(num_pr, "numId") {
                paragraph.num_id = u32_attribute(num_id, "val");
            }
            if let Some(ilvl) = child_element(num_pr, "ilvl") {
                paragraph.ilvl = u32_attribute(ilvl, "val");
            }
        }
    }

    for child in paragraph_element.children.iter().filter_map(XmlValue::as_element) {
        if element_is(child, "hyperlink") {
            let link = parse_hyperlink_element(child, rels, graph, footnotes, referenced_footnotes)?;
            paragraph.push_hyperlink(&link, rels, graph);
        }
        else if element_is(child, "r") {
            append_paragraph_run(child, &mut paragraph, rels, graph, footnotes, referenced_footnotes)?;
        }
        else if element_is(child, "ins") {
            for run in elements_by_local_name(child, "r")? {
                append_paragraph_run(run, &mut paragraph, rels, graph, footnotes, referenced_footnotes)?;
            }
        }
        else if element_is(child, "del") {
            // Deleted text is intentionally omitted while the document-level
            // loss records the revision gap.
        }
        else if element_is(child, "footnoteReference") {
            push_footnote_reference(graph, &mut paragraph.inlines, footnotes, referenced_footnotes, u32_attribute(child, "id"));
        }
        else if element_is(child, "fldSimple") {
            let instr = attribute_value(child, "instr").unwrap_or_default();
            let display = parse_field_result(child, rels, graph, footnotes, referenced_footnotes)?;
            if let Some(key) = citation_key(&instr) {
                // The exporter writes the semantic key into `w:instr` and the
                // authored display into the field result, so both sides of a
                // citation survive a DOCX round trip.
                paragraph
                    .inlines
                    .push(Inline::Citation { key, display: if display.is_empty() { None } else { Some(display) } });
            }
            else {
                // An unmapped field keeps its visible result; the document-level
                // `reader.docx.fields` loss names the dynamic semantics gap.
                paragraph.inlines.extend(display);
            }
        }
        else if element_is(child, "oMathPara") {
            for formula in omath_children(child) {
                paragraph.math_blocks.push(lower_docx_math(graph, formula, MathDisplay::Display));
            }
        }
        else if element_is(child, "oMath") {
            paragraph.inlines.push(Inline::InlineMath { formula: lower_docx_math_formula(graph, child, MathDisplay::Inline) });
        }
    }

    Ok(paragraph)
}

fn lower_docx_math(graph: &mut DocumentGraph, element: &XmlElement, display: MathDisplay) -> Block {
    Block::Math { formula: lower_docx_math_formula(graph, element, display) }
}

fn lower_docx_math_formula(graph: &mut DocumentGraph, element: &XmlElement, display: MathDisplay) -> document_ir::MathFormula {
    let (formula, unsupported) = lower_omml(element, display);
    record_math_unsupported(graph, "import.docx.math_unsupported", &unsupported);
    formula
}

fn append_paragraph_run(
    element: &XmlElement,
    paragraph: &mut ParagraphState,
    rels: &HashMap<String, String>,
    graph: &mut DocumentGraph,
    footnotes: &FootnoteCatalog,
    referenced_footnotes: &mut HashSet<u32>,
) -> Result<(), FormatError> {
    let mut run = parse_run_element(element);
    run.text.clear();
    for child in element.children.iter().filter_map(XmlValue::as_element) {
        if element_is(child, "t") {
            run.text.push_str(&element_text(child));
        }
        else if element_is(child, "tab") {
            run.text.push('\t');
        }
        else if element_is(child, "br") {
            paragraph.push_run(&run);
            run.text.clear();
            push_inline(&mut paragraph.inlines, Inline::Break { kind: BreakKind::Hard });
        }
        else if element_is(child, "footnoteReference") {
            paragraph.push_run(&run);
            run.text.clear();
            push_footnote_reference(graph, &mut paragraph.inlines, footnotes, referenced_footnotes, u32_attribute(child, "id"));
        }
        else if element_is(child, "drawing") {
            paragraph.push_run(&run);
            run.text.clear();
            let doc_prs = elements_by_local_name(child, "docPr")?;
            for (index, blip) in elements_by_local_name(child, "blip")?.iter().enumerate() {
                if let Some(rel_id) = attribute_value(blip, "embed") {
                    paragraph.pending_image_alt = doc_prs.get(index).and_then(|doc_pr| attribute_value(doc_pr, "descr"));
                    paragraph.pending_image_title = doc_prs
                        .get(index)
                        .and_then(|doc_pr| attribute_value(doc_pr, "title"))
                        .filter(|title| !title.is_empty());
                    paragraph.push_image(&rel_id, rels, graph);
                }
            }
        }
        else if element_is(child, "oMath") {
            paragraph.push_run(&run);
            run.text.clear();
            paragraph.inlines.push(Inline::InlineMath { formula: lower_docx_math_formula(graph, child, MathDisplay::Inline) });
        }
    }
    paragraph.push_run(&run);
    Ok(())
}

fn parse_hyperlink_element(
    element: &XmlElement,
    rels: &HashMap<String, String>,
    graph: &mut DocumentGraph,
    footnotes: &FootnoteCatalog,
    referenced_footnotes: &mut HashSet<u32>,
) -> Result<HyperlinkState, FormatError> {
    let mut link = HyperlinkState {
        rel_id: attribute_value(element, "id"),
        title: attribute_value(element, "tooltip"),
        inlines: Vec::new(),
    };
    for child in element.children.iter().filter_map(XmlValue::as_element) {
        if element_is(child, "r") {
            let mut paragraph = ParagraphState::default();
            append_paragraph_run(child, &mut paragraph, rels, graph, footnotes, referenced_footnotes)?;
            link.inlines.extend(paragraph.inlines);
        }
        else if element_is(child, "footnoteReference") {
            push_footnote_reference(graph, &mut link.inlines, footnotes, referenced_footnotes, u32_attribute(child, "id"));
        }
    }
    Ok(link)
}

/// Whether any `w:fldSimple` carries instructions other than a citation.
///
/// A citation field is mapped to `Inline::Citation`, so only the remaining
/// simple fields still need the document-level dynamic-field loss.
fn fld_simple_carries_an_unmapped_field(root: &XmlElement) -> Result<bool, FormatError> {
    for field in elements_by_local_name(root, "fldSimple")? {
        let instr = attribute_value(field, "instr").unwrap_or_default();
        if citation_key(&instr).is_none() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Citation key from a Word field instruction such as ` CITATION smith2024 `.
fn citation_key(instr: &str) -> Option<String> {
    let mut tokens = instr.split_whitespace();
    let field = tokens.next()?;
    let key = tokens.next()?;
    field.eq_ignore_ascii_case("CITATION").then(|| key.to_owned())
}

/// Visible inlines of a simple field result.
///
/// The exporter stores the authored display inside `w:fldSimple`, so reading
/// the child runs back is what keeps the citation display from disappearing.
fn parse_field_result(
    element: &XmlElement,
    rels: &HashMap<String, String>,
    graph: &mut DocumentGraph,
    footnotes: &FootnoteCatalog,
    referenced_footnotes: &mut HashSet<u32>,
) -> Result<Vec<Inline>, FormatError> {
    let mut paragraph = ParagraphState::default();
    for child in element.children.iter().filter_map(XmlValue::as_element) {
        if element_is(child, "r") {
            append_paragraph_run(child, &mut paragraph, rels, graph, footnotes, referenced_footnotes)?;
        }
        else if element_is(child, "hyperlink") {
            let link = parse_hyperlink_element(child, rels, graph, footnotes, referenced_footnotes)?;
            paragraph.push_hyperlink(&link, rels, graph);
        }
    }
    Ok(paragraph.inlines)
}

fn parse_run_element(element: &XmlElement) -> RunState {
    let mut run = RunState::default();
    if let Some(r_pr) = child_element(element, "rPr") {
        if let Some(bold) = child_element(r_pr, "b") {
            run.bold = bool_from_element(Some(bold), true);
        }
        if let Some(italic) = child_element(r_pr, "i") {
            run.italic = bool_from_element(Some(italic), true);
        }
        if let Some(underline) = child_element(r_pr, "u") {
            run.underline = attribute_value(underline, "val").map(|value| value != "none").unwrap_or(true);
        }
        if let Some(strike) = child_element(r_pr, "strike") {
            run.strike = bool_from_element(Some(strike), true);
        }
        if let Some(vert_align) = child_element(r_pr, "vertAlign") {
            match attribute_value(vert_align, "val").as_deref() {
                Some("superscript") => run.superscript = true,
                Some("subscript") => run.subscript = true,
                _ => {}
            }
        }
        if let Some(color) = child_element(r_pr, "color") {
            if let Some(value) = attribute_value(color, "val") {
                let value = value.to_ascii_lowercase();
                if value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    run.color = Some(value);
                }
            }
        }
        if let Some(size) = child_element(r_pr, "sz") {
            if let Some(value) = attribute_value(size, "val") {
                if value.parse::<u32>().is_ok_and(|value| value > 0) {
                    run.font_size = Some(value);
                }
            }
        }
        if let Some(fonts) = child_element(r_pr, "rFonts") {
            for attribute in ["ascii", "hAnsi", "eastAsia", "cs"] {
                if let Some(value) = attribute_value(fonts, attribute) {
                    if !value.is_empty() {
                        run.font_family = Some(value);
                        break;
                    }
                }
            }
        }
        if let Some(highlight) = child_element(r_pr, "highlight") {
            if let Some(value) = attribute_value(highlight, "val") {
                if !value.is_empty() && value != "none" {
                    run.highlight = Some(value.to_ascii_lowercase());
                }
            }
        }
    }
    for child in &element.children {
        match child {
            XmlValue::Element(child) if element_is(child, "t") => {
                run.text.push_str(&element_text(child));
            }
            XmlValue::Element(child) if element_is(child, "tab") => run.text.push('\t'),
            XmlValue::Element(child) if element_is(child, "br") => run.text.push('\n'),
            _ => {}
        }
    }
    run
}

#[derive(Debug, Default)]
struct ParagraphState {
    style: Option<String>,
    inlines: Vec<Inline>,
    math_blocks: Vec<Block>,
    pending_image_alt: Option<String>,
    pending_image_title: Option<String>,
    is_list_item: bool,
    num_id: Option<u32>,
    ilvl: Option<u32>,
}

#[derive(Debug, Default)]
struct BodyState {
    pending_list: Option<PendingListState>,
}

#[derive(Debug)]
struct PendingListState {
    num_id: Option<u32>,
    ordered: Option<bool>,
    items: Vec<PendingListItem>,
}

#[derive(Debug)]
struct PendingListItem {
    content: Vec<Inline>,
    level: u32,
}

impl Default for PendingListState {
    fn default() -> Self {
        Self { num_id: None, ordered: None, items: Vec::new() }
    }
}

impl ParagraphState {
    fn push_run(&mut self, run: &RunState) {
        if run.text.is_empty() {
            return;
        }
        let inline = run.inline();
        push_inline(&mut self.inlines, inline);
    }

    fn push_hyperlink(&mut self, link: &HyperlinkState, rels: &HashMap<String, String>, graph: &mut DocumentGraph) {
        if link.inlines.iter().any(is_image_inline) {
            graph.push_loss(LossMarker {
                code: "reader.docx.image_hyperlink".into(),
                message:
                    "hyperlinked images retain their image assets and inline order, but image click targets are not retained"
                        .into(),
                status: SemanticStatus::Partial,
            });
            let mut segment = HyperlinkState { rel_id: link.rel_id.clone(), title: link.title.clone(), inlines: Vec::new() };
            for inline in &link.inlines {
                if is_image_inline(inline) {
                    self.push_hyperlink(&segment, rels, graph);
                    segment.inlines.clear();
                    push_inline(&mut self.inlines, inline.clone());
                }
                else {
                    segment.inlines.push(inline.clone());
                }
            }
            self.push_hyperlink(&segment, rels, graph);
            return;
        }
        if link.inlines.iter().all(|inline| inline.plain_text().is_empty()) {
            return;
        }
        let url = link.rel_id.as_ref().and_then(|id| rels.get(id));
        if let Some(url) = url {
            push_inline(
                &mut self.inlines,
                Inline::Link {
                    target: RelationEndpoint::ExternalUri(url.clone()),
                    display: link.inlines.clone(),
                    title: link.title.clone(),
                },
            );
            return;
        }
        graph.push_loss(LossMarker {
            code: "reader.docx.unresolved_hyperlink".into(),
            message: format!("hyperlink relationship {:?} could not be resolved", link.rel_id),
            status: SemanticStatus::Unresolved,
        });
        for inline in &link.inlines {
            push_inline(&mut self.inlines, inline.clone());
        }
    }

    fn push_image(&mut self, rel_id: &str, rels: &HashMap<String, String>, graph: &mut DocumentGraph) {
        let alt = self.pending_image_alt.take().unwrap_or_default();
        let title = self.pending_image_title.take();
        let target = rels.get(rel_id);
        if let Some(target) = target {
            let asset_id = AssetId(graph.assets.len() as u64 + 1);
            graph.push_asset(Asset {
                id: asset_id,
                kind: AssetKind::Image,
                content_identity: None,
                source: Some(target.clone()),
                media_type: media_type_for(target),
                status: SemanticStatus::Resolved,
                bytes: None,
            });
            push_inline(
                &mut self.inlines,
                Inline::Image {
                    source: target.clone(),
                    alt: if alt.is_empty() { Vec::new() } else { vec![Inline::Text { text: alt }] },
                    title,
                    asset: Some(asset_id),
                },
            );
            return;
        }
        graph.push_loss(LossMarker {
            code: "reader.docx.unresolved_image".into(),
            message: format!("image relationship {rel_id} could not be resolved"),
            status: SemanticStatus::Unresolved,
        });
    }
}

fn is_image_inline(inline: &Inline) -> bool {
    matches!(inline, Inline::Image { .. })
}

#[derive(Debug, Default)]
struct HyperlinkState {
    rel_id: Option<String>,
    title: Option<String>,
    inlines: Vec<Inline>,
}

impl HyperlinkState {
    #[allow(dead_code)]
    fn push_run(&mut self, run: &RunState) {
        if run.text.is_empty() {
            return;
        }
        push_inline(&mut self.inlines, run.inline());
    }
}

#[derive(Debug, Default)]
struct RunState {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    superscript: bool,
    subscript: bool,
    color: Option<String>,
    font_size: Option<String>,
    font_family: Option<String>,
    highlight: Option<String>,
    text: String,
}

impl RunState {
    fn inline(&self) -> Inline {
        let mut inline = Inline::Text { text: self.text.clone() };
        for (enabled, style) in [
            (self.strike, "strike"),
            (self.underline, "underline"),
            (self.superscript, "superscript"),
            (self.subscript, "subscript"),
            (self.italic, "italic"),
            (self.bold, "bold"),
        ] {
            if enabled {
                inline = Inline::Styled { style: style.into(), children: vec![inline] };
            }
        }
        if let Some(color) = &self.color {
            inline = Inline::Styled { style: format!("color:{color}"), children: vec![inline] };
        }
        if let Some(highlight) = &self.highlight {
            inline = Inline::Styled { style: format!("highlight:{highlight}"), children: vec![inline] };
        }
        if let Some(font_family) = &self.font_family {
            inline = Inline::Styled { style: format!("font-family:{font_family}"), children: vec![inline] };
        }
        if let Some(font_size) = &self.font_size {
            inline = Inline::Styled { style: format!("font-size:{font_size}"), children: vec![inline] };
        }
        inline
    }
}

fn push_footnote_reference(
    graph: &mut DocumentGraph,
    inlines: &mut Vec<Inline>,
    footnotes: &FootnoteCatalog,
    referenced: &mut HashSet<u32>,
    id: Option<u32>,
) {
    let label = id.map(|value| value.to_string()).unwrap_or_else(|| "?".to_string());
    push_inline(inlines, Inline::Note { label: label.clone(), display: None });
    if let Some(id) = id {
        referenced.insert(id);
        if !footnotes.contains_key(&id) {
            graph.push_loss(LossMarker {
                code: "reader.docx.footnote_body".into(),
                message: format!("footnote {label} body is not resolved yet"),
                status: SemanticStatus::Unresolved,
            });
        }
    }
    else {
        graph.push_loss(LossMarker {
            code: "reader.docx.footnote_body".into(),
            message: "footnote reference is missing an id".into(),
            status: SemanticStatus::Unresolved,
        });
    }
}

fn push_inline(inlines: &mut Vec<Inline>, inline: Inline) {
    if let Some(last) = inlines.last_mut() {
        if merge_text_inline(last, &inline) {
            return;
        }
    }
    inlines.push(inline);
}

fn merge_text_inline(existing: &mut Inline, incoming: &Inline) -> bool {
    match (existing, incoming) {
        (Inline::Text { text: left }, Inline::Text { text: right }) => {
            left.push_str(right);
            true
        }
        _ => false,
    }
}

fn media_type_for(path: &str) -> Option<String> {
    let extension = path.rsplit('.').next()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png".into()),
        "jpg" | "jpeg" => Some("image/jpeg".into()),
        "gif" => Some("image/gif".into()),
        "webp" => Some("image/webp".into()),
        "svg" => Some("image/svg+xml".into()),
        _ => None,
    }
}

fn finish_paragraph(graph: &mut DocumentGraph, body: &mut BodyState, paragraph: &ParagraphState, numbering: &NumberingCatalog) {
    if paragraph.is_list_item {
        let mut content = paragraph_inlines(paragraph).unwrap_or_default();
        content.extend(math_block_inlines(paragraph));
        if !content.is_empty() {
            let item = PendingListItem { content, level: paragraph.ilvl.unwrap_or(0) };
            let ordered = list_marker_ordered(paragraph, numbering);
            match &mut body.pending_list {
                Some(list) if list.num_id == paragraph.num_id => list.items.push(item),
                Some(_) => {
                    flush_pending_list(graph, body, numbering);
                    body.pending_list = Some(PendingListState { num_id: paragraph.num_id, ordered, items: vec![item] });
                }
                None => {
                    body.pending_list = Some(PendingListState { num_id: paragraph.num_id, ordered, items: vec![item] });
                }
            }
        }
        return;
    }
    flush_pending_list(graph, body, numbering);
    push_paragraph(graph, paragraph);
    for block in &paragraph.math_blocks {
        graph.push_block(block.clone());
    }
}

fn math_block_inlines(paragraph: &ParagraphState) -> Vec<Inline> {
    paragraph
        .math_blocks
        .iter()
        .filter_map(|block| match block {
            Block::Math { formula } => Some(Inline::InlineMath { formula: formula.clone() }),
            _ => None,
        })
        .collect()
}

fn list_marker_ordered(paragraph: &ParagraphState, numbering: &NumberingCatalog) -> Option<bool> {
    match (paragraph.num_id, paragraph.ilvl) {
        (Some(num_id), ilvl) => numbering.is_ordered(num_id, ilvl.unwrap_or(0)),
        _ => None,
    }
}

fn flush_pending_list(graph: &mut DocumentGraph, body: &mut BodyState, numbering: &NumberingCatalog) {
    let Some(list) = body.pending_list.take()
    else {
        return;
    };
    if list.items.is_empty() {
        return;
    }
    let resolved = list.ordered.or_else(|| list.num_id.and_then(|num_id| numbering.is_ordered(num_id, 0)));
    let ordered = resolved.unwrap_or(false);
    if resolved.is_none() {
        graph.push_loss(LossMarker {
            code: "reader.docx.numbering_unresolved".into(),
            message: "list marker style inferred without word/numbering.xml".into(),
            status: SemanticStatus::Partial,
        });
    }
    // The IR list model carries `ordered` plus a sequential number only. Marker
    // formats such as `lowerRoman` or `none` would be rewritten as decimal or
    // bullet markers on export, so record the degradation instead of claiming a
    // clean round-trip.
    if let Some(num_id) = list.num_id {
        let unrepresentable: BTreeSet<&str> = list
            .items
            .iter()
            .filter_map(|item| numbering.marker_format(num_id, item.level))
            .filter(|format| !marker_format_is_representable(format))
            .collect();
        if !unrepresentable.is_empty() {
            graph.push_loss(LossMarker {
                code: "reader.docx.numbering_format".into(),
                message: format!(
                    "list marker formats [{}] do not map to the IR and are written as decimal or bullet markers",
                    unrepresentable.iter().copied().collect::<Vec<_>>().join(", ")
                ),
                status: SemanticStatus::Partial,
            });
        }
    }
    let mut cursor = 0;
    let items = build_nested_items(
        graph,
        &list.items,
        &mut cursor,
        list.items.first().map_or(0, |item| item.level),
        list.num_id,
        ordered,
        numbering,
    );
    graph.push_block(Block::List { ordered, items });
}

fn build_nested_items(
    graph: &mut DocumentGraph,
    pending: &[PendingListItem],
    cursor: &mut usize,
    level: u32,
    num_id: Option<u32>,
    ordered: bool,
    numbering: &NumberingCatalog,
) -> Vec<ListItem> {
    let mut items = Vec::new();
    while *cursor < pending.len() {
        let current = &pending[*cursor];
        if current.level < level {
            break;
        }
        if current.level > level {
            break;
        }
        *cursor += 1;
        // Word derives ordered markers from `w:numbering`; the IR requires an
        // explicit non-zero number for every ordered item, so the sequential
        // position within this level is the resolved marker value.
        let number = ordered.then_some(items.len() as u32 + 1);
        let mut item =
            ListItem { content: current.content.clone(), children: Vec::new(), number, is_task: false, is_checked: None };
        if *cursor < pending.len() && pending[*cursor].level > level {
            let child_level = pending[*cursor].level;
            let child_ordered = num_id.and_then(|id| numbering.is_ordered(id, child_level)).unwrap_or(ordered);
            let child_items = build_nested_items(graph, pending, cursor, child_level, num_id, child_ordered, numbering);
            let child_id = graph.push_block(Block::List { ordered: child_ordered, items: child_items });
            item.children.push(child_id);
        }
        items.push(item);
    }
    items
}

fn paragraph_inlines(paragraph: &ParagraphState) -> Option<Vec<Inline>> {
    if paragraph.inlines.is_empty() {
        return None;
    }
    let trimmed = paragraph.inlines.iter().map(Inline::plain_text).collect::<String>().trim().to_string();
    let has_non_text = paragraph.inlines.iter().any(|inline| !matches!(inline, Inline::Text { .. }));
    if trimmed.is_empty() && !has_non_text {
        return None;
    }
    Some(paragraph.inlines.clone())
}

fn push_paragraph(graph: &mut DocumentGraph, paragraph: &ParagraphState) {
    if let Some(language) = code_language(&paragraph.style) {
        let content = paragraph.inlines.iter().map(Inline::plain_text).collect::<String>();
        graph.push_block(Block::Code { language: Some(language), content });
        return;
    }
    if let Some(language) = diagram_language(&paragraph.style) {
        let source = paragraph.inlines.iter().map(Inline::plain_text).collect::<String>();
        graph.push_block(Block::Diagram { language, source });
        return;
    }
    let Some(inlines) = paragraph_inlines(paragraph)
    else {
        return;
    };
    if let Some(level) = heading_level(&paragraph.style) {
        graph.push_block(Block::Section { level, title: inlines, children: Vec::new() });
        return;
    }
    if paragraph.style.as_deref().is_some_and(|style| style.eq_ignore_ascii_case("Quote")) {
        let child = graph.push_block(Block::Paragraph { content: inlines });
        graph.push_block(Block::Quote { children: vec![child] });
        return;
    }
    if paragraph.style.is_some() {
        graph.push_loss(LossMarker {
            code: "reader.docx.unmapped_style".into(),
            message: format!("paragraph style {:?} mapped to plain text", paragraph.style),
            status: SemanticStatus::Partial,
        });
    }
    graph.push_block(Block::Paragraph { content: inlines });
}

/// Diagram paragraphs carry their language in the paragraph style written by
/// the DOCX exporter, so the block reopens with the same language and source.
fn diagram_language(style: &Option<String>) -> Option<String> {
    let language = style.as_deref()?.strip_prefix("Diagram:")?.trim();
    (!language.is_empty()).then(|| language.to_string())
}

/// Code paragraphs carry their language in the paragraph style written by the
/// DOCX exporter, so a code block reopens with the same language.
fn code_language(style: &Option<String>) -> Option<String> {
    let language = style.as_deref()?.strip_prefix("Code:")?.trim();
    (!language.is_empty()).then(|| language.to_string())
}

fn heading_level(style: &Option<String>) -> Option<u8> {
    let style = style.as_deref()?;
    if let Some(rest) = style.strip_prefix("Heading") {
        if let Ok(level) = rest.parse::<u8>() {
            if (1..=6).contains(&level) {
                return Some(level);
            }
        }
    }
    if style.eq_ignore_ascii_case("Title") {
        return Some(1);
    }
    None
}

/// Creates a new graph with a stable document id derived from the label.
pub fn new_graph(label: &str) -> DocumentGraph {
    DocumentGraph::new(document_id_for(label))
}

fn document_id_for(label: &str) -> DocumentId {
    let mut hash = 1u64;
    for byte in label.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    DocumentId(hash)
}
