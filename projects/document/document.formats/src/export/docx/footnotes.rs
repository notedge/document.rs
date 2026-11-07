use std::collections::{BTreeMap, BTreeSet};

use document_ir::{Block, DocumentGraph, Inline};

use crate::FormatError;

/// Footnote bodies from `footnote_definition` opaque blocks, keyed by the
/// authored label, plus the integer id WordprocessingML assigns to each label.
///
/// `w:footnoteReference` only accepts integer ids while GFM and the IR address
/// notes by arbitrary labels such as `[^note]`. Numeric labels keep their own
/// value so `[^1]` still exports with `w:id="1"`. Every other label takes the
/// next free id in sorted label order, which keeps the body and the reference
/// instead of failing the whole document.
#[derive(Debug, Default, Clone)]
pub struct FootnoteCatalog {
    bodies: BTreeMap<String, String>,
    ids: BTreeMap<String, u32>,
}

impl FootnoteCatalog {
    /// Whether any footnote body resolved from the graph.
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    /// WordprocessingML id for an authored label.
    pub fn id(&self, label: &str) -> Option<u32> {
        self.ids.get(label).copied()
    }

    /// Bodies paired with their ids in ascending id order.
    pub fn bodies(&self) -> impl Iterator<Item = (u32, &str)> {
        let mut entries: Vec<(u32, &str)> =
            self.ids.iter().filter_map(|(label, id)| self.bodies.get(label).map(|body| (*id, body.as_str()))).collect();
        entries.sort_unstable_by_key(|(id, _)| *id);
        entries.into_iter()
    }
}

/// Collect footnote bodies from `footnote_definition` opaque blocks and assign
/// the ids that `word/footnotes.xml` and the body references share.
pub fn collect_footnote_catalog(graph: &DocumentGraph) -> FootnoteCatalog {
    let mut bodies = BTreeMap::new();
    for node in &graph.blocks {
        if let Block::Opaque { kind, payload_hint, .. } = &node.block {
            if kind == "footnote_definition" {
                if let Some((label, body)) = parse_footnote_definition(payload_hint) {
                    bodies.insert(label, body);
                }
            }
        }
    }
    // An inline note may carry its body in `Note.display` without a separate
    // `footnote_definition` block. `render_footnotes_xml` already writes
    // catalog bodies, so collecting the display here is what keeps the whole
    // note body from disappearing on export. An explicit definition still wins
    // when both forms name the same label.
    for node in &graph.blocks {
        match &node.block {
            Block::Section { title, .. } => collect_inline_notes(title, &mut bodies),
            Block::Paragraph { content } => collect_inline_notes(content, &mut bodies),
            Block::List { items, .. } => {
                for item in items {
                    collect_inline_notes(&item.content, &mut bodies);
                }
            }
            _ => {}
        }
    }

    let mut used = BTreeSet::new();
    let mut ids = BTreeMap::new();
    for label in bodies.keys() {
        if let Ok(value) = label.parse::<u32>() {
            if used.insert(value) {
                ids.insert(label.clone(), value);
            }
        }
    }
    let mut next = used.last().copied().unwrap_or(0).saturating_add(1);
    for label in bodies.keys() {
        if ids.contains_key(label) {
            continue;
        }
        while used.contains(&next) {
            next = next.saturating_add(1);
        }
        used.insert(next);
        ids.insert(label.clone(), next);
        next = next.saturating_add(1);
    }

    FootnoteCatalog { bodies, ids }
}

/// Collect inline note bodies, including notes nested in styled or linked runs.
fn collect_inline_notes(inlines: &[Inline], bodies: &mut BTreeMap<String, String>) {
    for inline in inlines {
        match inline {
            Inline::Note { label, display: Some(display) } => {
                let body: String = display.iter().map(Inline::plain_text).collect();
                let body = body.trim();
                if !body.is_empty() {
                    bodies.entry(label.clone()).or_insert_with(|| body.to_string());
                }
                collect_inline_notes(display, bodies);
            }
            Inline::Styled { children, .. } | Inline::Attributed { children, .. } | Inline::Link { display: children, .. } => {
                collect_inline_notes(children, bodies)
            }
            Inline::Image { alt, .. } => collect_inline_notes(alt, bodies),
            Inline::Citation { display: Some(display), .. } => collect_inline_notes(display, bodies),
            _ => {}
        }
    }
}

/// Parse GFM footnote definition text such as `[^1]: body` or `[^note]: body`.
pub fn parse_footnote_definition(payload: &str) -> Option<(String, String)> {
    let trimmed = payload.trim();
    if !trimmed.starts_with("[^") {
        return None;
    }
    let close = trimmed.find(']')?;
    let label = trimmed[2..close].trim();
    let body =
        trimmed.get(close + 1..).and_then(|rest| rest.strip_prefix(':')).map(str::trim).filter(|body| !body.is_empty())?;
    Some((label.to_string(), body.to_string()))
}

/// Parse an inline footnote reference token such as `[^1]` into a numeric
/// label. Named labels are resolved through [`FootnoteCatalog::id`] instead.
pub fn parse_footnote_reference_token(token: &str) -> Option<u32> {
    let trimmed = token.trim();
    if !trimmed.starts_with("[^") || !trimmed.ends_with(']') {
        return None;
    }
    trimmed[2..trimmed.len() - 1].trim().parse().ok()
}

/// Render `word/footnotes.xml` for the given catalog.
pub fn render_footnotes_xml(catalog: &FootnoteCatalog) -> Result<String, FormatError> {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    );
    // Word expects the separator and continuation separator parts before the
    // content notes. Their reserved ids are -1 and 0, which never collide with
    // the catalog ids that start at 1.
    xml.push_str(
        r#"
  <w:footnote w:type="separator" w:id="-1"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:separator/></w:r></w:p></w:footnote>
  <w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>"#,
    );
    for (id, body) in catalog.bodies() {
        xml.push_str("\n  <w:footnote w:id=\"");
        xml.push_str(&id.to_string());
        xml.push_str("\"><w:p><w:r><w:t>");
        write_xml_text(&mut xml, body)?;
        xml.push_str("</w:t></w:r></w:p></w:footnote>");
    }
    xml.push_str("\n</w:footnotes>");
    Ok(xml)
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
