use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
};

use document_ir::{
    Attributes, Block, BreakKind, DocumentGraph, Inline, ListItem, LossMarker, NodeId, RelationEndpoint, TableCell, TableRow,
};

use crate::FormatError;

/// Markdown text plus the semantics the export explicitly declares as lost.
///
/// The channel is not a diagnostic sink. Only codes written down here, at a
/// known degradation point, may be pushed. No Markdown code is declared yet,
/// so `losses` is empty; the convenience string APIs below discard it.
pub struct MarkdownExport {
    /// The serialized Markdown text.
    pub text: String,
    /// Declared losses, one marker per known degradation.
    pub losses: Vec<LossMarker>,
}

/// Export `notedown-ir` to Markdown text.
pub fn export_markdown(graph: &DocumentGraph) -> Result<String, FormatError> {
    Ok(export_markdown_report(graph)?.text)
}

/// Export `notedown-ir` to Markdown text and report declared losses.
pub fn export_markdown_report(graph: &DocumentGraph) -> Result<MarkdownExport, FormatError> {
    export_markdown_report_with_image_urls_for_roots(graph, None, None)
}

/// Export Markdown while rewriting inline image destinations through
/// `image_urls`.
pub fn export_markdown_with_image_urls(
    graph: &DocumentGraph,
    image_urls: Option<&HashMap<String, String>>,
) -> Result<String, FormatError> {
    Ok(export_markdown_report_with_image_urls_for_roots(graph, None, image_urls)?.text)
}

/// Export Markdown for selected top-level block roots.
pub fn export_markdown_with_image_urls_for_roots(
    graph: &DocumentGraph,
    roots: Option<&[NodeId]>,
    image_urls: Option<&HashMap<String, String>>,
) -> Result<String, FormatError> {
    Ok(export_markdown_report_with_image_urls_for_roots(graph, roots, image_urls)?.text)
}

/// Export Markdown for selected roots and report declared losses.
pub fn export_markdown_report_with_image_urls_for_roots(
    graph: &DocumentGraph,
    roots: Option<&[NodeId]>,
    image_urls: Option<&HashMap<String, String>>,
) -> Result<MarkdownExport, FormatError> {
    let validation = graph.validate();
    if !validation.is_valid() {
        return Err(FormatError::invalid_input(format!("invalid document graph: {:?}", validation.issues)));
    }

    let root_filter = roots.map(|items| items.iter().copied().collect::<HashSet<_>>());
    let mut output = String::new();
    let mut footnotes = Vec::new();
    let nested = nested_block_ids(graph);
    let mut written_roots = Vec::new();
    for node in &graph.blocks {
        if nested.contains(&node.id) {
            continue;
        }
        if let Some(filter) = &root_filter {
            if !filter.contains(&node.id) {
                continue;
            }
        }
        written_roots.push(node.id);
        if let Block::Opaque { kind, payload_hint, .. } = &node.block {
            if kind == "footnote_definition" {
                footnotes.push(payload_hint.as_str());
                continue;
            }
        }
        write_block(&mut output, graph, &node.block, image_urls)?;
    }
    for footnote in footnotes {
        output.push_str(footnote);
        output.push_str("\n\n");
    }
    for (label, display) in synthesized_footnotes(graph, &written_roots) {
        let mut body = String::new();
        write_inlines(&mut body, &display, image_urls)?;
        let body = body.replace('\n', "\n    ");
        output.push_str("[^");
        output.push_str(&label);
        output.push_str("]: ");
        output.push_str(&body);
        output.push_str("\n\n");
    }
    Ok(MarkdownExport { text: output, losses: Vec::new() })
}

/// Definitions already authored as opaque blocks, keyed by their GFM label.
fn authored_footnote_labels(graph: &DocumentGraph) -> HashSet<String> {
    graph
        .blocks
        .iter()
        .filter_map(|node| match &node.block {
            Block::Opaque { kind, payload_hint, .. } if kind == "footnote_definition" => {
                footnote_definition_label(payload_hint).map(str::to_string)
            }
            _ => None,
        })
        .collect()
}

fn footnote_definition_label(payload: &str) -> Option<&str> {
    payload.trim_start().strip_prefix("[^")?.split_once("]:").map(|(label, _)| label)
}

/// Synthesizes GFM definitions for note references that carry authored display
/// content, which is how Pandoc notes arrive. A label is defined once, first
/// occurrence wins.
fn synthesized_footnotes(graph: &DocumentGraph, roots: &[NodeId]) -> Vec<(String, Vec<Inline>)> {
    let authored = authored_footnote_labels(graph);
    let mut seen = authored;
    let mut definitions = Vec::new();
    for id in roots {
        collect_block_footnotes(graph, *id, &mut seen, &mut definitions);
    }
    definitions
}

fn collect_block_footnotes(
    graph: &DocumentGraph,
    id: NodeId,
    seen: &mut HashSet<String>,
    definitions: &mut Vec<(String, Vec<Inline>)>,
) {
    let Some(node) = graph.block(id)
    else {
        return;
    };
    match &node.block {
        Block::Paragraph { content } => collect_inline_footnotes(content, seen, definitions),
        Block::Section { title, children, .. } => {
            collect_inline_footnotes(title, seen, definitions);
            for child in children {
                collect_block_footnotes(graph, *child, seen, definitions);
            }
        }
        Block::Quote { children } => {
            for child in children {
                collect_block_footnotes(graph, *child, seen, definitions);
            }
        }
        Block::List { items, .. } => {
            for item in items {
                collect_inline_footnotes(&item.content, seen, definitions);
                for child in &item.children {
                    collect_block_footnotes(graph, *child, seen, definitions);
                }
            }
        }
        Block::Table { head, body, foot, .. } => {
            for cell in head.iter().chain(body).chain(foot).flat_map(|row| row.cells.iter()) {
                for child in &cell.children {
                    collect_block_footnotes(graph, *child, seen, definitions);
                }
            }
        }
        _ => {}
    }
}

fn collect_inline_footnotes(inlines: &[Inline], seen: &mut HashSet<String>, definitions: &mut Vec<(String, Vec<Inline>)>) {
    for inline in inlines {
        match inline {
            Inline::Note { label, display } => {
                if let Some(display) = display {
                    if !display.is_empty() && seen.insert(label.clone()) {
                        definitions.push((label.clone(), display.clone()));
                    }
                    collect_inline_footnotes(display, seen, definitions);
                }
            }
            Inline::Styled { children, .. } | Inline::Attributed { children, .. } | Inline::Link { display: children, .. } => {
                collect_inline_footnotes(children, seen, definitions)
            }
            Inline::Image { alt, .. } => collect_inline_footnotes(alt, seen, definitions),
            Inline::Citation { display: Some(display), .. } => collect_inline_footnotes(display, seen, definitions),
            _ => {}
        }
    }
}

fn nested_block_ids(graph: &DocumentGraph) -> HashSet<NodeId> {
    let mut ids = HashSet::new();
    for node in &graph.blocks {
        match &node.block {
            Block::Section { children, .. } => ids.extend(children.iter().copied()),
            Block::Quote { children, .. } => ids.extend(children.iter().copied()),
            Block::List { items, .. } => {
                for item in items {
                    ids.extend(item.children.iter().copied());
                }
            }
            Block::Table { head, body, foot, .. } => {
                for cell in head.iter().chain(body).chain(foot).flat_map(|row| row.cells.iter()) {
                    ids.extend(cell.children.iter().copied());
                }
            }
            _ => {}
        }
    }
    ids
}

fn write_block(
    out: &mut String,
    graph: &DocumentGraph,
    block: &Block,
    image_urls: Option<&HashMap<String, String>>,
) -> Result<(), FormatError> {
    match block {
        Block::Section { level, title, children } => {
            let level = (*level).clamp(1, 6);
            for _ in 0..level {
                out.push('#');
            }
            out.push(' ');
            write_inlines(out, title, image_urls)?;
            out.push_str("\n\n");
            for child_id in children {
                if let Some(child) = graph.block(*child_id) {
                    write_block(out, graph, &child.block, image_urls)?;
                }
            }
        }
        Block::Paragraph { content } => {
            write_inlines(out, content, image_urls)?;
            out.push_str("\n\n");
        }
        Block::Code { language, content } => write_fenced(out, language.as_deref(), content),
        Block::Diagram { language, source } => {
            ensure_reopenable_language(language)?;
            write_fenced(out, Some(language), source);
        }
        Block::Chart { language, source } => {
            ensure_reopenable_chart_language(language)?;
            write_fenced(out, Some(language), source);
        }
        Block::Quote { children } => {
            let mut body = String::new();
            for child_id in children {
                let child = graph
                    .block(*child_id)
                    .ok_or_else(|| FormatError::invalid_input(format!("quote child `{child_id:?}` is missing")))?;
                if !body.is_empty() {
                    body.push_str("\n\n");
                }
                write_block(&mut body, graph, &child.block, image_urls)?;
            }
            let mut previous_blank = false;
            for part in body.trim_end_matches('\n').lines() {
                if part.is_empty() {
                    if previous_blank {
                        continue;
                    }
                    previous_blank = true;
                    out.push_str(">\n");
                }
                else {
                    previous_blank = false;
                    out.push_str("> ");
                    out.push_str(part);
                    out.push('\n');
                }
            }
            out.push('\n');
        }
        Block::List { ordered, items } => {
            write_list(out, graph, *ordered, items, 0, image_urls)?;
            out.push('\n');
        }
        Block::Table { caption, head, body, foot } => {
            write_table(out, graph, caption.as_deref(), head, body, foot, image_urls)?;
            out.push('\n');
        }
        Block::Math { formula } => {
            ensure_latex_math(&formula.source, formula.language.as_deref())?;
            out.push_str("$$");
            out.push_str(&formula.source);
            out.push_str("$$\n\n");
        }
        Block::Opaque { kind, payload_hint, .. } => {
            if kind == "thematic_break" {
                out.push_str(payload_hint);
                out.push_str("\n\n");
            }
            else {
                return Err(FormatError::unsupported(
                    "markdown",
                    format!("opaque block kind `{kind}` is not supported by the IR markdown exporter yet"),
                ));
            }
        }
        Block::RawHtml { html } => {
            out.push_str(html);
            out.push_str("\n\n");
        }
    }
    Ok(())
}

fn write_list(
    out: &mut String,
    graph: &DocumentGraph,
    ordered: bool,
    items: &[ListItem],
    indent: usize,
    image_urls: Option<&HashMap<String, String>>,
) -> Result<(), FormatError> {
    let prefix = " ".repeat(indent);
    let mut next_number = 1;
    for item in items {
        out.push_str(&prefix);
        if ordered {
            let number = item.number.unwrap_or(next_number);
            next_number = number.saturating_add(1);
            write!(out, "{number}. ").map_err(|error| FormatError::unsupported("markdown", error.to_string()))?;
        }
        else {
            out.push_str("- ");
        }
        if item.is_task {
            out.push_str(if item.is_checked.unwrap_or(false) { "[x] " } else { "[ ] " });
        }
        write_inlines(out, &item.content, image_urls)?;
        out.push('\n');
        for child_id in &item.children {
            if let Some(child) = graph.block(*child_id) {
                if let Block::List { ordered: nested_ordered, items: nested_items } = &child.block {
                    write_list(out, graph, *nested_ordered, nested_items, indent + 2, image_urls)?;
                }
                else {
                    return Err(FormatError::unsupported(
                        "markdown",
                        format!("nested list child `{child_id:?}` is not representable yet"),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn write_table(
    out: &mut String,
    graph: &DocumentGraph,
    caption: Option<&str>,
    head: &[TableRow],
    body: &[TableRow],
    foot: &[TableRow],
    image_urls: Option<&HashMap<String, String>>,
) -> Result<(), FormatError> {
    if head.is_empty() && body.is_empty() && foot.is_empty() {
        return Ok(());
    }
    if caption.is_some() || !table_cells_fit_a_pipe_table(graph, head, body, foot) {
        // GFM pipe cells hold inline content only and cannot express merged
        // cells. A spanning table, or one whose cells carry list, quote or
        // nested-table blocks, is written as an inline HTML table instead of
        // silently dropping the structure. A caption has no pipe-table slot
        // either, so a captioned table takes the same HTML path and keeps
        // `<caption>` for the reader that lowers the table again.
        super::html::write_table(
            out,
            graph,
            caption,
            head,
            body,
            foot,
            &HashMap::new(),
            &mut super::html::FootnoteRefIds::default(),
        )?;
        out.push('\n');
        return Ok(());
    }
    // GFM carries exactly one header row. The first row of the table becomes
    // that header, and every later row, including extra head and foot rows,
    // follows as body content so no authored row disappears.
    let mut first = true;
    for row in head.iter().chain(body).chain(foot) {
        out.push('|');
        for cell in &row.cells {
            out.push(' ');
            write_table_cell(out, graph, cell, image_urls)?;
            out.push_str(" |");
        }
        out.push('\n');
        if first {
            out.push('|');
            for _ in &row.cells {
                out.push_str(" --- |");
            }
            out.push('\n');
            first = false;
        }
    }
    Ok(())
}

/// A GFM pipe table holds one inline run per cell and no merged cells.
///
/// The `write_table_cell` helper can join several paragraph children with
/// `<br>`, but it cannot express any other block. Everything outside that
/// shape goes through the HTML table fallback.
fn table_cells_fit_a_pipe_table(graph: &DocumentGraph, head: &[TableRow], body: &[TableRow], foot: &[TableRow]) -> bool {
    head.iter().chain(body).chain(foot).all(|row| {
        row.cells.iter().all(|cell| {
            cell.colspan <= 1
                && cell.rowspan <= 1
                && cell
                    .children
                    .iter()
                    .all(|id| matches!(graph.block(*id).map(|node| &node.block), Some(Block::Paragraph { .. })))
        })
    })
}

/// Table cells carry full child blocks, so emphasis, code spans, links, and
/// images are written as Markdown instead of flattened to plain text. A cell
/// with more than one paragraph joins them with `<br>` because a GFM pipe
/// cell cannot hold block separators.
fn write_table_cell(
    out: &mut String,
    graph: &DocumentGraph,
    cell: &TableCell,
    image_urls: Option<&HashMap<String, String>>,
) -> Result<(), FormatError> {
    let mut wrote = false;
    for child_id in &cell.children {
        let child = graph
            .block(*child_id)
            .ok_or_else(|| FormatError::invalid_input(format!("table cell child `{child_id:?}` is missing")))?;
        let Block::Paragraph { content } = &child.block
        else {
            return Err(FormatError::unsupported(
                "markdown",
                format!("table cell child `{child_id:?}` is not representable in a GFM pipe cell"),
            ));
        };
        if wrote {
            out.push_str("<br>");
        }
        write_inlines_in_table_cell(out, content, image_urls, true)?;
        wrote = true;
    }
    Ok(())
}

fn inline_plain_text(inline: &Inline) -> String {
    inline.plain_text()
}

fn write_inlines(
    out: &mut String,
    inlines: &[Inline],
    image_urls: Option<&HashMap<String, String>>,
) -> Result<(), FormatError> {
    write_inlines_in_table_cell(out, inlines, image_urls, false)
}

fn write_inlines_in_table_cell(
    out: &mut String,
    inlines: &[Inline],
    image_urls: Option<&HashMap<String, String>>,
    in_table_cell: bool,
) -> Result<(), FormatError> {
    for inline in inlines {
        write_inline(out, inline, image_urls, in_table_cell)?;
    }
    Ok(())
}

fn write_inline(
    out: &mut String,
    inline: &Inline,
    image_urls: Option<&HashMap<String, String>>,
    in_table_cell: bool,
) -> Result<(), FormatError> {
    match inline {
        Inline::Text { text } => {
            write_markdown_text(out, text);
        }
        Inline::InlineCode { text } => {
            let delimiter = "`".repeat(longest_backtick_run(text) + 1);
            let needs_padding = text.starts_with('`')
                || text.ends_with('`')
                || (text.starts_with(' ') && text.ends_with(' ') && !text.trim().is_empty());
            out.push_str(&delimiter);
            if needs_padding {
                out.push(' ');
            }
            if in_table_cell {
                out.push_str(&text.replace('|', "\\|"));
            }
            else {
                out.push_str(text);
            }
            if needs_padding {
                out.push(' ');
            }
            out.push_str(&delimiter);
        }
        Inline::Styled { style, children } => {
            let wrapper = match style.as_str() {
                "bold" | "strong" => ("**", "**"),
                "italic" | "emphasis" => ("*", "*"),
                "strike" | "strikethrough" => ("~~", "~~"),
                _ => {
                    return Err(FormatError::unsupported("markdown", format!("inline style `{style}` is not representable")));
                }
            };
            out.push_str(wrapper.0);
            write_inlines_in_table_cell(out, children, image_urls, in_table_cell)?;
            out.push_str(wrapper.1);
        }
        Inline::InlineMath { formula } => {
            ensure_latex_math(&formula.source, formula.language.as_deref())?;
            out.push('$');
            out.push_str(&formula.source);
            out.push('$');
        }
        Inline::Reference { target, .. } => {
            return Err(FormatError::unsupported(
                "markdown",
                format!("semantic reference target `{target:?}` has no stable Markdown anchor mapping"),
            ));
        }
        Inline::Link { target, display, title } => {
            let url = markdown_link_destination(target)?;
            out.push('[');
            write_inlines_in_table_cell(out, display, image_urls, in_table_cell)?;
            out.push_str("](");
            write_markdown_destination(out, url);
            write_markdown_title(out, title.as_deref())?;
            out.push(')');
        }
        Inline::Image { source, alt, title, asset: _ } => {
            let url = image_urls.and_then(|map| map.get(source)).map(String::as_str).unwrap_or(source.as_str());
            out.push_str("![");
            write_markdown_text(out, &alt.iter().map(inline_plain_text).collect::<String>());
            out.push_str("](");
            write_markdown_destination(out, url);
            write_markdown_title(out, title.as_deref())?;
            out.push(')');
        }
        Inline::Note { label, .. } => {
            out.push_str("[^");
            out.push_str(label);
            out.push(']');
        }
        Inline::Break { kind } => match kind {
            BreakKind::Soft => {
                if in_table_cell {
                    out.push(' ');
                }
                else {
                    out.push('\n');
                }
            }
            BreakKind::Hard => {
                if in_table_cell {
                    out.push(' ');
                }
                else {
                    out.push_str("  \n");
                }
            }
        },
        Inline::Attributed { attributes, children }
            if matches!(children.as_slice(), [Inline::Image { .. }]) && !attributes.is_empty() =>
        {
            // An image wrapper carries the display size or identity of the bitmap
            // itself. Inline HTML keeps those on the `<img>` element, where every
            // downstream tool reads them; a `<span width="…">` would be invalid
            // markup that no consumer treats as a size.
            let Inline::Image { source, alt, title, asset: _ } = &children[0]
            else {
                unreachable!("the guard matched one image")
            };
            let url = image_urls.and_then(|map| map.get(source)).map(String::as_str).unwrap_or(source.as_str());
            out.push_str(r#"<img src=""#);
            write_html_attribute_value(out, url);
            out.push('"');
            out.push_str(r#" alt=""#);
            write_html_attribute_value(out, &alt.iter().map(inline_plain_text).collect::<String>());
            out.push('"');
            if let Some(title) = title {
                out.push_str(r#" title=""#);
                write_html_attribute_value(out, title);
                out.push('"');
            }
            write_span_attributes(out, attributes)?;
            out.push('>');
        }
        Inline::Attributed { attributes, children } => {
            // Markdown accepts inline HTML, which is the only spelling that
            // keeps the authored `id`/`class` attributes instead of dropping
            // them, so the attributes survive the projection.
            out.push_str("<span");
            write_span_attributes(out, attributes)?;
            out.push('>');
            write_inlines_in_table_cell(out, children, image_urls, in_table_cell)?;
            out.push_str("</span>");
        }
        Inline::Citation { .. } => {
            return Err(FormatError::unsupported(
                "markdown",
                "inline semantic node is not representable by the conservative IR markdown exporter yet",
            ));
        }
        Inline::RawHtml { html } => {
            // Raw inline HTML stays verbatim in Markdown output.
            out.push_str(html);
        }
    }
    Ok(())
}

fn write_span_attributes(out: &mut String, attributes: &Attributes) -> Result<(), FormatError> {
    if let Some(id) = &attributes.id {
        out.push_str(r#" id=""#);
        write_html_attribute_value(out, id);
        out.push('"');
    }
    if !attributes.classes.is_empty() {
        out.push_str(r#" class=""#);
        write_html_attribute_value(out, &attributes.classes.join(" "));
        out.push('"');
    }
    for pair in &attributes.pairs {
        let valid_name = !pair.key.is_empty()
            && pair.key.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.'));
        if !valid_name || pair.key.eq_ignore_ascii_case("id") || pair.key.eq_ignore_ascii_case("class") {
            return Err(FormatError::unsupported(
                "markdown",
                format!("attribute name `{}` has no inline HTML spelling", pair.key),
            ));
        }
        out.push(' ');
        out.push_str(&pair.key);
        out.push_str(r#"=""#);
        write_html_attribute_value(out, &pair.value);
        out.push('"');
    }
    Ok(())
}

fn write_html_attribute_value(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

fn write_markdown_text(out: &mut String, text: &str) {
    let mut characters = text.chars().peekable();
    let mut previous = None;
    while let Some(character) = characters.next() {
        let next = characters.peek().copied();
        // CommonMark already treats an underscore between two word characters
        // as literal text, so escaping it would change authored bytes for no
        // reason. Every other underscore stays escaped.
        let intraword_underscore =
            character == '_' && previous.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric);
        if matches!(character, '\\' | '`' | '*' | '{' | '}' | '[' | ']' | '<' | '>' | '!' | '|' | '#' | '+' | '-')
            || (character == '_' && !intraword_underscore)
        {
            out.push('\\');
        }
        out.push(character);
        previous = Some(character);
    }
}

fn ensure_latex_math(content: &str, language: Option<&str>) -> Result<(), FormatError> {
    if language.is_some_and(|language| language != "latex") || content.contains('$') {
        return Err(FormatError::unsupported("markdown", "math requires LaTeX content without dollar delimiters"));
    }
    Ok(())
}

/// Resolve a semantic link target to a Markdown destination.
///
/// Only external URIs have a stable Markdown spelling. Node and asset endpoints
/// are reported instead of being flattened into text. Titles are read back by
/// the importer, so they are written next to the destination.
fn markdown_link_destination(target: &RelationEndpoint) -> Result<&str, FormatError> {
    match target {
        RelationEndpoint::ExternalUri(url) => Ok(url),
        other => Err(FormatError::unsupported(
            "markdown",
            format!("link target `{other:?}` has no stable Markdown destination mapping"),
        )),
    }
}

fn write_markdown_title(out: &mut String, title: Option<&str>) -> Result<(), FormatError> {
    let Some(title) = title
    else {
        return Ok(());
    };
    if title.contains(['\n', '\r']) {
        return Err(FormatError::unsupported("markdown", "inline title cannot contain a line break"));
    }
    out.push_str(" \"");
    for character in title.chars() {
        if matches!(character, '\\' | '"') {
            out.push('\\');
        }
        out.push(character);
    }
    out.push('"');
    Ok(())
}

fn write_markdown_destination(out: &mut String, destination: &str) {
    for character in destination.chars() {
        if matches!(character, '\\' | '(' | ')') {
            out.push('\\');
        }
        out.push(character);
    }
}

fn write_fenced(out: &mut String, language: Option<&str>, content: &str) {
    let fence = "`".repeat(longest_backtick_run(content).max(2) + 1);
    out.push_str(&fence);
    if let Some(language) = language {
        out.push_str(language);
    }
    out.push('\n');
    out.push_str(content);
    out.push('\n');
    out.push_str(&fence);
    out.push_str("\n\n");
}

/// A diagram only round-trips through Markdown when the importer promotes the
/// same fence back to `Block::Diagram`.
fn ensure_reopenable_language(language: &str) -> Result<(), FormatError> {
    if language.eq_ignore_ascii_case("mermaid")
        || language.eq_ignore_ascii_case("dot")
        || language.eq_ignore_ascii_case("plantuml")
    {
        Ok(())
    }
    else {
        Err(FormatError::unsupported(
            "markdown",
            format!("diagram language `{language}` has no Markdown reopen rule and would reopen as a code block"),
        ))
    }
}

/// A chart only round-trips through Markdown when the importer promotes the
/// same fence back to `Block::Chart`.
fn ensure_reopenable_chart_language(language: &str) -> Result<(), FormatError> {
    if language.eq_ignore_ascii_case("vega")
        || language.eq_ignore_ascii_case("vega-lite")
        || language.eq_ignore_ascii_case("chart")
    {
        Ok(())
    }
    else {
        Err(FormatError::unsupported(
            "markdown",
            format!("chart language `{language}` has no Markdown reopen rule and would reopen as a code block"),
        ))
    }
}

fn longest_backtick_run(text: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for character in text.chars() {
        if character == '`' {
            current += 1;
            longest = longest.max(current);
        }
        else {
            current = 0;
        }
    }
    longest
}
