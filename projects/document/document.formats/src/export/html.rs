use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
};

use document_ir::{
    Attributes, Block, BreakKind, DocumentGraph, Inline, ListItem, LossMarker, NodeId, RelationEndpoint, SemanticStatus,
    TableRow,
};

use crate::FormatError;

/// HTML output plus the semantics the projection had to defer.
///
/// `export_html` stays a string-returning convenience for hosts that only need
/// markup. Callers that report fidelity use [`export_html_report`] and surface
/// `losses` next to the output.
pub struct HtmlExport {
    /// The exported HTML fragment.
    pub html: String,
    /// Semantics that have no HTML form and were degraded or projected inert.
    pub losses: Vec<LossMarker>,
}

/// A diagram scene rendered by the host, embedded into the HTML output in
/// place of the source-only fallback.
///
/// The host owns scene lowering (for example the DaVinci diagram chain); this
/// exporter only owns the markup shape. `svg` must be a serialized, valid SVG
/// fragment and is emitted verbatim inside the figure.
pub struct RenderedDiagram {
    /// Inline SVG markup serialized from the resolved display scene.
    pub svg: String,
}

/// Unique `id` values for footnote reference anchors, in document order.
///
/// The first reference to a label gets `fnref-<label>` so the endnote
/// backlink resolves. Repeated references get `fnref-<label>-<n>` so ids stay
/// unique inside one document, and every reference keeps its own backlink.
#[derive(Default)]
pub(crate) struct FootnoteRefIds {
    ids: HashMap<String, Vec<String>>,
    defined: HashSet<String>,
}

impl FootnoteRefIds {
    /// Collect the footnote labels that have a definition block, so a reference
    /// only links when its target exists.
    fn with_definitions(graph: &DocumentGraph) -> Self {
        let defined = graph
            .blocks
            .iter()
            .filter_map(|node| match &node.block {
                Block::Opaque { kind, payload_hint, .. } if kind == "footnote_definition" => {
                    split_footnote_definition(payload_hint).map(|(label, _)| label)
                }
                _ => None,
            })
            .collect();
        Self { ids: HashMap::new(), defined }
    }

    fn is_defined(&self, label: &str) -> bool {
        self.defined.contains(label)
    }

    pub(crate) fn next(&mut self, label: &str) -> String {
        let ids = self.ids.entry(label.to_string()).or_default();
        let id = if ids.is_empty() { format!("fnref-{label}") } else { format!("fnref-{label}-{}", ids.len()) };
        ids.push(id.clone());
        id
    }

    /// Reference anchor ids authored for one footnote label, in document order.
    pub(crate) fn ids(&self, label: &str) -> &[String] {
        self.ids.get(label).map(Vec::as_slice).unwrap_or_default()
    }
}

/// Export a semantic document graph as a standalone HTML fragment.
///
/// Diagram blocks without a host-rendered scene keep the source-only `<pre>`
/// fallback. Hosts that render diagrams use [`export_html_with_diagrams`].
pub fn export_html(graph: &DocumentGraph) -> Result<String, FormatError> {
    Ok(export_html_report(graph)?.html)
}

/// Export a document graph as HTML together with the fidelity report.
pub fn export_html_report(graph: &DocumentGraph) -> Result<HtmlExport, FormatError> {
    export_html_report_with_diagrams(graph, &HashMap::new())
}

/// Export a semantic document graph as a standalone HTML fragment, embedding
/// host-rendered diagram scenes keyed by the block that authored them.
///
/// A rendered diagram becomes a `<figure>` carrying the inline SVG plus the
/// `data-diagram-language` / `data-diagram-source-hex` round-trip markers, so
/// the HTML importer still recovers the authored source. A diagram block whose
/// scene failed to render keeps the visible source `<pre>` fallback.
pub fn export_html_with_diagrams(
    graph: &DocumentGraph,
    diagrams: &HashMap<NodeId, RenderedDiagram>,
) -> Result<String, FormatError> {
    Ok(export_html_report_with_diagrams(graph, diagrams)?.html)
}

/// Export as HTML and report every construct the projection could not carry.
pub fn export_html_report_with_diagrams(
    graph: &DocumentGraph,
    diagrams: &HashMap<NodeId, RenderedDiagram>,
) -> Result<HtmlExport, FormatError> {
    let validation = graph.validate();
    if !validation.is_valid() {
        return Err(FormatError::invalid_input(format!("invalid document graph: {:?}", validation.issues)));
    }

    let mut out = String::new();
    out.push_str(r#"<article class="notedown-document">"#);
    if let Some(title) = &graph.metadata.title {
        // The title is document metadata, not body content. The marker lets the
        // HTML importer keep it in `metadata.title` instead of lowering the
        // heading into the body, which used to duplicate it in round trips.
        out.push_str(r#"<header class="notedown-document-title" data-document-title=""#);
        escape_attribute(&mut out, title);
        out.push_str(r#""><h1>"#);
        escape_text(&mut out, title);
        out.push_str("</h1></header>");
    }
    let nested = nested_block_ids(graph);
    let mut footnote_refs = FootnoteRefIds::with_definitions(graph);
    let mut footnotes: Vec<&str> = Vec::new();
    for node in &graph.blocks {
        if nested.contains(&node.id) {
            continue;
        }
        if let Block::Opaque { kind, payload_hint, .. } = &node.block {
            if kind == "footnote_definition" {
                footnotes.push(payload_hint.as_str());
                continue;
            }
        }
        write_node(&mut out, graph, node.id, &node.block, diagrams, &mut footnote_refs)?;
    }
    if !footnotes.is_empty() {
        write_footnote_section(&mut out, &footnotes, &footnote_refs);
    }
    out.push_str("</article>");

    Ok(HtmlExport { html: out, losses: collect_export_losses(graph) })
}

/// Footnote definitions have no block form in HTML: they are collected into
/// one endnotes section so the reference links resolve. Every entry keeps the
/// authored GFM payload in `data-footnote-source-hex` so the HTML importer can
/// restore the exact definition instead of re-serializing it from display text.
fn write_footnote_section(out: &mut String, definitions: &[&str], footnote_refs: &FootnoteRefIds) {
    out.push_str(r#"<section class="footnotes" role="doc-endnotes"><ol>"#);
    for payload in definitions {
        let Some((label, body)) = split_footnote_definition(payload)
        else {
            continue;
        };
        out.push_str(r#"<li id="fn-"#);
        escape_attribute(out, &label);
        out.push_str(r#"" data-footnote-source-hex=""#);
        escape_attribute(out, &encode_hex(payload));
        out.push_str(r#""><p>"#);
        escape_text(out, &body);
        out.push_str("</p>");
        // One backlink per reference keeps every citation site reachable,
        // including repeated references to the same footnote.
        for reference_id in footnote_refs.ids(&label) {
            out.push_str(r##"<a class="footnote-backref" href="#"##);
            escape_attribute(out, reference_id);
            out.push_str("\">↩</a>");
        }
        // The backlinks live outside the paragraph so the imported body stays
        // byte-identical to the authored definition.
        out.push_str(r#"</li>"#);
    }
    out.push_str("</ol></section>");
}

/// The LaTeX body a front-end math renderer consumes.
///
/// [`MathFormula::source`](document_ir::MathFormula) keeps the authored text
/// for round-trip. A display wrapper such as `\begin{equation}` is not part of
/// the formula body and KaTeX does not implement it, so the HTML projection
/// drops that wrapper and keeps the inner text. Environments that carry the
/// formula itself (`matrix`, `aligned`, `cases`, `align`) stay intact: without
/// their wrapper their `&` and `\\` are not consumable LaTeX.
fn math_payload(source: &str) -> &str {
    let Some(rest) = source.strip_prefix("\\begin{")
    else {
        return source;
    };
    let Some(close) = rest.find('}')
    else {
        return source;
    };
    let name = &rest[..close];
    if !DISPLAY_WRAPPER_ENVIRONMENTS.contains(&name) {
        return source;
    }
    let body = &rest[close + 1..];
    let end = format!("\\end{{{name}}}");
    match body.strip_suffix(&end) {
        Some(inner) => inner.trim(),
        None => source,
    }
}

/// Environments that only wrap display math, and that KaTeX does not implement.
const DISPLAY_WRAPPER_ENVIRONMENTS: &[&str] = &["equation", "equation*"];

/// Split a GFM footnote definition payload (`[^label]: body`) into its label
/// and body. Continuation lines stay in the body verbatim.
fn split_footnote_definition(payload: &str) -> Option<(String, String)> {
    let rest = payload.strip_prefix("[^")?;
    let (label, body) = rest.split_once("]:")?;
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    Some((label.to_string(), body.trim_start_matches([' ', '\t', '\n', '\r']).trim_end().to_string()))
}

fn write_node(
    out: &mut String,
    graph: &DocumentGraph,
    id: NodeId,
    block: &Block,
    diagrams: &HashMap<NodeId, RenderedDiagram>,
    footnote_refs: &mut FootnoteRefIds,
) -> Result<(), FormatError> {
    write!(out, r#"<div id="node-{}" data-node-id="{}">"#, id.0, id.0)
        .map_err(|error| FormatError::unsupported("html", error.to_string()))?;
    write_block(out, graph, id, block, diagrams, footnote_refs)?;
    out.push_str("</div>");
    Ok(())
}

fn nested_block_ids(graph: &DocumentGraph) -> std::collections::HashSet<document_ir::NodeId> {
    let mut ids = std::collections::HashSet::new();
    for node in &graph.blocks {
        match &node.block {
            Block::Section { children, .. } => ids.extend(children.iter().copied()),
            Block::Quote { children, .. } => ids.extend(children.iter().copied()),
            Block::List { items, .. } => {
                for item in items {
                    ids.extend(item.children.iter().copied());
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
    id: NodeId,
    block: &Block,
    diagrams: &HashMap<NodeId, RenderedDiagram>,
    footnote_refs: &mut FootnoteRefIds,
) -> Result<(), FormatError> {
    match block {
        Block::Section { level, title, children } => {
            let level = (*level).clamp(1, 6);
            write!(out, "<h{}>", level).map_err(format_error)?;
            write_inlines(out, title, footnote_refs)?;
            write!(out, "</h{}>", level).map_err(format_error)?;
            for child_id in children {
                let child = graph
                    .block(*child_id)
                    .ok_or_else(|| FormatError::invalid_input(format!("section child `{child_id:?}` is missing")))?;
                write_node(out, graph, child.id, &child.block, diagrams, footnote_refs)?;
            }
        }
        Block::Paragraph { content } => {
            out.push_str("<p>");
            write_inlines(out, content, footnote_refs)?;
            out.push_str("</p>");
        }
        Block::Quote { children } => {
            out.push_str("<blockquote>");
            for child_id in children {
                let child = graph
                    .block(*child_id)
                    .ok_or_else(|| FormatError::invalid_input(format!("quote child `{child_id:?}` is missing")))?;
                write_node(out, graph, child.id, &child.block, diagrams, footnote_refs)?;
            }
            out.push_str("</blockquote>");
        }
        Block::Code { language, content } => {
            out.push_str("<pre><code");
            if let Some(language) = language {
                out.push_str(r#" class="language-"#);
                escape_attribute(out, language);
                out.push('"');
            }
            out.push('>');
            escape_text(out, content);
            out.push_str("</code></pre>");
        }
        Block::Diagram { language, source } => match diagrams.get(&id) {
            Some(rendered) => {
                out.push_str(r#"<figure class="diagram" data-diagram-language=""#);
                escape_attribute(out, language);
                out.push_str(r#"" data-diagram-source-hex=""#);
                out.push_str(&encode_hex(source));
                out.push_str(r#"">"#);
                out.push_str(&rendered.svg);
                out.push_str("</figure>");
            }
            None => {
                out.push_str(r#"<pre class="diagram" data-diagram-language=""#);
                escape_attribute(out, language);
                out.push_str(r#"" data-diagram-source-hex=""#);
                out.push_str(&encode_hex(source));
                out.push_str(r#""><code>"#);
                escape_text(out, source);
                out.push_str("</code></pre>");
            }
        },
        Block::Chart { language, source } => match diagrams.get(&id) {
            Some(rendered) => {
                out.push_str(r#"<figure class="chart" data-chart-language=""#);
                escape_attribute(out, language);
                out.push_str(r#"" data-chart-source-hex=""#);
                out.push_str(&encode_hex(source));
                out.push_str(r#"">"#);
                out.push_str(&rendered.svg);
                out.push_str("</figure>");
            }
            None => {
                out.push_str(r#"<pre class="chart" data-chart-language=""#);
                escape_attribute(out, language);
                out.push_str(r#"" data-chart-source-hex=""#);
                out.push_str(&encode_hex(source));
                out.push_str(r#""><code>"#);
                escape_text(out, source);
                out.push_str("</code></pre>");
            }
        },
        Block::Math { formula } => {
            out.push_str(r#"<div class="math""#);
            if let Some(language) = &formula.language {
                out.push_str(r#" data-language=""#);
                escape_attribute(out, language);
                out.push('"');
            }
            out.push('>');
            escape_text(out, math_payload(&formula.source));
            out.push_str("</div>");
        }
        Block::List { ordered, items } => write_list(out, graph, *ordered, items, diagrams, footnote_refs)?,
        Block::Table { rows, .. } => write_table(out, rows, footnote_refs)?,
        Block::Opaque { kind, payload_hint, .. } if kind == "thematic_break" => {
            out.push_str("<hr>");
            if !payload_hint.is_empty() {
                out.push_str(r#"<span class="opaque-hint">"#);
                escape_text(out, payload_hint);
                out.push_str("</span>");
            }
        }
        Block::Opaque { kind, payload_hint, .. } => {
            // Opaque payloads stay in the IR for the stages that understand
            // them. The HTML projection either skips the construct or renders
            // the authored payload as inert text, so one construct that has no
            // HTML form no longer fails the whole document. Every deferral is
            // already recorded in the graph loss markers at import time.
            if !kind.starts_with("typst.") {
                out.push_str(r#"<div class="opaque" data-opaque-kind=""#);
                escape_attribute(out, kind);
                out.push_str(r#"">"#);
                escape_text(out, payload_hint);
                out.push_str("</div>");
            }
        }
        Block::RawHtml { html } => {
            out.push_str(html);
        }
    }
    Ok(())
}

fn write_list(
    out: &mut String,
    graph: &DocumentGraph,
    ordered: bool,
    items: &[ListItem],
    diagrams: &HashMap<NodeId, RenderedDiagram>,
    footnote_refs: &mut FootnoteRefIds,
) -> Result<(), FormatError> {
    out.push_str(if ordered { "<ol>" } else { "<ul>" });
    for item in items {
        if item.is_task {
            out.push_str(r#"<li class="task-list-item"><input type="checkbox" disabled"#);
            if item.is_checked.unwrap_or(false) {
                out.push_str(" checked");
            }
            out.push_str(">");
        }
        else {
            out.push_str("<li>");
        }
        write_inlines(out, &item.content, footnote_refs)?;
        for child_id in &item.children {
            let child = graph
                .block(*child_id)
                .ok_or_else(|| FormatError::invalid_input(format!("list child `{child_id:?}` is missing")))?;
            write_node(out, graph, child.id, &child.block, diagrams, footnote_refs)?;
        }
        out.push_str("</li>");
    }
    out.push_str(if ordered { "</ol>" } else { "</ul>" });
    Ok(())
}

pub(crate) fn write_table(out: &mut String, rows: &[TableRow], footnote_refs: &mut FootnoteRefIds) -> Result<(), FormatError> {
    out.push_str("<table>");
    // `TableRow::is_header` is the only section information the IR keeps, so the
    // leading run of header rows becomes `<thead>` and everything after it
    // becomes `<tbody>`. Header rows that appear after body rows are reported
    // by the HTML importer as `import.html.table_sections`, because no bag of
    // per-row flags can express two header groups.
    let header_count = rows.iter().take_while(|row| row.is_header).count();
    if header_count > 0 {
        out.push_str("<thead>");
        write_table_rows(out, &rows[..header_count], footnote_refs)?;
        out.push_str("</thead>");
    }
    if header_count < rows.len() {
        out.push_str("<tbody>");
        write_table_rows(out, &rows[header_count..], footnote_refs)?;
        out.push_str("</tbody>");
    }
    out.push_str("</table>");
    Ok(())
}

fn write_table_rows(out: &mut String, rows: &[TableRow], footnote_refs: &mut FootnoteRefIds) -> Result<(), FormatError> {
    for row in rows {
        out.push_str("<tr>");
        let tag = if row.is_header { "th" } else { "td" };
        for cell in &row.cells {
            out.push('<');
            out.push_str(tag);
            if cell.colspan > 1 {
                out.push_str(" colspan=\"");
                out.push_str(&cell.colspan.to_string());
                out.push('"');
            }
            if cell.rowspan > 1 {
                out.push_str(" rowspan=\"");
                out.push_str(&cell.rowspan.to_string());
                out.push('"');
            }
            out.push('>');
            write_inlines(out, &cell.content, footnote_refs)?;
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        out.push_str("</tr>");
    }
    Ok(())
}

fn write_inlines(out: &mut String, inlines: &[Inline], footnote_refs: &mut FootnoteRefIds) -> Result<(), FormatError> {
    for inline in inlines {
        match inline {
            Inline::Text { text } => escape_text(out, text),
            Inline::InlineCode { text } => {
                out.push_str("<code>");
                escape_text(out, text);
                out.push_str("</code>");
            }
            Inline::InlineMath { formula } => {
                out.push_str(r#"<span class="math""#);
                if let Some(language) = &formula.language {
                    out.push_str(r#" data-language=""#);
                    escape_attribute(out, language);
                    out.push('"');
                }
                out.push('>');
                escape_text(out, &formula.source);
                out.push_str("</span>");
            }
            Inline::Reference { display, target } => {
                write!(out, r##"<a href="#node-{}">"##, target.0).map_err(format_error)?;
                escape_text(out, display);
                out.push_str("</a>");
            }
            Inline::Styled { style, children } => {
                if let Some(color) = style.strip_prefix("color:") {
                    if color.len() != 6 || !color.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        return Err(FormatError::unsupported("html", format!("invalid color style `{style}`")));
                    }
                    write!(out, "<span style=\"color:#{color}\">").map_err(format_error)?;
                    write_inlines(out, children, footnote_refs)?;
                    out.push_str("</span>");
                    continue;
                }
                let tag = match style.as_str() {
                    "bold" | "strong" => "strong",
                    "italic" | "emphasis" => "em",
                    "underline" => "u",
                    "strike" | "strikethrough" => "s",
                    "superscript" => "sup",
                    "subscript" => "sub",
                    _ => {
                        return Err(FormatError::unsupported("html", format!("inline style `{style}` is not representable")));
                    }
                };
                write!(out, "<{}>", tag).map_err(format_error)?;
                write_inlines(out, children, footnote_refs)?;
                write!(out, "</{}>", tag).map_err(format_error)?;
            }
            Inline::Link { target, display, title } => {
                // In-document targets reuse the `#node-{id}` destination that
                // every block wrapper already publishes, so a `\ref` to a
                // `\label` anchor resolves to the block it names instead of
                // failing the whole document.
                let destination = match target {
                    RelationEndpoint::ExternalUri(url) => url.clone(),
                    RelationEndpoint::Node(node) => format!("#node-{}", node.0),
                    _ => {
                        // No destination exists, so the link cannot stay a link.
                        // Keep the authored display text visible and let the
                        // fidelity report name the dropped target instead of
                        // failing the whole document.
                        out.push_str(r#"<span class="unresolved-link">"#);
                        write_inlines(out, display, footnote_refs)?;
                        out.push_str("</span>");
                        continue;
                    }
                };
                out.push_str(r#"<a href=""#);
                escape_attribute(out, &destination);
                out.push('"');
                write_optional_title(out, title.as_deref());
                out.push('>');
                write_inlines(out, display, footnote_refs)?;
                out.push_str("</a>");
            }
            Inline::Image { source, alt, title, asset: _ } => {
                out.push_str(r#"<img alt=""#);
                escape_attribute(out, &alt.iter().map(inline_plain_text).collect::<String>());
                out.push_str(r#"" src=""#);
                escape_attribute(out, source);
                out.push('"');
                write_optional_title(out, title.as_deref());
                out.push('>');
            }
            Inline::Break { kind } => match kind {
                BreakKind::Hard => out.push_str("<br>"),
                BreakKind::Soft => out.push('\n'),
            },
            Inline::Attributed { attributes, children } => {
                out.push_str("<span");
                write_attributes(out, attributes)?;
                out.push('>');
                write_inlines(out, children, footnote_refs)?;
                out.push_str("</span>");
            }
            Inline::Note { label, display } => {
                out.push_str(r#"<sup class="footnote-reference" data-footnote-label=""#);
                escape_attribute(out, label);
                out.push('"');
                if footnote_refs.is_defined(label) {
                    out.push_str("><a id=\"");
                    escape_attribute(out, &footnote_refs.next(label));
                    out.push('"');
                    out.push_str(r##" href="#fn-"##);
                    escape_attribute(out, label);
                    out.push_str("\">");
                    match display {
                        Some(display) => write_inlines(out, display, footnote_refs)?,
                        None => escape_text(out, label),
                    }
                    out.push_str("</a></sup>");
                }
                else {
                    // No definition block means no endnote target, so the
                    // reference stays a plain marker instead of a dead link.
                    out.push('>');
                    match display {
                        Some(display) => write_inlines(out, display, footnote_refs)?,
                        None => escape_text(out, label),
                    }
                    out.push_str("</sup>");
                }
            }
            Inline::Citation { key, display } => {
                // A citation has no HTML form without a citation database, so
                // it degrades to its authored key as text instead of failing
                // the document. The unresolved relation stays visible in the
                // graph loss markers.
                out.push_str(r#"<span class="citation" data-citation-key=""#);
                escape_attribute(out, key);
                out.push_str(r#"">"#);
                match display {
                    Some(display) => write_inlines(out, display, footnote_refs)?,
                    None => {
                        out.push('[');
                        escape_text(out, key);
                        out.push(']');
                    }
                }
                out.push_str("</span>");
            }
            Inline::RawHtml { html } => out.push_str(html),
        }
    }
    Ok(())
}

/// Collect the semantics an HTML projection has to degrade or project inert.
///
/// The writer already keeps the authored display text for each of these, so the
/// report is the only place a consumer can learn that the markup is lossy.
fn collect_export_losses(graph: &DocumentGraph) -> Vec<LossMarker> {
    let mut losses = Vec::new();
    let mut seen = HashSet::new();
    for node in &graph.blocks {
        match &node.block {
            Block::Section { title, .. } => collect_inline_losses(graph, title, &mut losses, &mut seen),
            Block::Paragraph { content } => collect_inline_losses(graph, content, &mut losses, &mut seen),
            Block::List { items, .. } => {
                for item in items {
                    collect_inline_losses(graph, &item.content, &mut losses, &mut seen);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_inline_losses(graph, &cell.content, &mut losses, &mut seen);
                    }
                }
            }
            Block::Opaque { kind, .. } if kind == "thematic_break" || kind == "footnote_definition" => {}
            Block::Opaque { kind, .. } => push_export_loss(
                &mut losses,
                &mut seen,
                "export.html.opaque_projection",
                format!("opaque block kind `{kind}` has no HTML form and is projected as inert markup"),
                SemanticStatus::Partial,
            ),
            _ => {}
        }
    }
    losses
}

fn collect_inline_losses(
    graph: &DocumentGraph,
    inlines: &[Inline],
    losses: &mut Vec<LossMarker>,
    seen: &mut HashSet<(String, String)>,
) {
    for inline in inlines {
        match inline {
            Inline::Styled { children, .. } | Inline::Attributed { children, .. } => {
                collect_inline_losses(graph, children, losses, seen)
            }
            Inline::Link { target, display, .. } => {
                if !matches!(target, RelationEndpoint::ExternalUri(_) | RelationEndpoint::Node(_)) {
                    push_export_loss(
                        losses,
                        seen,
                        "export.html.unresolved_link",
                        format!("link target `{target:?}` has no HTML destination and is written as display text"),
                        SemanticStatus::Unresolved,
                    );
                }
                collect_inline_losses(graph, display, losses, seen);
            }
            Inline::Citation { key, display } => {
                push_export_loss(
                    losses,
                    seen,
                    "export.html.citation_degraded",
                    format!("citation `{key}` has no HTML form without a citation database and is written as authored text"),
                    SemanticStatus::Partial,
                );
                if let Some(display) = display {
                    collect_inline_losses(graph, display, losses, seen);
                }
            }
            Inline::Image { source, asset, .. } => {
                let resolved = asset
                    .and_then(|id| graph.assets.iter().find(|candidate| candidate.id == id))
                    .is_some_and(|asset| asset.status == SemanticStatus::Resolved);
                if !resolved {
                    push_export_loss(
                        losses,
                        seen,
                        "export.html.image_asset_unresolved",
                        format!("image `{source}` has no resolved asset and keeps its source path only"),
                        SemanticStatus::Unresolved,
                    );
                }
            }
            Inline::Note { display: Some(display), .. } => collect_inline_losses(graph, display, losses, seen),
            _ => {}
        }
    }
}

fn push_export_loss(
    losses: &mut Vec<LossMarker>,
    seen: &mut HashSet<(String, String)>,
    code: &str,
    message: String,
    status: SemanticStatus,
) {
    if seen.insert((code.to_string(), message.clone())) {
        losses.push(LossMarker { code: code.into(), message, status });
    }
}

fn write_attributes(out: &mut String, attributes: &Attributes) -> Result<(), FormatError> {
    if let Some(id) = &attributes.id {
        out.push_str(r#" id=""#);
        escape_attribute(out, id);
        out.push('"');
    }
    if !attributes.classes.is_empty() {
        out.push_str(r#" class=""#);
        escape_attribute(out, &attributes.classes.join(" "));
        out.push('"');
    }
    for pair in &attributes.pairs {
        if !is_valid_attribute_name(&pair.key) || pair.key.eq_ignore_ascii_case("id") || pair.key.eq_ignore_ascii_case("class")
        {
            return Err(FormatError::unsupported("html", format!("invalid attribute name `{}`", pair.key)));
        }
        out.push(' ');
        out.push_str(&pair.key);
        out.push_str(r#"=""#);
        escape_attribute(out, &pair.value);
        out.push('"');
    }
    Ok(())
}

fn is_valid_attribute_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn inline_plain_text(inline: &Inline) -> String {
    inline.plain_text()
}

/// Diagram source keeps its exact bytes through HTML, where attribute decoding
/// and whitespace normalization would otherwise rewrite the authored lines.
fn encode_hex(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.as_bytes() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn escape_text(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

fn write_optional_title(out: &mut String, title: Option<&str>) {
    if let Some(title) = title {
        out.push_str(r#" title=""#);
        escape_attribute(out, title);
        out.push('"');
    }
}

fn escape_attribute(out: &mut String, value: &str) {
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

fn format_error(error: std::fmt::Error) -> FormatError {
    FormatError::unsupported("html", error.to_string())
}
