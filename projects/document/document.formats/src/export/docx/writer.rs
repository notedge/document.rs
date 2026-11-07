use std::fmt::Write as _;

use std::collections::HashSet;

use document_ir::{
    AssetId, AssetKind, Block, BreakKind, DocumentGraph, Inline, ListItem, MathDelimiter, MathDelimiterKind, MathEnvironment,
    MathEnvironmentKind, MathExpression, MathFractionLine, MathSpace, MathStyle, NodeId, RelationEndpoint, TableRow,
};

use crate::FormatError;

use super::{
    footnotes::{FootnoteCatalog, collect_footnote_catalog, parse_footnote_reference_token, render_footnotes_xml},
    numbering::{ORDERED_NUM_ID, UNORDERED_NUM_ID},
    page::{DocxPageSetup, twips},
    rels::DocumentRelsRegistry,
};

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const M_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

/// Rendered WordprocessingML body and collected OPC relationships.
pub struct DocxRenderOutput {
    pub document_xml: String,
    pub include_numbering: bool,
    pub document_rels_xml: String,
    pub media_parts: Vec<(String, Vec<u8>)>,
    pub image_extensions: Vec<String>,
    pub footnotes_xml: Option<String>,
}

pub fn render_document_xml(graph: &DocumentGraph) -> Result<DocxRenderOutput, FormatError> {
    let validation = graph.validate();
    if !validation.is_valid() {
        return Err(FormatError::invalid_input(format!("invalid DOCX document graph: {:?}", validation.issues)));
    }
    let mut body = String::new();
    let uses_numbering = graph.blocks.iter().any(|node| matches!(node.block, Block::List { .. }));
    let mut rels = DocumentRelsRegistry::default();
    rels.set_footnotes(collect_footnote_catalog(graph));
    let nested_blocks = nested_block_ids(graph);
    for node in &graph.blocks {
        if nested_blocks.contains(&node.id) {
            continue;
        }
        if let Block::Opaque { kind, .. } = &node.block {
            if kind == "footnote_definition" {
                continue;
            }
        }
        write_block(&mut body, &node.block, graph, &mut rels)?;
    }
    body.push_str("<w:sectPr/>");

    let footnotes_xml = if rels.footnotes().is_empty() {
        None
    }
    else {
        rels.ensure_footnotes_rel();
        Some(render_footnotes_xml(rels.footnotes())?)
    };

    Ok(DocxRenderOutput {
        document_xml: format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{W_NS}" xmlns:r="{R_NS}" xmlns:m="{M_NS}">
  <w:body>
    {body}
  </w:body>
</w:document>"#
        ),
        include_numbering: uses_numbering,
        document_rels_xml: rels.render_document_rels_xml(uses_numbering),
        media_parts: rels.media_parts(),
        image_extensions: rels.image_extensions(),
        footnotes_xml,
    })
}

fn write_block(
    out: &mut String,
    block: &Block,
    graph: &DocumentGraph,
    rels: &mut DocumentRelsRegistry,
) -> Result<(), FormatError> {
    match block {
        Block::Paragraph { content } => {
            out.push_str("<w:p>");
            write_inlines(out, content, graph, rels)?;
            out.push_str("</w:p>");
        }
        Block::Section { level, title, children } => {
            let level = (*level).clamp(1, 6);
            write!(out, "<w:p><w:pPr><w:pStyle w:val=\"Heading{level}\"/></w:pPr>").map_err(map_fmt_error)?;
            write_inlines(out, title, graph, rels)?;
            out.push_str("</w:p>");
            for child_id in children {
                let child = graph
                    .block(*child_id)
                    .ok_or_else(|| FormatError::invalid_input(format!("missing section child {child_id:?}")))?;
                write_block(out, &child.block, graph, rels)?;
            }
        }
        Block::Code { content, .. } => {
            // Code has no paragraph structure beyond its text, but the package
            // already declares `VerbatimChar`; referencing it is what makes the
            // monospace face actually apply instead of sitting unused.
            out.push_str("<w:p>");
            write_run_with_properties(out, content, "<w:rStyle w:val=\"VerbatimChar\"/>")?;
            out.push_str("</w:p>");
        }
        Block::Diagram { language, source } => {
            // The diagram language has no WordprocessingML paragraph property,
            // so it travels in the paragraph style and is read back by the
            // importer. The source stays plain text.
            out.push_str("<w:p><w:pPr><w:pStyle w:val=\"Diagram:");
            write_xml_text(out, language)?;
            out.push_str("\"/></w:pPr>");
            write_run_text(out, source)?;
            out.push_str("</w:p>");
        }
        Block::Chart { language, source } => {
            out.push_str("<w:p><w:pPr><w:pStyle w:val=\"Chart:");
            write_xml_text(out, language)?;
            out.push_str("\"/></w:pPr>");
            write_run_text(out, source)?;
            out.push_str("</w:p>");
        }
        Block::Quote { children } => {
            if children.is_empty() {
                out.push_str("<w:p><w:pPr><w:pStyle w:val=\"Quote\"/></w:pPr></w:p>");
            }
            for child_id in children {
                let child = graph
                    .block(*child_id)
                    .ok_or_else(|| FormatError::invalid_input(format!("missing quote child {child_id:?}")))?;
                if let Block::Paragraph { content } = &child.block {
                    out.push_str("<w:p><w:pPr><w:pStyle w:val=\"Quote\"/></w:pPr>");
                    write_inlines(out, content, graph, rels)?;
                    out.push_str("</w:p>");
                }
                else {
                    write_block(out, &child.block, graph, rels)?;
                }
            }
        }
        Block::List { ordered, items } => {
            write_list(out, *ordered, items, graph, rels)?;
        }
        Block::Table { head, body, foot, .. } => {
            write_table(out, head, body, foot, graph, rels)?;
        }
        Block::Math { formula } => {
            out.push_str("<w:p><m:oMathPara><m:oMath>");
            write_math_expression(out, &formula.expression, None)?;
            out.push_str("</m:oMath></m:oMathPara></w:p>");
        }
        Block::Opaque { payload_hint, .. } => {
            // An opaque block owns no WordprocessingML structure, so its payload
            // travels as a plain paragraph. Dropping it would delete authored
            // content, while refusing the export would lose the whole document.
            out.push_str("<w:p>");
            write_literal_text(out, payload_hint, "", rels.footnotes())?;
            out.push_str("</w:p>");
        }
        Block::RawHtml { html } => {
            // Raw markup cannot be interpreted by the DOCX exporter, but its
            // authored text stays visible as a paragraph instead of failing.
            out.push_str("<w:p>");
            write_literal_text(out, html, "", rels.footnotes())?;
            out.push_str("</w:p>");
        }
    }
    Ok(())
}

fn write_list(
    out: &mut String,
    ordered: bool,
    items: &[ListItem],
    graph: &DocumentGraph,
    rels: &mut DocumentRelsRegistry,
) -> Result<(), FormatError> {
    write_list_at_level(out, ordered, items, graph, rels, 0)
}

fn write_list_at_level(
    out: &mut String,
    ordered: bool,
    items: &[ListItem],
    graph: &DocumentGraph,
    rels: &mut DocumentRelsRegistry,
    level: u32,
) -> Result<(), FormatError> {
    if level > 8 {
        return Err(FormatError::unsupported("docx", "list nesting exceeds the nine numbering levels"));
    }
    let num_id = if ordered { ORDERED_NUM_ID } else { UNORDERED_NUM_ID };
    for item in items {
        write!(out, "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num_id}\"/></w:numPr></w:pPr>")
            .map_err(map_fmt_error)?;
        write_inlines(out, &item.content, graph, rels)?;
        out.push_str("</w:p>");
        for child_id in &item.children {
            let Some(child) = graph.block(*child_id)
            else {
                continue;
            };
            if let Block::List { ordered, items } = &child.block {
                write_list_at_level(out, *ordered, items, graph, rels, level + 1)?;
            }
            else {
                write_block(out, &child.block, graph, rels)?;
            }
        }
    }
    Ok(())
}

fn nested_block_ids(graph: &DocumentGraph) -> HashSet<NodeId> {
    let mut ids = HashSet::new();
    for node in &graph.blocks {
        collect_nested_ids(&node.block, &mut ids, graph);
    }
    ids
}

fn collect_nested_ids(block: &Block, ids: &mut HashSet<NodeId>, graph: &DocumentGraph) {
    match block {
        Block::Section { children, .. } => {
            for child in children {
                if ids.insert(*child) {
                    if let Some(node) = graph.block(*child) {
                        collect_nested_ids(&node.block, ids, graph);
                    }
                }
            }
        }
        Block::Quote { children, .. } => {
            for child in children {
                if ids.insert(*child) {
                    if let Some(node) = graph.block(*child) {
                        collect_nested_ids(&node.block, ids, graph);
                    }
                }
            }
        }
        Block::List { items, .. } => {
            for item in items {
                for child in &item.children {
                    if ids.insert(*child) {
                        if let Some(node) = graph.block(*child) {
                            collect_nested_ids(&node.block, ids, graph);
                        }
                    }
                }
            }
        }
        Block::Table { head, body, foot, .. } => {
            for cell in head.iter().chain(body).chain(foot).flat_map(|row| row.cells.iter()) {
                for child in &cell.children {
                    if ids.insert(*child) {
                        if let Some(node) = graph.block(*child) {
                            collect_nested_ids(&node.block, ids, graph);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Where a grid slot came from: the cell at `row`/`cell` that covers it.
#[derive(Clone, Copy)]
struct GridOrigin {
    row: usize,
    cell: usize,
    colspan: usize,
    rowspan: usize,
}

/// Places every cell on the grid its spans declare. Slots covered by a
/// `rowspan` keep pointing at the originating cell, so a later row still
/// writes a continuation cell for them.
fn place_cells(rows: &[TableRow]) -> (Vec<Vec<Option<GridOrigin>>>, usize) {
    let mut slots: Vec<Vec<Option<GridOrigin>>> = vec![Vec::new(); rows.len()];
    let mut columns = 0;
    for (row_index, row) in rows.iter().enumerate() {
        let mut column = 0;
        for (cell_index, cell) in row.cells.iter().enumerate() {
            while column < slots[row_index].len() && slots[row_index][column].is_some() {
                column += 1;
            }
            let colspan = cell.colspan.max(1) as usize;
            let rowspan = cell.rowspan.max(1) as usize;
            let origin = GridOrigin { row: row_index, cell: cell_index, colspan, rowspan };
            for covered_row in row_index..(row_index + rowspan).min(rows.len()) {
                let width = column + colspan;
                if slots[covered_row].len() < width {
                    slots[covered_row].resize(width, None);
                }
                for slot in &mut slots[covered_row][column..width] {
                    if slot.is_none() {
                        *slot = Some(origin);
                    }
                }
            }
            column += colspan;
        }
        columns = columns.max(slots[row_index].len());
    }
    (slots, columns.max(1))
}

fn write_table(
    out: &mut String,
    head: &[TableRow],
    body: &[TableRow],
    foot: &[TableRow],
    graph: &DocumentGraph,
    rels: &mut DocumentRelsRegistry,
) -> Result<(), FormatError> {
    // WordprocessingML has no head/body/foot section element, so the bands are
    // laid out as one continuous row sequence in authored order.
    let rows: Vec<TableRow> = head.iter().chain(body).chain(foot).cloned().collect();
    if rows.is_empty() {
        return Ok(());
    }
    // Word renders a bare `w:tbl` with no borders and autofit widths derived
    // from content. The body declares the text box, so every table spans it
    // exactly with a fixed column grid, hairline borders and explicit cell
    // widths. Spanned cells carry `w:gridSpan` / `w:vMerge`, and a slot covered
    // by a `rowspan` still emits a continuation cell so the grid stays aligned.
    let setup = DocxPageSetup::from_graph(graph);
    let (slots, columns) = place_cells(&rows);
    let total = twips((setup.page_width - setup.margin_left - setup.margin_right).max(1.0));
    let widths = column_widths(total, columns);
    out.push_str("<w:tbl><w:tblPr>");
    let _ = write!(out, "<w:tblW w:w=\"{total}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/>");
    out.push_str("<w:tblBorders>");
    for side in ["top", "left", "bottom", "right", "insideH", "insideV"] {
        let _ = write!(out, "<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    }
    out.push_str("</w:tblBorders></w:tblPr><w:tblGrid>");
    for width in &widths {
        let _ = write!(out, "<w:gridCol w:w=\"{width}\"/>");
    }
    out.push_str("</w:tblGrid>");
    for (row_index, row) in rows.iter().enumerate() {
        out.push_str("<w:tr>");
        let mut column = 0;
        while column < columns {
            let Some(origin) = slots[row_index].get(column).copied().flatten()
            else {
                column += 1;
                continue;
            };
            let end = (column + origin.colspan).min(columns);
            let width: i64 = widths[column..end].iter().sum();
            let _ = write!(out, "<w:tc><w:tcPr><w:tcW w:w=\"{width}\" w:type=\"dxa\"/>");
            if origin.colspan > 1 {
                let _ = write!(out, "<w:gridSpan w:val=\"{}\"/>", origin.colspan);
            }
            if origin.row == row_index {
                if origin.rowspan > 1 {
                    out.push_str("<w:vMerge w:val=\"restart\"/>");
                }
                out.push_str("</w:tcPr>");
                let cell = &row.cells[origin.cell];
                if cell.children.is_empty() {
                    out.push_str("<w:p/>");
                }
                else {
                    for child_id in &cell.children {
                        let Some(child) = graph.block(*child_id)
                        else {
                            return Err(FormatError::invalid_input(format!("table cell child `{child_id:?}` is missing")));
                        };
                        write_block(out, &child.block, graph, rels)?;
                    }
                }
                out.push_str("</w:tc>");
            }
            else {
                out.push_str("<w:vMerge/></w:tcPr><w:p/></w:tc>");
            }
            column += origin.colspan;
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    Ok(())
}

/// Split the text box width into equal columns, giving the remainder to the
/// leading ones so the grid columns always sum to the table width.
fn column_widths(total: i64, columns: usize) -> Vec<i64> {
    let base = total / columns as i64;
    let remainder = total - base * columns as i64;
    (0..columns).map(|index| base + i64::from((index as i64) < remainder)).collect()
}

fn write_inlines(
    out: &mut String,
    inlines: &[Inline],
    graph: &DocumentGraph,
    rels: &mut DocumentRelsRegistry,
) -> Result<(), FormatError> {
    for inline in inlines {
        write_inline(out, inline, graph, rels)?;
    }
    Ok(())
}

fn write_inline(
    out: &mut String,
    inline: &Inline,
    graph: &DocumentGraph,
    rels: &mut DocumentRelsRegistry,
) -> Result<(), FormatError> {
    write_inline_with_properties(out, inline, graph, rels, "")
}

fn write_inline_with_properties(
    out: &mut String,
    inline: &Inline,
    graph: &DocumentGraph,
    rels: &mut DocumentRelsRegistry,
    properties: &str,
) -> Result<(), FormatError> {
    match inline {
        Inline::Text { text } => {
            if properties.is_empty() {
                write_text_with_footnotes(out, text, rels.footnotes())?;
            }
            else {
                write_run_with_properties(out, text, properties)?;
            }
        }
        Inline::Styled { style, children } => match style.as_str() {
            style if style.strip_prefix("color:").is_some() => {
                let value = style.strip_prefix("color:").unwrap_or_default();
                if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(FormatError::unsupported("docx", format!("invalid color style `{style}`")));
                }
                let property = format!("<w:color w:val=\"{value}\"/>");
                let combined =
                    if properties.contains(&property) { properties.to_owned() } else { format!("{properties}{property}") };
                for child in children {
                    write_inline_with_properties(out, child, graph, rels, &combined)?;
                }
            }
            style if style.strip_prefix("font-size:").is_some() => {
                let value = style.strip_prefix("font-size:").unwrap_or_default();
                if value.is_empty() || !value.parse::<u32>().map(|value| value > 0).unwrap_or(false) {
                    return Err(FormatError::unsupported("docx", format!("invalid font size style `{style}`")));
                }
                let property = format!("<w:sz w:val=\"{value}\"/><w:szCs w:val=\"{value}\"/>");
                let combined = format!("{properties}{property}");
                for child in children {
                    write_inline_with_properties(out, child, graph, rels, &combined)?;
                }
            }
            style if style.strip_prefix("font-family:").is_some() => {
                let value = style.strip_prefix("font-family:").unwrap_or_default();
                if value.is_empty() || value.chars().any(|character| character.is_control()) {
                    return Err(FormatError::unsupported("docx", format!("invalid font family style `{style}`")));
                }
                let escaped = escape_xml_attr(value);
                let property = format!("<w:rFonts w:ascii=\"{escaped}\" w:hAnsi=\"{escaped}\"/>");
                let combined = format!("{properties}{property}");
                for child in children {
                    write_inline_with_properties(out, child, graph, rels, &combined)?;
                }
            }
            style if style.strip_prefix("highlight:").is_some() => {
                let value = style.strip_prefix("highlight:").unwrap_or_default();
                if value.is_empty()
                    || value.chars().any(|character| character.is_control() || matches!(character, '<' | '>' | '"' | '\''))
                {
                    return Err(FormatError::unsupported("docx", format!("invalid highlight style `{style}`")));
                }
                let property = format!("<w:highlight w:val=\"{}\"/>", escape_xml_attr(value));
                let combined = format!("{properties}{property}");
                for child in children {
                    write_inline_with_properties(out, child, graph, rels, &combined)?;
                }
            }
            "bold" | "strong" | "italic" | "underline" | "strike" | "superscript" | "subscript" => {
                let property = match style.as_str() {
                    "bold" | "strong" => "<w:b/>",
                    "italic" => "<w:i/>",
                    "underline" => "<w:u w:val=\"single\"/>",
                    "strike" => "<w:strike/>",
                    "superscript" => "<w:vertAlign w:val=\"superscript\"/>",
                    "subscript" => "<w:vertAlign w:val=\"subscript\"/>",
                    _ => unreachable!(),
                };
                let combined =
                    if properties.contains(property) { properties.to_owned() } else { format!("{properties}{property}") };
                for child in children {
                    write_inline_with_properties(out, child, graph, rels, &combined)?;
                }
            }
            other => {
                return Err(FormatError::unsupported(
                    "docx",
                    format!("inline style `{other}` is not supported by the conservative DOCX exporter yet"),
                ));
            }
        },
        Inline::InlineCode { text } => {
            write_run_with_properties(out, text, &format!("{properties}<w:rStyle w:val=\"VerbatimChar\"/>"))?;
        }
        Inline::InlineMath { formula } => {
            out.push_str("<m:oMath>");
            write_math_expression(out, &formula.expression, None)?;
            out.push_str("</m:oMath>");
        }
        Inline::Reference { display, .. } => {
            // The referenced target has no DOCX anchor to point at, so the
            // authored display text stays readable in place.
            write_literal_text(out, display, properties, rels.footnotes())?;
        }
        Inline::Citation { key, display } => {
            // A Word citation field keeps the key in the document while the
            // field result keeps the authored display. Word readers see the
            // display text, and the semantic key survives in `w:instr`.
            write!(out, "<w:fldSimple w:instr=\" CITATION {} \">", escape_xml_attr(key)).map_err(map_fmt_error)?;
            if let Some(display) = display {
                for child in display {
                    write_inline_with_properties(out, child, graph, rels, properties)?;
                }
            }
            else {
                write_literal_text(out, &format!("[{key}]"), properties, rels.footnotes())?;
            }
            out.push_str("</w:fldSimple>");
        }
        Inline::Attributed { children, .. } => {
            for child in children {
                write_inline_with_properties(out, child, graph, rels, properties)?;
            }
        }
        Inline::RawHtml { html } => {
            write_literal_text(out, html, properties, rels.footnotes())?;
        }
        Inline::Link { target, display, title } => {
            let RelationEndpoint::ExternalUri(url) = target
            else {
                return Err(FormatError::unsupported(
                    "docx",
                    format!("link target `{target:?}` has no DOCX hyperlink mapping"),
                ));
            };
            let rel_id = rels.id_for_hyperlink(url);
            let tooltip = title
                .as_deref()
                .filter(|tooltip| !tooltip.is_empty())
                .map(|tooltip| format!(" w:tooltip=\"{}\"", escape_xml_attr(tooltip)))
                .unwrap_or_default();
            write!(out, "<w:hyperlink r:id=\"{rel_id}\"{tooltip}>").map_err(map_fmt_error)?;
            for child in display {
                write_inline_with_properties(out, child, graph, rels, properties)?;
            }
            out.push_str("</w:hyperlink>");
        }
        Inline::Image { source, alt, title, asset } => {
            let alt = alt.iter().map(inline_plain_text).collect::<String>();
            match image_bytes_for_inline(graph, source, *asset) {
                Some(bytes) => {
                    let extent = image_extent(&bytes)?;
                    let rel_id = rels.id_for_image(source, bytes);
                    write_image_drawing(out, &rel_id, &alt, title.as_deref(), extent)?;
                }
                None => {
                    // A picture without materialized bytes cannot become a
                    // package part. It stays visible as an explicit marker that
                    // names the alternative text and the unresolved location, so
                    // the reader sees the gap instead of the export failing.
                    let label = if alt.trim().is_empty() { source.trim() } else { alt.trim() };
                    write_run_text(out, &format!("[image unavailable: {label} ({source})]"))?;
                }
            }
        }
        Inline::Note { label, display: _ } => {
            // WordprocessingML addresses footnotes by integer id only, so a
            // named label resolves through the catalog that assigned it one.
            // A reference without any definition keeps its authored token as
            // visible text instead of failing the whole document.
            match resolve_footnote_reference(label, rels.footnotes()) {
                Some(id) => write_footnote_reference(out, id)?,
                None => write_run_text(out, &format!("[^{label}]"))?,
            }
        }
        Inline::Break { kind } => match kind {
            BreakKind::Hard => {
                write!(out, "<w:r>{properties}<w:br/></w:r>").map_err(map_fmt_error)?;
            }
            BreakKind::Soft => write_run_with_properties(out, " ", properties)?,
        },
    }
    Ok(())
}

/// Lower one semantic math expression to OMML.
///
/// The exporter renders the IR expression instead of re-parsing the authored
/// source. `face` carries the enclosing [`MathStyle`] down to the runs, so a
/// styled body such as `\mathbf{x}` becomes `m:sty` or `m:scr` run properties.
/// Fragments the lowering marked as unsupported keep their authored text inside
/// a math run, so the formula stays visible without inventing structure.
fn write_math_expression(out: &mut String, expression: &MathExpression, face: Option<MathStyle>) -> Result<(), FormatError> {
    match expression {
        MathExpression::Sequence(items) => {
            for item in items {
                write_math_expression(out, item, face)?;
            }
        }
        MathExpression::Atom(atom) => write_math_run(out, &atom.text, face)?,
        MathExpression::Number { text, .. } => write_math_run(out, text, face)?,
        MathExpression::Text { text, .. } => write_math_run_normal(out, text)?,
        MathExpression::Group { body } => write_math_expression(out, body, face)?,
        MathExpression::Script { base, superscript, subscript } => match (superscript, subscript) {
            (Some(superscript), Some(subscript)) => {
                out.push_str("<m:sSubSup><m:e>");
                write_math_expression(out, base, face)?;
                out.push_str("</m:e><m:sub>");
                write_math_expression(out, subscript, face)?;
                out.push_str("</m:sub><m:sup>");
                write_math_expression(out, superscript, face)?;
                out.push_str("</m:sup></m:sSubSup>");
            }
            (Some(superscript), None) => {
                out.push_str("<m:sSup><m:e>");
                write_math_expression(out, base, face)?;
                out.push_str("</m:e><m:sup>");
                write_math_expression(out, superscript, face)?;
                out.push_str("</m:sup></m:sSup>");
            }
            (None, Some(subscript)) => {
                out.push_str("<m:sSub><m:e>");
                write_math_expression(out, base, face)?;
                out.push_str("</m:e><m:sub>");
                write_math_expression(out, subscript, face)?;
                out.push_str("</m:sub></m:sSub>");
            }
            (None, None) => write_math_expression(out, base, face)?,
        },
        MathExpression::Fraction { numerator, denominator, line } => {
            out.push_str("<m:f>");
            if *line == MathFractionLine::None {
                out.push_str("<m:fPr><m:fType m:val=\"noBar\"/></m:fPr>");
            }
            out.push_str("<m:num>");
            write_math_expression(out, numerator, face)?;
            out.push_str("</m:num><m:den>");
            write_math_expression(out, denominator, face)?;
            out.push_str("</m:den></m:f>");
        }
        MathExpression::Radical { index, radicand, .. } => {
            out.push_str("<m:rad>");
            match index {
                Some(index) => {
                    out.push_str("<m:deg>");
                    write_math_expression(out, index, face)?;
                    out.push_str("</m:deg>");
                }
                None => out.push_str("<m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/>"),
            }
            out.push_str("<m:e>");
            write_math_expression(out, radicand, face)?;
            out.push_str("</m:e></m:rad>");
        }
        MathExpression::Delimited { open, close, body, middles } => {
            out.push_str("<m:d><m:dPr><m:begChr m:val=\"");
            write_xml_text(out, &delimiter_char(open.as_ref(), true))?;
            out.push_str("\"/><m:endChr m:val=\"");
            write_xml_text(out, &delimiter_char(close.as_ref(), false))?;
            out.push_str("\"/></m:dPr><m:e>");
            write_math_expression(out, body, face)?;
            out.push_str("</m:e>");
            for middle in middles {
                out.push_str("<m:e>");
                write_math_expression(out, middle, face)?;
                out.push_str("</m:e>");
            }
            out.push_str("</m:d>");
        }
        MathExpression::Styled { style, body } => write_math_expression(out, body, Some(*style))?,
        MathExpression::Accent { mark, base, .. } => {
            out.push_str("<m:acc><m:accPr><m:chr m:val=\"");
            write_xml_text(out, &mark.to_string())?;
            out.push_str("\"/></m:accPr><m:e>");
            write_math_expression(out, base, face)?;
            out.push_str("</m:e></m:acc>");
        }
        MathExpression::StackAbove { over, base } => {
            out.push_str("<m:limUpp><m:e>");
            write_math_expression(out, base, face)?;
            out.push_str("</m:e><m:lim>");
            write_math_expression(out, over, face)?;
            out.push_str("</m:lim></m:limUpp>");
        }
        MathExpression::StackBelow { under, base } => {
            out.push_str("<m:limLow><m:e>");
            write_math_expression(out, base, face)?;
            out.push_str("</m:e><m:lim>");
            write_math_expression(out, under, face)?;
            out.push_str("</m:lim></m:limLow>");
        }
        MathExpression::Relation(body) => write_math_expression(out, body, face)?,
        MathExpression::Space { kind } => write_math_run(out, math_space_text(*kind), None)?,
        MathExpression::StyleSwitch { .. } | MathExpression::LimitsPolicy { .. } => {}
        MathExpression::Environment(environment) => write_math_environment(out, environment, face)?,
        MathExpression::Unsupported { source, .. } => write_math_run(out, source, None)?,
    }
    Ok(())
}

fn write_math_run(out: &mut String, text: &str, face: Option<MathStyle>) -> Result<(), FormatError> {
    if text.is_empty() {
        return Ok(());
    }
    out.push_str("<m:r>");
    if let Some(style) = face {
        let (element, value) = math_face(style);
        out.push_str("<m:rPr><");
        out.push_str(element);
        out.push_str(" m:val=\"");
        out.push_str(value);
        out.push_str("\"/></m:rPr>");
    }
    write_math_text(out, text)
}

/// Text authored inside `\text{...}` is upright prose, so OMML marks it as
/// normal text instead of math italics.
fn write_math_run_normal(out: &mut String, text: &str) -> Result<(), FormatError> {
    if text.is_empty() {
        return Ok(());
    }
    out.push_str("<m:r><m:rPr><m:nor/></m:rPr>");
    write_math_text(out, text)
}

fn write_math_text(out: &mut String, text: &str) -> Result<(), FormatError> {
    out.push_str("<m:t");
    if text.chars().any(char::is_whitespace) {
        out.push_str(" xml:space=\"preserve\"");
    }
    out.push('>');
    write_xml_text(out, text)?;
    out.push_str("</m:t></m:r>");
    Ok(())
}

/// OMML run properties for a semantic math face.
fn math_face(style: MathStyle) -> (&'static str, &'static str) {
    match style {
        MathStyle::Upright => ("m:sty", "p"),
        MathStyle::Italic => ("m:sty", "i"),
        MathStyle::Bold => ("m:sty", "b"),
        MathStyle::DoubleStruck => ("m:scr", "double-struck"),
        MathStyle::Script => ("m:scr", "script"),
        MathStyle::Fraktur => ("m:scr", "fraktur"),
        MathStyle::SansSerif => ("m:scr", "sans-serif"),
        MathStyle::Monospace => ("m:scr", "monospace"),
    }
}

fn math_space_text(kind: MathSpace) -> &'static str {
    match kind {
        MathSpace::Thin => "\u{2009}",
        MathSpace::Medium => "\u{2005}",
        MathSpace::Thick => "\u{2004}",
        MathSpace::Interword => " ",
        MathSpace::Quad => "\u{2003}",
        MathSpace::DoubleQuad => "\u{2003}\u{2003}",
        MathSpace::NegativeThin => "",
    }
}

fn write_math_environment(out: &mut String, environment: &MathEnvironment, face: Option<MathStyle>) -> Result<(), FormatError> {
    let cases = environment.kind == MathEnvironmentKind::Cases;
    if cases {
        out.push_str("<m:d><m:dPr><m:begChr m:val=\"{\"/><m:endChr m:val=\"\"/></m:dPr><m:e>");
    }
    if is_equation_array(environment) {
        // A row-per-equation environment is an equation array, not a matrix:
        // each row keeps one `m:e` and the rows stack without a cell grid.
        out.push_str("<m:eqArr>");
        for row in &environment.rows {
            out.push_str("<m:e>");
            for cell in &row.cells {
                write_math_expression(out, cell, face)?;
            }
            out.push_str("</m:e>");
        }
        out.push_str("</m:eqArr>");
    }
    else {
        out.push_str("<m:m>");
        write_matrix_alignment(out, &environment.column_alignment);
        for row in &environment.rows {
            out.push_str("<m:mr>");
            for cell in &row.cells {
                out.push_str("<m:e>");
                write_math_expression(out, cell, face)?;
                out.push_str("</m:e>");
            }
            out.push_str("</m:mr>");
        }
        out.push_str("</m:m>");
    }
    if cases {
        out.push_str("</m:e></m:d>");
    }
    Ok(())
}

/// Environments whose rows are single expressions stack as `m:eqArr`.
///
/// `Matrix` and `Gathered` keep the cell grid, and `Cases` keeps its brace,
/// so only the row-per-equation families qualify.
fn is_equation_array(environment: &MathEnvironment) -> bool {
    if matches!(environment.kind, MathEnvironmentKind::Matrix | MathEnvironmentKind::Cases | MathEnvironmentKind::Gathered) {
        return false;
    }
    !environment.rows.is_empty() && environment.rows.iter().all(|row| row.cells.len() == 1)
}

/// Declare the per-column justification an `m:m` carries.
///
/// OMML groups consecutive columns with the same justification into one
/// `m:mc` whose `m:count` repeats that run. `Spread` has no OMML counterpart
/// (it is a per-row placement, not a column justification), so an environment
/// that asks for it keeps the default centred columns instead of inventing a
/// justification.
fn write_matrix_alignment(out: &mut String, alignments: &[document_ir::MathColumnAlignment]) {
    use document_ir::MathColumnAlignment;
    let mut groups: Vec<(&'static str, usize)> = Vec::new();
    for alignment in alignments {
        let value = match alignment {
            MathColumnAlignment::Left => "left",
            MathColumnAlignment::Center => "center",
            MathColumnAlignment::Right => "right",
            MathColumnAlignment::Spread => return,
        };
        match groups.last_mut() {
            Some((last, count)) if *last == value => *count += 1,
            _ => groups.push((value, 1)),
        }
    }
    if groups.is_empty() {
        return;
    }
    out.push_str("<m:mPr><m:mcs>");
    for (value, count) in groups {
        let _ = write!(out, "<m:mc><m:mcPr><m:count m:val=\"{count}\"/><m:mcJc m:val=\"{value}\"/></m:mcPr></m:mc>");
    }
    out.push_str("</m:mcs></m:mPr>");
}

fn delimiter_char(delimiter: Option<&MathDelimiter>, open: bool) -> String {
    let Some(delimiter) = delimiter
    else {
        return String::new();
    };
    if let Some(character) = delimiter.character {
        if character != '.' {
            return character.to_string();
        }
    }
    match delimiter.kind {
        MathDelimiterKind::None => String::new(),
        MathDelimiterKind::Parenthesis => (if open { "(" } else { ")" }).to_string(),
        MathDelimiterKind::Bracket => (if open { "[" } else { "]" }).to_string(),
        MathDelimiterKind::Brace => (if open { "{" } else { "}" }).to_string(),
        MathDelimiterKind::Angle => (if open { "⟨" } else { "⟩" }).to_string(),
        MathDelimiterKind::VerticalBar => "|".to_string(),
        MathDelimiterKind::DoubleVerticalBar => "‖".to_string(),
        MathDelimiterKind::Slash => "/".to_string(),
        MathDelimiterKind::Other => delimiter.character.map(|character| character.to_string()).unwrap_or_default(),
    }
}

fn write_image_drawing(
    out: &mut String,
    rel_id: &str,
    alt: &str,
    title: Option<&str>,
    extent: (u64, u64),
) -> Result<(), FormatError> {
    let (width, height) = extent;
    let title_attribute = title
        .filter(|title| !title.is_empty())
        .map(|title| format!(" title=\"{}\"", escape_xml_attr(title)))
        .unwrap_or_default();
    write!(
        out,
        r#"<w:r><w:drawing><wp:inline xmlns:wp="{WP_NS}" xmlns:a="{A_NS}" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><wp:extent cx="{width}" cy="{height}"/><wp:docPr id="1" name="Picture" descr="{descr}"{title_attribute}/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="{descr}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rel_id}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{width}" cy="{height}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#,
        descr = escape_xml_attr(alt),
        rel_id = rel_id,
    )
    .map_err(map_fmt_error)?;
    Ok(())
}

fn image_extent(bytes: &[u8]) -> Result<(u64, u64), FormatError> {
    let dimensions = if bytes.starts_with(b"\x89PNG") {
        png_dimensions(bytes)
    }
    else if bytes.starts_with(&[0xff, 0xd8]) {
        jpeg_dimensions(bytes)
    }
    else {
        return Ok((4_680_000, 3_000_000));
    };
    let (pixel_width, pixel_height) = dimensions
        .filter(|(width, height)| *width > 0 && *height > 0)
        .ok_or_else(|| FormatError::unsupported("docx", "image has invalid or unsupported PNG/JPEG dimensions"))?;
    let natural_width = u64::from(pixel_width) * 9_525;
    let natural_height = u64::from(pixel_height) * 9_525;
    let scale = (4_680_000.0 / natural_width as f64).min(3_000_000.0 / natural_height as f64).min(1.0);
    Ok(((natural_width as f64 * scale).round().max(1.0) as u64, (natural_height as f64 * scale).round().max(1.0) as u64))
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.get(8..12)? != 13u32.to_be_bytes()
        || bytes.get(12..16)? != b"IHDR"
        || bytes.len() < 33
    {
        return None;
    }
    Some((u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?), u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?)))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut cursor = 2;
    while cursor + 3 < bytes.len() {
        if bytes[cursor] != 0xff {
            return None;
        }
        while bytes.get(cursor) == Some(&0xff) {
            cursor += 1;
        }
        let marker = *bytes.get(cursor)?;
        cursor += 1;
        if marker == 0x01 {
            continue;
        }
        if matches!(marker, 0xd0..=0xda | 0x00) {
            return None;
        }
        let length = u16::from_be_bytes(bytes.get(cursor..cursor + 2)?.try_into().ok()?) as usize;
        if length < 2 || cursor.checked_add(length)? > bytes.len() {
            return None;
        }
        if matches!(marker, 0xc0 | 0xc1 | 0xc2) {
            if length < 8 || bytes[cursor + 2] != 8 {
                return None;
            }
            let components = usize::from(bytes[cursor + 7]);
            if components == 0 || length != 8 + components * 3 {
                return None;
            }
            let height = u16::from_be_bytes(bytes.get(cursor + 3..cursor + 5)?.try_into().ok()?) as u32;
            let width = u16::from_be_bytes(bytes.get(cursor + 5..cursor + 7)?.try_into().ok()?) as u32;
            return Some((width, height));
        }
        cursor += length;
    }
    None
}

/// Resolve image bytes through the bound asset identity first, then the
/// location.
///
/// A bound asset keeps its bytes when the storage location is renamed, so the
/// identity has to win over the authored `source` string.
fn image_bytes_for_inline(graph: &DocumentGraph, source: &str, asset: Option<AssetId>) -> Option<Vec<u8>> {
    if let Some(bytes) = asset
        .and_then(|asset_id| graph.assets.iter().find(|candidate| candidate.id == asset_id))
        .and_then(|asset| asset.bytes.clone())
    {
        return Some(bytes);
    }
    image_bytes_for_target(graph, source)
}

fn image_bytes_for_target(graph: &DocumentGraph, target: &str) -> Option<Vec<u8>> {
    graph
        .assets
        .iter()
        .find(|asset| asset.kind == AssetKind::Image && asset.source.as_deref() == Some(target))
        .and_then(|asset| asset.bytes.clone())
}

/// Writes one authored string as run content, honoring accumulated run
/// properties and keeping inline footnote tokens in text without properties.
fn write_literal_text(out: &mut String, text: &str, properties: &str, notes: &FootnoteCatalog) -> Result<(), FormatError> {
    if properties.is_empty() {
        write_text_with_footnotes(out, text, notes)
    }
    else {
        write_run_with_properties(out, text, properties)
    }
}

fn write_text_with_footnotes(out: &mut String, text: &str, notes: &FootnoteCatalog) -> Result<(), FormatError> {
    if let Some(id) = resolve_footnote_token(text, notes) {
        return write_footnote_reference(out, id);
    }

    let mut rest = text;
    while !rest.is_empty() {
        let Some(start) = rest.find("[^")
        else {
            write_run_text(out, rest)?;
            break;
        };
        if start > 0 {
            write_run_text(out, &rest[..start])?;
        }
        let token_start = &rest[start..];
        if let Some(end) = token_start.find(']') {
            let token = &token_start[..=end];
            if let Some(id) = resolve_footnote_token(token, notes) {
                write_footnote_reference(out, id)?;
                rest = &token_start[end + 1..];
                continue;
            }
        }
        // The token names no footnote, so the opening bracket stays as text and
        // scanning resumes after it instead of swallowing the remaining text.
        write_run_text(out, "[^")?;
        rest = &token_start[2..];
    }
    Ok(())
}

fn resolve_footnote_token(token: &str, notes: &FootnoteCatalog) -> Option<u32> {
    let trimmed = token.trim();
    if !trimmed.starts_with("[^") || !trimmed.ends_with(']') {
        return None;
    }
    resolve_footnote_reference(trimmed[2..trimmed.len() - 1].trim(), notes)
}

fn resolve_footnote_reference(label: &str, notes: &FootnoteCatalog) -> Option<u32> {
    notes.id(label).or_else(|| parse_footnote_reference_token(&format!("[^{label}]")))
}

fn write_run_text(out: &mut String, text: &str) -> Result<(), FormatError> {
    write_run_with_properties(out, text, "")
}

fn write_run_with_properties(out: &mut String, text: &str, properties: &str) -> Result<(), FormatError> {
    if text.is_empty() {
        return Ok(());
    }
    out.push_str("<w:r>");
    if !properties.is_empty() {
        write!(out, "<w:rPr>{properties}</w:rPr>").map_err(map_fmt_error)?;
    }
    let mut segment_start = 0;
    let mut characters = text.char_indices().peekable();
    while let Some((position, character)) = characters.next() {
        if !matches!(character, '\n' | '\r' | '\t') {
            continue;
        }
        if segment_start < position {
            out.push_str("<w:t xml:space=\"preserve\">");
            write_xml_text(out, &text[segment_start..position])?;
            out.push_str("</w:t>");
        }
        out.push_str(if character == '\t' { "<w:tab/>" } else { "<w:br/>" });
        segment_start = position + character.len_utf8();
        if character == '\r' && characters.peek().is_some_and(|(_, next)| *next == '\n') {
            let (next_position, _) = characters.next().expect("peeked line feed");
            segment_start = next_position + 1;
        }
    }
    if segment_start < text.len() {
        out.push_str("<w:t xml:space=\"preserve\">");
        write_xml_text(out, &text[segment_start..])?;
        out.push_str("</w:t>");
    }
    out.push_str("</w:r>");
    Ok(())
}

fn write_footnote_reference(out: &mut String, id: u32) -> Result<(), FormatError> {
    write!(out, "<w:r><w:footnoteReference w:id=\"{id}\"/></w:r>").map_err(map_fmt_error)?;
    Ok(())
}

fn write_xml_text(out: &mut String, text: &str) -> Result<(), FormatError> {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    Ok(())
}

fn inline_plain_text(inline: &Inline) -> String {
    inline.plain_text()
}

fn map_fmt_error(error: std::fmt::Error) -> FormatError {
    FormatError::unsupported("docx", error.to_string())
}

fn escape_xml_attr(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '"' => output.push_str("&quot;"),
            _ => output.push(ch),
        }
    }
    output
}
