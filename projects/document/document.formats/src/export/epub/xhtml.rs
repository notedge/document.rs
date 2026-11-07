//! XHTML chapter rendering for EPUB export.
//!
//! Own block/inline → XHTML emitter so the EPUB exporter stays self-contained
//! from `export::html`. Rendered shape matches the conservative HTML exporter
//! but wraps every chapter in a full XHTML document with the EPUB required
//! namespaces and `xml:lang`/`lang` pair.

use std::fmt::Write as _;

use document_ir::{Attributes, Block, BreakKind, DocumentGraph, Inline, ListItem, NodeId, RelationEndpoint, TableRow};

use super::chapter::{ChapterPlan, asset_source_map, plan_assets};
use crate::{FormatError, export::html::FootnoteRefIds};

/// Render a single chapter as a full XHTML document.
pub fn chapter_xhtml(graph: &DocumentGraph, plan: &ChapterPlan, links: &ChapterLinks) -> Result<String, FormatError> {
    let mut out = String::new();
    out.push_str(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml""#,
    );
    if let Some(lang) = graph.metadata.language.as_deref() {
        out.push_str(" xml:lang=\"");
        escape_attribute(&mut out, lang);
        out.push_str("\" lang=\"");
        escape_attribute(&mut out, lang);
        out.push('"');
    }
    else {
        out.push_str(" xml:lang=\"en\" lang=\"en\"");
    }
    out.push_str(
        r#">
  <head>
    <meta charset="UTF-8"/>
    <title>"#,
    );
    escape_text(&mut out, &plan.label);
    out.push_str(
        r#"</title>
  </head>
  <body>
"#,
    );

    // Emit each block owned by this chapter (only top-level ones in the plan).
    let assets = plan_assets(graph);
    let source_map = asset_source_map(&assets);
    let mut emitted = std::collections::HashSet::new();
    let mut footnotes: Vec<&str> = Vec::new();
    let mut footnote_refs = FootnoteRefIds::default();
    for node_id in &plan.blocks {
        if emitted.contains(node_id) {
            continue;
        }
        emitted.extend(nested_block_ids(graph, *node_id));
        let Some(node) = graph.block(*node_id)
        else {
            return Err(FormatError::invalid_input(format!("chapter block `{node_id:?}` is missing from the graph")));
        };
        if let Block::Opaque { kind, payload_hint, .. } = &node.block {
            if kind == "footnote_definition" {
                footnotes.push(payload_hint.as_str());
                continue;
            }
        }
        write_node(&mut out, graph, *node_id, &node.block, Some(&source_map), &mut footnote_refs, links)?;
    }
    if !footnotes.is_empty() {
        write_footnote_section(&mut out, &footnotes, &footnote_refs);
    }

    out.push_str(
        r#"  </body>
</html>
"#,
    );
    Ok(out)
}

/// Footnote definitions are collected into one endnotes section per chapter.
/// Each item keeps the authored GFM payload in `data-footnote-source-hex` so
/// the definition can be restored instead of re-serialized from display text.
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
        for reference_id in footnote_refs.ids(&label) {
            out.push_str(r##"<a class="footnote-backref" href="#"##);
            escape_attribute(out, reference_id);
            out.push_str("\">↩</a>");
        }
        out.push_str("</li>");
    }
    out.push_str("</ol></section>\n");
}

fn split_footnote_definition(payload: &str) -> Option<(String, String)> {
    let rest = payload.strip_prefix("[^")?;
    let (label, body) = rest.split_once("]:")?;
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    Some((label.to_string(), body.trim_start_matches([' ', '\t', '\n', '\r']).trim_end().to_string()))
}

fn encode_hex(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub(super) fn nested_block_ids(graph: &DocumentGraph, node_id: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let Some(node) = graph.block(node_id)
    else {
        return out;
    };
    match &node.block {
        Block::Section { children, .. } => {
            for child in children {
                out.push(*child);
                out.extend(nested_block_ids(graph, *child));
            }
        }
        Block::Quote { children, .. } => {
            for child in children {
                out.push(*child);
                out.extend(nested_block_ids(graph, *child));
            }
        }
        Block::List { items, .. } => {
            for item in items {
                for child in &item.children {
                    out.push(*child);
                    out.extend(nested_block_ids(graph, *child));
                }
            }
        }
        Block::Table { head, body, foot, .. } => {
            for cell in head.iter().chain(body).chain(foot).flat_map(|row| row.cells.iter()) {
                for child in &cell.children {
                    out.push(*child);
                    out.extend(nested_block_ids(graph, *child));
                }
            }
        }
        _ => {}
    }
    out
}

/// Maps every block node id to the chapter file that carries its anchor.
pub struct ChapterLinks<'a> {
    pub(super) current: &'a str,
    pub(super) chapter_of: &'a std::collections::HashMap<NodeId, String>,
}

impl ChapterLinks<'_> {
    fn href(&self, target: NodeId) -> String {
        match self.chapter_of.get(&target) {
            Some(chapter) if chapter == self.current => format!("#node-{}", target.0),
            Some(chapter) => format!("{}#node-{}", chapter, target.0),
            None => format!("#node-{}", target.0),
        }
    }
}

fn write_node(
    out: &mut String,
    graph: &DocumentGraph,
    id: NodeId,
    block: &Block,
    source_map: Option<&std::collections::HashMap<String, String>>,
    footnote_refs: &mut FootnoteRefIds,
    links: &ChapterLinks,
) -> Result<(), FormatError> {
    // Every block carries the same `node-N` anchor the HTML exporter uses, so
    // `Inline::Reference` links resolve inside the chapter instead of pointing
    // at a missing id. Without this the reference is a dangling link.
    let _ = write!(out, "<div id=\"node-{}\" data-node-id=\"{}\">", id.0, id.0);
    write_block(out, graph, block, source_map, footnote_refs, links)?;
    out.push_str("</div>\n");
    Ok(())
}

fn write_block(
    out: &mut String,
    graph: &DocumentGraph,
    block: &Block,
    source_map: Option<&std::collections::HashMap<String, String>>,
    footnote_refs: &mut FootnoteRefIds,
    links: &ChapterLinks,
) -> Result<(), FormatError> {
    match block {
        Block::Section { level, title, children } => {
            let level = (*level).clamp(1, 6);
            write!(out, "<h{}>", level).map_err(format_error)?;
            write_inlines(out, title, source_map, footnote_refs, links)?;
            write!(out, "</h{}>\n", level).map_err(format_error)?;
            for child_id in children {
                let Some(child) = graph.block(*child_id)
                else {
                    return Err(FormatError::invalid_input(format!("section child `{child_id:?}` is missing")));
                };
                write_node(out, graph, child.id, &child.block, source_map, footnote_refs, links)?;
            }
        }
        Block::Paragraph { content } => {
            out.push_str("<p>");
            write_inlines(out, content, source_map, footnote_refs, links)?;
            out.push_str("</p>\n");
        }
        Block::Quote { children } => {
            out.push_str("<blockquote>");
            for child_id in children {
                let Some(child) = graph.block(*child_id)
                else {
                    return Err(FormatError::invalid_input(format!("missing quote child {child_id:?}")));
                };
                write_node(out, graph, child.id, &child.block, source_map, footnote_refs, links)?;
            }
            out.push_str("</blockquote>\n");
        }
        Block::Code { language, content } => {
            out.push_str("<pre><code");
            if let Some(language) = language {
                out.push_str(" class=\"language-");
                escape_attribute(out, language);
                out.push('"');
            }
            out.push('>');
            escape_text(out, content);
            out.push_str("</code></pre>\n");
        }
        Block::Diagram { language, source } => {
            // Partial: no raster conversion — emit as a marked-up pre.
            out.push_str("<pre class=\"diagram\" data-diagram-language=\"");
            escape_attribute(out, language);
            out.push_str("\" data-diagram-source=\"");
            escape_attribute(out, source);
            out.push_str("\"><code>");
            escape_text(out, source);
            out.push_str("</code></pre>\n");
        }
        Block::Chart { language, source } => {
            // Partial: no chart raster — emit the authored fence as marked-up pre.
            out.push_str("<pre class=\"chart\" data-chart-language=\"");
            escape_attribute(out, language);
            out.push_str("\" data-chart-source=\"");
            escape_attribute(out, source);
            out.push_str("\"><code>");
            escape_text(out, source);
            out.push_str("</code></pre>\n");
        }
        Block::Math { formula } => {
            out.push_str("<div class=\"math\"");
            if let Some(language) = &formula.language {
                out.push_str(" data-language=\"");
                escape_attribute(out, language);
                out.push('"');
            }
            out.push('>');
            escape_text(out, &formula.source);
            out.push_str("</div>\n");
        }
        Block::List { ordered, items } => write_list(out, graph, *ordered, items, source_map, footnote_refs, links)?,
        Block::Table { caption, head, body, foot } => {
            write_table(out, graph, caption.as_deref(), head, body, foot, source_map, footnote_refs, links)?
        }
        Block::Opaque { kind, payload_hint, .. } if kind == "thematic_break" => {
            out.push_str("<hr/>");
            if !payload_hint.is_empty() {
                out.push_str("<span class=\"opaque-hint\">");
                escape_text(out, payload_hint);
                out.push_str("</span>");
            }
            out.push('\n');
        }
        Block::Opaque { kind, .. } => {
            return Err(FormatError::unsupported("epub", format!("opaque block kind `{kind}` is not representable")));
        }
        Block::RawHtml { html } => {
            out.push_str(html);
            out.push('\n');
        }
    }
    Ok(())
}

fn write_list(
    out: &mut String,
    graph: &DocumentGraph,
    ordered: bool,
    items: &[ListItem],
    source_map: Option<&std::collections::HashMap<String, String>>,
    footnote_refs: &mut FootnoteRefIds,
    links: &ChapterLinks,
) -> Result<(), FormatError> {
    let start = items.first().and_then(|item| item.number).unwrap_or(1);
    if ordered {
        if start == 1 {
            out.push_str("<ol>\n");
        }
        else {
            let _ = write!(out, "<ol start=\"{start}\">\n");
        }
    }
    else {
        out.push_str("<ul>\n");
    }
    let mut expected = start;
    for (index, item) in items.iter().enumerate() {
        let number = item.number.unwrap_or(expected);
        out.push_str("<li");
        if ordered && index > 0 && number != expected {
            let _ = write!(out, " value=\"{number}\"");
        }
        if item.is_task {
            // XHTML is a slot that can express a task marker, so the checked
            // state is written instead of silently flattening to a plain item.
            out.push_str(r#" class="task-list-item"><input type="checkbox" disabled="disabled""#);
            if item.is_checked.unwrap_or(false) {
                out.push_str(r#" checked="checked""#);
            }
            out.push_str(" />");
        }
        else {
            out.push('>');
        }
        expected = number.saturating_add(1);
        write_inlines(out, &item.content, source_map, footnote_refs, links)?;
        out.push('\n');
        for child_id in &item.children {
            let Some(child) = graph.block(*child_id)
            else {
                return Err(FormatError::invalid_input(format!("list item child `{child_id:?}` is missing")));
            };
            write_node(out, graph, child.id, &child.block, source_map, footnote_refs, links)?;
        }
        out.push_str("</li>\n");
    }
    out.push_str(if ordered { "</ol>\n" } else { "</ul>\n" });
    Ok(())
}

fn write_table(
    out: &mut String,
    graph: &DocumentGraph,
    caption: Option<&str>,
    head: &[TableRow],
    body: &[TableRow],
    foot: &[TableRow],
    source_map: Option<&std::collections::HashMap<String, String>>,
    footnote_refs: &mut FootnoteRefIds,
    links: &ChapterLinks,
) -> Result<(), FormatError> {
    out.push_str("<table>\n");
    if let Some(caption) = caption {
        out.push_str("  <caption>");
        escape_text(out, caption);
        out.push_str("</caption>\n");
    }
    for (band, tag) in [(head, "th"), (body, "td"), (foot, "td")] {
        for row in band {
            out.push_str("  <tr>\n");
            for cell in &row.cells {
                out.push_str("    <");
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
                for child_id in &cell.children {
                    let Some(child) = graph.block(*child_id)
                    else {
                        return Err(FormatError::invalid_input(format!("table cell child `{child_id:?}` is missing")));
                    };
                    write_node(out, graph, child.id, &child.block, source_map, footnote_refs, links)?;
                }
                out.push_str("</");
                out.push_str(tag);
                out.push_str(">\n");
            }
            out.push_str("  </tr>\n");
        }
    }
    out.push_str("</table>\n");
    Ok(())
}

fn write_inlines(
    out: &mut String,
    inlines: &[Inline],
    source_map: Option<&std::collections::HashMap<String, String>>,
    footnote_refs: &mut FootnoteRefIds,
    links: &ChapterLinks,
) -> Result<(), FormatError> {
    for inline in inlines {
        match inline {
            Inline::Text { text } => escape_text(out, text),
            Inline::InlineCode { text } => {
                out.push_str("<code>");
                escape_text(out, text);
                out.push_str("</code>");
            }
            Inline::InlineMath { formula } => {
                out.push_str("<span class=\"math\"");
                if let Some(language) = &formula.language {
                    out.push_str(" data-language=\"");
                    escape_attribute(out, language);
                    out.push('"');
                }
                out.push('>');
                escape_text(out, &formula.source);
                out.push_str("</span>");
            }
            Inline::Reference { display, target } => {
                let href = links.href(*target);
                out.push_str("<a href=\"");
                escape_attribute(out, &href);
                out.push_str("\">");
                escape_text(out, display);
                out.push_str("</a>");
            }
            Inline::Styled { style, children } => {
                if let Some(color) = style.strip_prefix("color:") {
                    if color.len() != 6 || !color.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        return Err(FormatError::unsupported("epub", format!("invalid color style `{style}`")));
                    }
                    write!(out, "<span style=\"color:#{color}\">").map_err(format_error)?;
                    write_inlines(out, children, source_map, footnote_refs, links)?;
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
                        // A style with no known XHTML tag still has a class slot.
                        // Keep the authored name visible instead of rejecting
                        // the whole document.
                        out.push_str("<span class=\"");
                        escape_attribute(out, style);
                        out.push_str("\">");
                        write_inlines(out, children, source_map, footnote_refs, links)?;
                        out.push_str("</span>");
                        return Ok(());
                    }
                };
                write!(out, "<{}>", tag).map_err(format_error)?;
                write_inlines(out, children, source_map, footnote_refs, links)?;
                write!(out, "</{}>", tag).map_err(format_error)?;
            }
            Inline::Link { target, display, title } => {
                let RelationEndpoint::ExternalUri(url) = target
                else {
                    return Err(FormatError::unsupported(
                        "epub",
                        format!("link target `{target:?}` has no EPUB destination mapping"),
                    ));
                };
                out.push_str("<a href=\"");
                escape_attribute(out, url);
                out.push('"');
                write_optional_title(out, title.as_deref());
                out.push('>');
                write_inlines(out, display, source_map, footnote_refs, links)?;
                out.push_str("</a>");
            }
            Inline::Image { source, alt, title, asset: _ } => {
                let href = rewrite_image_href(source, source_map);
                out.push_str("<img alt=\"");
                escape_attribute(out, &alt.iter().map(|i| i.plain_text()).collect::<String>());
                out.push_str("\" src=\"");
                escape_attribute(out, &href);
                out.push('"');
                write_optional_title(out, title.as_deref());
                out.push_str("/>");
            }
            Inline::Break { kind } => match kind {
                BreakKind::Hard => out.push_str("<br/>"),
                BreakKind::Soft => out.push('\n'),
            },
            Inline::Attributed { attributes, children }
                if matches!(children.as_slice(), [Inline::Image { .. }]) && !attributes.is_empty() =>
            {
                // A size or identity wrapper around one bitmap belongs on the
                // `<img>` element, never on a `<span>` where `width`/`height`
                // would be inert.
                let Inline::Image { source, alt, title, asset: _ } = &children[0]
                else {
                    unreachable!("the guard matched one image")
                };
                let href = rewrite_image_href(source, source_map);
                out.push_str("<img alt=\"");
                escape_attribute(out, &alt.iter().map(|i| i.plain_text()).collect::<String>());
                out.push_str("\" src=\"");
                escape_attribute(out, &href);
                out.push('"');
                write_optional_title(out, title.as_deref());
                write_attributes(out, attributes)?;
                out.push_str("/>");
            }
            Inline::Attributed { attributes, children } => {
                out.push_str("<span");
                write_attributes(out, attributes)?;
                out.push('>');
                write_inlines(out, children, source_map, footnote_refs, links)?;
                out.push_str("</span>");
            }
            Inline::Note { label, display } => {
                out.push_str(r#"<sup class="footnote-reference" data-footnote-label=""#);
                escape_attribute(out, label);
                out.push_str("\"><a id=\"");
                escape_attribute(out, &footnote_refs.next(label));
                out.push_str("\" href=\"#fn-");
                escape_attribute(out, label);
                out.push_str("\">");
                match display {
                    Some(display) => write_inlines(out, display, source_map, footnote_refs, links)?,
                    None => escape_text(out, label),
                }
                out.push_str("</a></sup>");
            }
            Inline::Citation { .. } => {
                return Err(FormatError::unsupported(
                    "epub",
                    "inline Citation is not representable by the conservative EPUB exporter",
                ));
            }
            Inline::RawHtml { html } => out.push_str(html),
        }
    }
    Ok(())
}

fn rewrite_image_href(source: &str, source_map: Option<&std::collections::HashMap<String, String>>) -> String {
    let Some(map) = source_map
    else {
        return source.to_string();
    };
    // Try exact match, then basename match.
    if let Some(mapped) = map.get(source) {
        return mapped.clone();
    }
    let basename = source.rsplit('/').next().unwrap_or(source);
    if let Some(mapped) = map.get(basename) {
        return mapped.clone();
    }
    source.to_string()
}

fn write_attributes(out: &mut String, attributes: &Attributes) -> Result<(), FormatError> {
    if let Some(id) = &attributes.id {
        out.push_str(" id=\"");
        escape_attribute(out, id);
        out.push('"');
    }
    if !attributes.classes.is_empty() {
        out.push_str(" class=\"");
        escape_attribute(out, &attributes.classes.join(" "));
        out.push('"');
    }
    for pair in &attributes.pairs {
        if !is_valid_attribute_name(&pair.key) || pair.key.eq_ignore_ascii_case("id") || pair.key.eq_ignore_ascii_case("class")
        {
            return Err(FormatError::unsupported("epub", format!("invalid attribute name `{}`", pair.key)));
        }
        out.push(' ');
        out.push_str(&pair.key);
        out.push_str("=\"");
        escape_attribute(out, &pair.value);
        out.push('"');
    }
    Ok(())
}

fn is_valid_attribute_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

fn write_optional_title(out: &mut String, title: Option<&str>) {
    if let Some(title) = title {
        out.push_str(" title=\"");
        escape_attribute(out, title);
        out.push('"');
    }
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
    FormatError::unsupported("epub", error.to_string())
}
