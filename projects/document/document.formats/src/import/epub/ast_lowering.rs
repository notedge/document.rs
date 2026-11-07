//! EPUB XHTML block lowering via typed `oak-html` AST and CSS selectors.

use document_ir::{
    Attributes, Block, BreakKind, DocumentGraph, Inline, ListItem, LossMarker, MathDisplay, SemanticStatus, TableCell, TableRow,
};
use oak_html::ast::{Element, HtmlDocument, HtmlNode};

use crate::import::{
    common::cell_children,
    math_ml::{lower_math_element, math_display, single_display_math_child},
};

use super::{
    assets::register_image_from_src,
    html_extensions::{
        attribute_value, child_elements, descendant_text, direct_child_elements, element_is, first_descendant_element,
        select_css,
    },
    xhtml::{language_from_class, normalize_whitespace, push_inline, quote_block_from_blocks, register_and_image_inline},
};

/// Lower a parsed HTML document into semantic blocks and import losses.
pub fn blocks_from_html_document(
    document: &HtmlDocument,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
) -> (Vec<Block>, Vec<LossMarker>) {
    let mut blocks = Vec::new();
    let mut losses = Vec::new();
    let scope = body_elements(document);
    for element in scope {
        collect_semantic_blocks(element, &mut blocks, &mut losses, graph, member_path, false);
    }
    (blocks, losses)
}

fn body_elements(document: &HtmlDocument) -> Vec<&Element> {
    if let Ok(bodies) = select_css(document, "body") {
        if let Some(body) = bodies.first() {
            return direct_child_elements(body);
        }
    }
    document
        .nodes
        .iter()
        .filter_map(|node| match node {
            HtmlNode::Element(element) => Some(element),
            _ => None,
        })
        .collect()
}

fn collect_semantic_blocks(
    element: &Element,
    blocks: &mut Vec<Block>,
    losses: &mut Vec<LossMarker>,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
    inside_list_item: bool,
) {
    let tag = element.tag_name.to_ascii_lowercase();
    if tag == "li" {
        for child in direct_child_elements(element) {
            collect_semantic_blocks(child, blocks, losses, graph, member_path, true);
        }
        return;
    }
    if inside_list_item && (tag == "ul" || tag == "ol") {
        return;
    }
    if matches!(tag.as_str(), "section" | "article" | "main" | "div" | "figure") {
        blocks.extend(collect_child_blocks(element, graph, member_path, losses));
        return;
    }
    if let Some(block) = lower_semantic_block(element, &tag, losses, graph, member_path) {
        blocks.push(block);
        return;
    }
    for child in direct_child_elements(element) {
        collect_semantic_blocks(child, blocks, losses, graph, member_path, inside_list_item);
    }
}

fn collect_child_blocks(
    element: &Element,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
    losses: &mut Vec<LossMarker>,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    for child in direct_child_elements(element) {
        collect_semantic_blocks(child, &mut blocks, losses, graph, member_path, false);
    }
    blocks
}

fn lower_semantic_block(
    element: &Element,
    tag: &str,
    losses: &mut Vec<LossMarker>,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
) -> Option<Block> {
    note_inline_style_loss(element, losses);
    match tag {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = tag[1..].parse::<u8>().unwrap_or(1);
            let plain = descendant_text(element);
            let mut title = if plain.is_empty() {
                collect_inlines_from_element(element, graph, member_path, losses)
            }
            else {
                vec![Inline::Text { text: plain }]
            };
            trim_inline_edges(&mut title);
            Some(Block::Section { level, title, children: Vec::new() })
        }
        "p" => {
            if let Some(math) = single_display_math_child(element) {
                return Some(Block::Math { formula: lower_math(math, MathDisplay::Display, losses) });
            }
            let mut content =
                non_empty_inlines(collect_inlines_from_element(element, graph, member_path, losses)).or_else(|| {
                    let plain = descendant_text(element);
                    if plain.is_empty() { None } else { Some(vec![Inline::Text { text: plain }]) }
                })?;
            trim_inline_edges(&mut content);
            Some(Block::Paragraph { content })
        }
        "math" => Some(Block::Math { formula: lower_math(element, math_display(element), losses) }),
        "figcaption" => lower_figcaption_block(element, graph, member_path, losses),
        "ul" | "ol" => {
            let ordered = tag == "ol";
            // Ordered items must carry their number: the IR validator refuses
            // lists whose items lack one. `<ol start="n">` shifts the sequence,
            // unordered items stay unnumbered.
            let start = ordered.then(|| ordered_list_start(element));
            let items = collect_list_items(element, ordered, start, graph, member_path, losses);
            if items.is_empty() { None } else { Some(Block::List { ordered, items }) }
        }
        "blockquote" => {
            let inner = collect_child_blocks(element, graph, member_path, losses);
            if inner.is_empty() {
                let plain = descendant_text(element);
                if plain.is_empty() {
                    None
                }
                else {
                    let child = graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: plain }] });
                    Some(Block::Quote { children: vec![child] })
                }
            }
            else {
                Some(quote_block_from_blocks(inner, graph))
            }
        }
        "pre" => {
            let (language, content) = extract_pre_code_content(element);
            if content.is_empty() { None } else { Some(Block::Code { language, content }) }
        }
        "hr" => {
            Some(Block::Opaque { kind: "thematic_break".into(), payload_hint: "---".into(), status: SemanticStatus::Partial })
        }
        "script" | "head" | "meta" | "title" | "html" | "body" | "section" | "article" | "main" | "div" | "nav" | "figure" => {
            None
        }
        "style" => {
            losses.push(LossMarker {
                code: "import.epub.css_inline_block_unsupported".into(),
                message: "embedded <style> blocks are not lowered into Notedown IR yet".into(),
                status: SemanticStatus::Unsupported,
            });
            None
        }
        "link" => {
            let rel = attribute_value(element, "rel").unwrap_or_default();
            if rel.eq_ignore_ascii_case("stylesheet") {
                losses.push(LossMarker {
                    code: "import.epub.css_link_unsupported".into(),
                    message: "linked stylesheets are registered as package assets but not applied during import".into(),
                    status: SemanticStatus::Unsupported,
                });
            }
            None
        }
        "img" => {
            let src = attribute_value(element, "src").unwrap_or_default();
            let alt = attribute_value(element, "alt").unwrap_or_default();
            register_and_image_inline(&src, &alt, graph, member_path).map(|inline| Block::Paragraph { content: vec![inline] })
        }
        "svg" => extract_svg_image_href(element)
            .and_then(|(href, alt)| {
                register_and_image_inline(&href, &alt, graph, member_path)
                    .map(|inline| Block::Paragraph { content: vec![inline] })
            })
            .or_else(|| {
                losses.push(LossMarker {
                    code: "import.epub.svg_inline_unsupported".into(),
                    message: "inline <svg> without an external <image> reference is not lowered yet".into(),
                    status: SemanticStatus::Unsupported,
                });
                None
            }),
        "aside" if is_footnote_aside(element) => lower_footnote_definition(element, losses),
        "table" => {
            let (head, body) = collect_table_rows(element, graph, member_path, losses);
            if head.is_empty() && body.is_empty() {
                None
            }
            else {
                Some(Block::Table { caption: None, head, body, foot: Vec::new() })
            }
        }
        _ => {
            losses.push(LossMarker {
                code: "import.epub.unsupported_xhtml_block".into(),
                message: format!("unsupported xhtml block element `<{tag}>`"),
                status: SemanticStatus::Unsupported,
            });
            None
        }
    }
}

fn non_empty_inlines(inlines: Vec<Inline>) -> Option<Vec<Inline>> {
    if inlines.is_empty() { None } else { Some(inlines) }
}

/// Trim whitespace that only separates a block from its surrounding markup.
///
/// `normalize_whitespace` keeps edge spaces so inline runs stay readable across
/// element boundaries. A block trims the outer edges after its inline run is
/// complete, so authored indentation never reaches the IR.
fn trim_inline_edges(inlines: &mut Vec<Inline>) {
    if let Some(Inline::Text { text }) = inlines.first_mut() {
        *text = text.trim_start().to_string();
    }
    if let Some(Inline::Text { text }) = inlines.last_mut() {
        *text = text.trim_end().to_string();
    }
    inlines.retain(|inline| !matches!(inline, Inline::Text { text } if text.is_empty()));
}

/// Whether an `<aside>` declares the EPUB structural footnote marker.
fn is_footnote_aside(element: &Element) -> bool {
    attribute_value(element, "epub:type")
        .is_some_and(|value| value.split_whitespace().any(|token| token.eq_ignore_ascii_case("footnote")))
}

/// Lower `<aside epub:type="footnote">` to the opaque authored payload shape.
///
/// A GFM definition body is single-paragraph unless it continues with a blank
/// line and four-space indentation, so every authored block after the first
/// becomes a continuation instead of being joined into the first sentence.
/// Inline markup is flattened to text and recorded as a loss because the
/// payload carries text, not an inline tree.
///
/// The label follows the HTML endnotes path: `data-footnote-label` wins,
/// otherwise the `id` drops an `fn` prefix and an optional `-`. A missing label
/// records an explicit loss so the body stays as ordinary content instead of
/// becoming an unusable definition.
fn lower_footnote_definition(element: &Element, losses: &mut Vec<LossMarker>) -> Option<Block> {
    let label = attribute_value(element, "data-footnote-label")
        .or_else(|| attribute_value(element, "id").and_then(|id| footnote_label(&id)))
        .map(|label| label.trim().to_string())
        .filter(|label| !label.is_empty());
    let Some(label) = label
    else {
        losses.push(LossMarker {
            code: "import.epub.footnote_definition_unlabelled".into(),
            message: "`<aside epub:type=\"footnote\">` has neither `id` nor `data-footnote-label`, so its body stays as ordinary content"
                .into(),
            status: SemanticStatus::Partial,
        });
        return None;
    };
    let (paragraphs, flattened_markup) = footnote_body_paragraphs(element);
    if flattened_markup {
        losses.push(LossMarker {
            code: "import.epub.footnote_inline_markup_flattened".into(),
            message: "`<aside epub:type=\"footnote\">` inline markup is flattened to plain text in the definition payload"
                .into(),
            status: SemanticStatus::Partial,
        });
    }
    let body = paragraphs.join("\n\n    ");
    Some(Block::Opaque {
        kind: "footnote_definition".into(),
        payload_hint: format!("[^{label}]: {body}"),
        status: SemanticStatus::Resolved,
    })
}

/// Collect the definition body paragraph by paragraph without its return link.
///
/// Block children start a new paragraph while inline children stay in the
/// current one. Any authored inline markup sets the flattening flag so the
/// caller can record that the payload carries plain text only.
fn footnote_body_paragraphs(element: &Element) -> (Vec<String>, bool) {
    let mut paragraphs = Vec::new();
    let mut current = String::new();
    let mut flattened_markup = false;
    collect_footnote_paragraphs(element, &mut paragraphs, &mut current, &mut flattened_markup);
    push_footnote_paragraph(&mut paragraphs, &mut current);
    (paragraphs, flattened_markup)
}

fn collect_footnote_paragraphs(
    element: &Element,
    paragraphs: &mut Vec<String>,
    current: &mut String,
    flattened_markup: &mut bool,
) {
    for child in &element.children {
        match child {
            HtmlNode::Text(text) => current.push_str(&text.content),
            HtmlNode::Element(child) if is_footnote_backlink(child) => {}
            HtmlNode::Comment(_) => {}
            HtmlNode::Element(child) => {
                let tag = child.tag_name.to_ascii_lowercase();
                if is_footnote_body_block(&tag) {
                    push_footnote_paragraph(paragraphs, current);
                    collect_footnote_paragraphs(child, paragraphs, current, flattened_markup);
                    push_footnote_paragraph(paragraphs, current);
                }
                else {
                    if is_footnote_markup_tag(&tag) {
                        *flattened_markup = true;
                    }
                    collect_footnote_paragraphs(child, paragraphs, current, flattened_markup);
                }
            }
        }
    }
}

fn push_footnote_paragraph(paragraphs: &mut Vec<String>, current: &mut String) {
    let text = current.split_whitespace().collect::<Vec<_>>().join(" ");
    current.clear();
    if !text.is_empty() {
        paragraphs.push(text);
    }
}

/// Block tags that start a new definition paragraph.
fn is_footnote_body_block(tag: &str) -> bool {
    matches!(
        tag,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "figcaption"
            | "figure"
            | "footer"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "ul"
    )
}

/// Inline tags whose semantics the text payload cannot keep.
fn is_footnote_markup_tag(tag: &str) -> bool {
    matches!(
        tag,
        "a" | "b"
            | "big"
            | "cite"
            | "code"
            | "dfn"
            | "em"
            | "font"
            | "i"
            | "kbd"
            | "mark"
            | "q"
            | "s"
            | "samp"
            | "small"
            | "span"
            | "strong"
            | "sub"
            | "sup"
            | "u"
            | "var"
    )
}

/// Whether an anchor inside a footnote definition returns to the reference.
///
/// EPUB authors commonly close the aside with `<a href="#fnref1">↩</a>`. The
/// return target and the short glyph or `return` copy are presentation, not
/// definition body, so both are stripped from the opaque payload.
fn is_footnote_backlink(element: &Element) -> bool {
    if !element_is(element, "a") {
        return false;
    }
    let returns_to_reference = attribute_value(element, "href").is_some_and(|href| {
        href.trim().strip_prefix('#').is_some_and(|fragment| fragment.trim().to_ascii_lowercase().starts_with("fnref"))
    });
    if returns_to_reference {
        return true;
    }
    let text = descendant_text(element).to_ascii_lowercase();
    matches!(text.as_str(), "↩" | "↩︎" | "⟲" | "return" | "return to text" | "back to text")
}

/// Whether an anchor declares an EPUB footnote reference marker.
fn is_footnote_reference(element: &Element) -> bool {
    attribute_value(element, "epub:type")
        .is_some_and(|value| value.split_whitespace().any(|token| token.eq_ignore_ascii_case("noteref")))
        || attribute_value(element, "role").is_some_and(|role| role.eq_ignore_ascii_case("doc-noteref"))
}

/// Collapse `<a epub:type="noteref" href="#fn1">` to `Inline::Note`.
///
/// The HTML path's label and display rules apply: the label comes from
/// `data-footnote-label` or the fragment with an `fn` prefix (plus optional
/// `-`) removed, and the authored marker text only survives as a custom
/// display when it differs from the label or `[^label]`. A reference whose
/// label cannot be resolved falls back to the ordinary anchor path.
fn footnote_reference_inline(element: &Element) -> Option<Inline> {
    let from_href = attribute_value(element, "href")
        .and_then(|href| href.trim().strip_prefix('#').map(|fragment| footnote_label(fragment)))
        .flatten();
    let label = attribute_value(element, "data-footnote-label")
        .or(from_href)
        .map(|label| label.trim().to_string())
        .filter(|label| !label.is_empty())?;
    let text = descendant_text(element);
    let gfm_marker = format!("[^{label}]");
    let is_default = text.is_empty() || text == label || text == gfm_marker;
    let display = (!is_default).then(|| vec![Inline::Text { text }]);
    Some(Inline::Note { label, display })
}

/// Normalize a fragment or element id into a footnote label.
///
/// Both sides of the reference/definition pair use this, so `#fn-1` and
/// `id="fn-1"` agree while an authored id such as `note1` is kept verbatim.
fn footnote_label(value: &str) -> Option<String> {
    let value = value.trim().strip_prefix("fn").map(|rest| rest.trim_start_matches('-')).unwrap_or(value.trim());
    (!value.is_empty()).then(|| value.to_string())
}

fn lower_figcaption_block(
    element: &Element,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
    losses: &mut Vec<LossMarker>,
) -> Option<Block> {
    let mut content = non_empty_inlines(collect_inlines_from_element(element, graph, member_path, losses)).or_else(|| {
        let plain = descendant_text(element);
        if plain.is_empty() { None } else { Some(vec![Inline::Text { text: plain }]) }
    })?;
    trim_inline_edges(&mut content);
    Some(Block::Paragraph { content: vec![Inline::Styled { style: "figcaption".into(), children: content }] })
}

fn collect_list_items(
    element: &Element,
    ordered: bool,
    start: Option<u32>,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
    losses: &mut Vec<LossMarker>,
) -> Vec<ListItem> {
    // The IR only carries positive, strictly increasing ordinals. HTML's
    // `reversed` asks for descending ordinals, which the model cannot express,
    // so the sequence stays ascending and the downgrade is recorded.
    if ordered && has_attribute(element, "reversed") {
        losses.push(LossMarker {
            code: "import.epub.list_reversed_unsupported".into(),
            message: "`<ol reversed>` descending ordinals are not representable in the IR list model, so items are numbered ascending"
                .into(),
            status: SemanticStatus::Partial,
        });
    }
    let mut next = start.unwrap_or(1);
    child_elements(element, "li")
        .into_iter()
        .map(|li| {
            let mut item_number = ordered.then_some(next);
            if ordered {
                // `<li value>` moves the ordinal for this item and the following
                // run. Honour it while it stays strictly increasing. A backward
                // reset would violate the IR invariant, so the authored value is
                // recorded and the running counter continues instead.
                match attribute_value(li, "value").map(|value| value.trim().parse::<u32>()) {
                    Some(Ok(value)) if value > 0 && value >= next => {
                        item_number = Some(value);
                        next = value.saturating_add(1);
                    }
                    Some(_) => {
                        losses.push(LossMarker {
                            code: "import.epub.list_item_value_unsupported".into(),
                            message:
                                "`<li value>` would break the strictly increasing ordinal invariant, so the running sequence continues"
                                    .into(),
                            status: SemanticStatus::Partial,
                        });
                        next = next.saturating_add(1);
                    }
                    None => next = next.saturating_add(1),
                }
            }
            lower_list_item(li, item_number, graph, member_path, losses)
        })
        .collect()
}

fn has_attribute(element: &Element, name: &str) -> bool {
    element.attributes.iter().any(|attribute| attribute.name.eq_ignore_ascii_case(name))
}

fn lower_list_item(
    li: &Element,
    number: Option<u32>,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
    losses: &mut Vec<LossMarker>,
) -> ListItem {
    let mut content = Vec::new();
    let mut children = Vec::new();
    for child in &li.children {
        match child {
            HtmlNode::Text(text) => {
                let normalized = normalize_whitespace(&text.content);
                if !normalized.is_empty() {
                    push_inline(&mut content, Inline::Text { text: normalized });
                }
            }
            HtmlNode::Element(child_element) => {
                note_inline_style_loss(child_element, losses);
                let tag = child_element.tag_name.to_ascii_lowercase();
                match tag.as_str() {
                    "ul" | "ol" => {
                        let nested_ordered = tag == "ol";
                        let nested_start = nested_ordered.then(|| ordered_list_start(child_element));
                        let nested =
                            collect_list_items(child_element, nested_ordered, nested_start, graph, member_path, losses);
                        if !nested.is_empty() {
                            let id = graph.push_block(Block::List { ordered: nested_ordered, items: nested });
                            children.push(id);
                        }
                    }
                    "p" => {
                        for inline in collect_inlines_from_element(child_element, graph, member_path, losses) {
                            push_inline(&mut content, inline);
                        }
                    }
                    _ => push_inline_element(child_element, graph, member_path, losses, &mut content),
                }
            }
            HtmlNode::Comment(_) => {}
        }
    }
    trim_inline_edges(&mut content);
    ListItem { content, children, number, is_task: false, is_checked: None }
}

/// Read the `start` ordinal of an `<ol>`.
///
/// HTML allows any integer there, but the IR only carries positive, strictly
/// increasing ordinals, so a non-positive or malformed value falls back to the
/// default sequence.
fn ordered_list_start(element: &Element) -> u32 {
    attribute_value(element, "start").and_then(|value| value.trim().parse::<u32>().ok()).filter(|value| *value > 0).unwrap_or(1)
}

fn collect_table_rows(
    element: &Element,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
    losses: &mut Vec<LossMarker>,
) -> (Vec<TableRow>, Vec<TableRow>) {
    let mut rows = Vec::new();
    walk_table_rows(element, graph, member_path, losses, &mut rows);
    let mut head = Vec::new();
    let mut body = Vec::new();
    let mut leading_header = true;
    for (is_header, row) in rows {
        if leading_header && is_header {
            head.push(row);
        }
        else {
            leading_header = false;
            body.push(row);
        }
    }
    (head, body)
}

fn walk_table_rows(
    element: &Element,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
    losses: &mut Vec<LossMarker>,
    rows: &mut Vec<(bool, TableRow)>,
) {
    if element_is(element, "tr") {
        rows.push(collect_table_row(element, graph, member_path, losses));
        return;
    }
    for child in direct_child_elements(element) {
        walk_table_rows(child, graph, member_path, losses, rows);
    }
}

fn collect_table_row(
    row: &Element,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
    losses: &mut Vec<LossMarker>,
) -> (bool, TableRow) {
    let mut is_header = false;
    let mut cells = Vec::new();
    for child in direct_child_elements(row) {
        let tag = child.tag_name.to_ascii_lowercase();
        if tag == "th" || tag == "td" {
            if tag == "th" {
                is_header = true;
            }
            let mut content = collect_inlines_from_element(child, graph, member_path, losses);
            trim_inline_edges(&mut content);
            cells.push(TableCell { children: cell_children(graph, content), colspan: 1, rowspan: 1 });
        }
    }
    (is_header, TableRow { cells })
}

fn lower_math(element: &Element, display: MathDisplay, losses: &mut Vec<LossMarker>) -> document_ir::MathFormula {
    let (formula, unsupported) = lower_math_element(element, display);
    if !unsupported.is_empty() {
        losses.push(LossMarker {
            code: "import.epub.math_unsupported".into(),
            message: format!(
                "math constructs {} have no structured mapping and were kept as readable text inside the formula",
                unsupported.join(", ")
            ),
            status: SemanticStatus::Partial,
        });
    }
    formula
}

fn collect_inlines_from_element(
    element: &Element,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
    losses: &mut Vec<LossMarker>,
) -> Vec<Inline> {
    let mut inlines = Vec::new();
    for child in &element.children {
        match child {
            HtmlNode::Text(text) => {
                let normalized = normalize_whitespace(&text.content);
                if !normalized.is_empty() {
                    push_inline(&mut inlines, Inline::Text { text: normalized });
                }
            }
            HtmlNode::Element(child_element) => {
                note_inline_style_loss(child_element, losses);
                push_inline_element(child_element, graph, member_path, losses, &mut inlines);
            }
            HtmlNode::Comment(_) => {}
        }
    }
    inlines
}

/// Lower one inline element into the surrounding run. A span is a transparent
/// grouping node, so its children are spliced in place instead of inventing an
/// unnamed style. Authored span attributes stay as an attributed wrapper.
fn push_inline_element(
    element: &Element,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
    losses: &mut Vec<LossMarker>,
    inlines: &mut Vec<Inline>,
) {
    if element.tag_name.eq_ignore_ascii_case("span") {
        let children = collect_inlines_from_element(element, graph, member_path, losses);
        let attributes = inline_attributes(element);
        if attributes.is_empty() {
            for inline in children {
                push_inline(inlines, inline);
            }
        }
        else if !children.is_empty() {
            push_inline(inlines, Inline::Attributed { attributes, children });
        }
        return;
    }
    if let Some(inline) = lower_inline_element(element, graph, member_path, losses) {
        push_inline(inlines, inline);
    }
}

fn lower_inline_element(
    element: &Element,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
    losses: &mut Vec<LossMarker>,
) -> Option<Inline> {
    let tag = element.tag_name.to_ascii_lowercase();
    // Footnote references are semantic inlines, not external links. Return
    // before the attribute wrapper so the note label is not hidden inside an
    // `Inline::Attributed` the Markdown writer cannot spell.
    if is_footnote_reference(element) {
        if let Some(note) = footnote_reference_inline(element) {
            return Some(note);
        }
    }
    let lowered = match tag.as_str() {
        "img" => {
            let src = attribute_value(element, "src").unwrap_or_default();
            let alt = attribute_value(element, "alt").unwrap_or_default();
            register_and_image_inline(&src, &alt, graph, member_path)
        }
        "strong" | "b" => Some(Inline::Styled {
            style: "bold".into(),
            children: collect_inlines_from_element(element, graph, member_path, losses),
        }),
        "em" | "i" => Some(Inline::Styled {
            style: "italic".into(),
            children: collect_inlines_from_element(element, graph, member_path, losses),
        }),
        "code" => Some(Inline::InlineCode { text: descendant_text(element) }),
        "a" => {
            let children = collect_inlines_from_element(element, graph, member_path, losses);
            let href = attribute_value(element, "href").unwrap_or_default();
            if href.is_empty() {
                children.first().cloned()
            }
            else {
                Some(Inline::Link {
                    target: document_ir::RelationEndpoint::ExternalUri(href.clone()),
                    display: if children.is_empty() { vec![Inline::Text { text: href }] } else { children },
                    title: attribute_value(element, "title"),
                })
            }
        }
        "math" => Some(Inline::InlineMath { formula: lower_math(element, math_display(element), losses) }),
        "br" => Some(Inline::Break { kind: BreakKind::Hard }),
        "s" | "del" | "strike" => Some(Inline::Styled {
            style: "strike".into(),
            children: collect_inlines_from_element(element, graph, member_path, losses),
        }),
        "u" => Some(Inline::Styled {
            style: "underline".into(),
            children: collect_inlines_from_element(element, graph, member_path, losses),
        }),
        "sup" | "sub" => {
            let style = if tag == "sup" { "superscript" } else { "subscript" };
            Some(Inline::Styled {
                style: style.into(),
                children: collect_inlines_from_element(element, graph, member_path, losses),
            })
        }
        _ => {
            let text = descendant_text(element);
            if text.is_empty() { None } else { Some(Inline::Text { text }) }
        }
    };
    let attributes = inline_attributes(element);
    if attributes.is_empty() {
        lowered
    }
    else {
        Some(Inline::Attributed { attributes, children: lowered.into_iter().collect() })
    }
}

/// Collect the identity and class attributes of an inline element so authored
/// anchors and style hooks survive into the semantic layer.
fn inline_attributes(element: &Element) -> Attributes {
    let mut attributes = Attributes::default();
    if let Some(id) = attribute_value(element, "id").map(|value| value.trim().to_string()).filter(|value| !value.is_empty()) {
        attributes.id = Some(id);
    }
    if let Some(classes) = attribute_value(element, "class") {
        attributes.classes = classes.split_whitespace().map(str::to_string).collect();
    }
    attributes
}

fn note_inline_style_loss(element: &Element, losses: &mut Vec<LossMarker>) {
    if attribute_value(element, "style").is_some() {
        losses.push(LossMarker {
            code: "import.epub.inline_style_unsupported".into(),
            message: "inline style attributes are not lowered into Notedown IR yet".into(),
            status: SemanticStatus::Unsupported,
        });
    }
}

fn extract_svg_image_href(element: &Element) -> Option<(String, String)> {
    if element_is(element, "image") {
        let href = attribute_value(element, "href").or_else(|| attribute_value(element, "xlink:href"))?;
        if href.is_empty() {
            return None;
        }
        let alt = attribute_value(element, "alt").or_else(|| attribute_value(element, "aria-label")).unwrap_or_default();
        return Some((href, alt));
    }
    for child in direct_child_elements(element) {
        if let Some(found) = extract_svg_image_href(child) {
            return Some(found);
        }
    }
    None
}

fn extract_pre_code_content(element: &Element) -> (Option<String>, String) {
    if let Some(code) = first_descendant_element(element, "code") {
        let language = attribute_value(code, "class").as_deref().map(language_from_class);
        return (language, descendant_text(code));
    }
    (None, descendant_text(element))
}

/// Register embedded image assets from an already parsed HTML document.
pub fn register_spine_images_from_document(
    document: &HtmlDocument,
    graph: &mut DocumentGraph,
    member_path: Option<&str>,
) -> Result<(), crate::FormatError> {
    let Some(member_path) = member_path
    else {
        return Ok(());
    };
    for img in select_css(document, "img[src]")? {
        if let Some(src) = attribute_value(img, "src") {
            register_image_from_src(graph, member_path, &src);
        }
    }
    for image in select_css(document, "svg image")? {
        let href = attribute_value(image, "href").or_else(|| attribute_value(image, "xlink:href"));
        if let Some(href) = href {
            register_image_from_src(graph, member_path, &href);
        }
    }
    Ok(())
}
