use oak_core::{Builder, SourceText, parser::session::ParseSession};
use oak_html::{
    HtmlBuilder, HtmlLanguage,
    ast::{Element, HtmlDocument, HtmlNode, Text},
    query::{HtmlDocumentView, select_css_elements},
};

use crate::{
    FormatError,
    query_gate::{QueryLimits, ensure_complete},
};

/// Parses XHTML or HTML bytes into a typed `oak-html` document.
pub fn parse_html_bytes(html: &[u8]) -> Result<HtmlDocument, FormatError> {
    let text = std::str::from_utf8(html).map_err(|error| FormatError::parse("epub", format!("html is not utf-8: {error}")))?;
    parse_html_text(text)
}

/// Parses XHTML or HTML text into a typed `oak-html` document.
pub fn parse_html_text(text: &str) -> Result<HtmlDocument, FormatError> {
    let normalized = crate::import::math_ml::normalize_self_closing_elements_expanded(text);
    let text = normalized.as_ref();
    let language = HtmlLanguage::default();
    let source = SourceText::new(text);
    let builder = HtmlBuilder::new(language);
    let mut cache = ParseSession::<HtmlLanguage>::default();
    let built = builder.build(&source, &[], &mut cache);
    let mut document = built.result.map_err(|error| FormatError::parse("epub", error.to_string()))?;
    restore_text_edge_whitespace(&mut document, text);
    Ok(document)
}

/// Restore whitespace that `oak-html` trims from text tokens.
///
/// Its builder collapses whitespace inside each token, which also drops the
/// space that separates text from adjacent inline markup. The token span keeps
/// pointing at the text itself, so an authored space immediately before or
/// after the span is put back and the block assembly later trims only the outer
/// run edges.
fn restore_text_edge_whitespace(document: &mut HtmlDocument, source: &str) {
    for node in &mut document.nodes {
        restore_node_edge_whitespace(node, source);
    }
}

fn restore_node_edge_whitespace(node: &mut HtmlNode, source: &str) {
    match node {
        HtmlNode::Text(text) => restore_one_text_edge_whitespace(text, source),
        HtmlNode::Element(element) => {
            for child in &mut element.children {
                restore_node_edge_whitespace(child, source);
            }
        }
        HtmlNode::Comment(_) => {}
    }
}

fn restore_one_text_edge_whitespace(text: &mut Text, source: &str) {
    if text.content.is_empty() {
        return;
    }
    let leading = source.get(..text.span.start).and_then(|prefix| prefix.chars().next_back()).is_some_and(char::is_whitespace);
    let trailing = source.get(text.span.end..).and_then(|suffix| suffix.chars().next()).is_some_and(char::is_whitespace);
    if leading && !text.content.starts_with(char::is_whitespace) {
        text.content.insert(0, ' ');
    }
    if trailing && !text.content.ends_with(char::is_whitespace) {
        text.content.push(' ');
    }
}

/// Returns whether an element's tag name matches `expected` case-insensitively.
pub fn element_is(element: &Element, expected: &str) -> bool {
    element.tag_name.eq_ignore_ascii_case(expected)
}

/// Reads an attribute by name case-insensitively.
pub fn attribute_value(element: &Element, expected: &str) -> Option<String> {
    element.attributes.iter().find(|attr| attr.name.eq_ignore_ascii_case(expected)).and_then(|attr| attr.value.clone())
}

/// Returns all direct child elements regardless of tag name.
pub fn direct_child_elements<'a>(element: &'a Element) -> Vec<&'a Element> {
    element.children.iter().filter_map(as_element).collect()
}

/// Returns direct child elements whose tag name matches `expected`.
pub fn child_elements<'a>(element: &'a Element, expected: &str) -> Vec<&'a Element> {
    element.children.iter().filter_map(as_element).filter(|child| element_is(child, expected)).collect()
}

fn as_element(node: &HtmlNode) -> Option<&Element> {
    match node {
        HtmlNode::Element(element) => Some(element),
        _ => None,
    }
}

/// Returns trimmed direct text content for an element.
pub fn direct_text(element: &Element) -> String {
    let mut text = String::new();
    for child in &element.children {
        if let HtmlNode::Text(Text { content, .. }) = child {
            text.push_str(content);
        }
    }
    text.trim().to_string()
}

/// Returns trimmed descendant text content for an element.
pub fn descendant_text(element: &Element) -> String {
    let mut text = String::new();
    collect_descendant_text(element, &mut text);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect_descendant_text(element: &Element, out: &mut String) {
    for child in &element.children {
        match child {
            HtmlNode::Text(Text { content, .. }) => out.push_str(content),
            HtmlNode::Element(child) => collect_descendant_text(child, out),
            HtmlNode::Comment(_) => {}
        }
    }
}

/// Finds the first descendant element whose tag name matches `expected`.
pub fn first_descendant_element<'a>(element: &'a Element, expected: &str) -> Option<&'a Element> {
    if element_is(element, expected) {
        return Some(element);
    }
    for child in &element.children {
        if let HtmlNode::Element(child_element) = child {
            if let Some(found) = first_descendant_element(child_element, expected) {
                return Some(found);
            }
        }
    }
    None
}

/// Selects elements in a document using a CSS selector subset.
pub fn select_css<'a>(document: &'a HtmlDocument, selector: &str) -> Result<Vec<&'a Element>, FormatError> {
    select_css_with_limits(document, selector, "epub", QueryLimits::package_member())
}

/// Selects elements with explicit query limits and an importer label.
///
/// A selector that stops at the query budget is reported as a format error
/// instead of returning a truncated element list.
pub fn select_css_with_limits<'a>(
    document: &'a HtmlDocument,
    selector: &str,
    format: &str,
    limits: QueryLimits,
) -> Result<Vec<&'a Element>, FormatError> {
    let view = HtmlDocumentView::from_document(document);
    let (result, elements) =
        select_css_elements(&view, selector, limits.budget()).map_err(|error| FormatError::parse(format, error.to_string()))?;
    ensure_complete(format, selector, result.outcome, result.matches.len())?;
    Ok(elements)
}

/// Finds the first descendant anchor and returns its label and `href`.
pub fn first_anchor(element: &Element) -> Option<(String, String)> {
    if element_is(element, "a") {
        let href = attribute_value(element, "href")?;
        let label = descendant_text(element);
        if label.is_empty() {
            return None;
        }
        return Some((label, href));
    }
    for child in &element.children {
        if let HtmlNode::Element(child_element) = child {
            if let Some(found) = first_anchor(child_element) {
                return Some(found);
            }
        }
    }
    None
}
