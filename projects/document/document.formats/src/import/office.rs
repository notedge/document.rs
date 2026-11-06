//! Shared OPC and XML traversal helpers for the XLSX and PPTX importers.
//!
//! The helpers stay byte-slice based because `oak-xml` keeps only text and
//! identifier leaves. Leaf text is restored from the raw source so escaped
//! characters and shared-string payloads survive import.

use oak_xml::ast::{XmlElement, XmlValue};

use crate::FormatError;

/// Parses UTF-8 XML through `oak-xml` and restores leaf text.
pub(crate) fn parse_xml(xml: &[u8], format: &str) -> Result<XmlValue, FormatError> {
    let text = std::str::from_utf8(xml).map_err(|error| FormatError::parse(format, format!("xml is not utf-8: {error}")))?;
    let mut value = oak_xml::parse(text).map_err(|error| FormatError::parse(format, error.to_string()))?;
    restore_leaf_text(&mut value, text);
    Ok(value)
}

fn restore_leaf_text(value: &mut XmlValue, source: &str) {
    match value {
        XmlValue::Element(element) => restore_element_leaf_text(element, source),
        XmlValue::Fragment(values) => {
            for child in values {
                restore_leaf_text(child, source);
            }
        }
        _ => {}
    }
}

fn restore_element_leaf_text(element: &mut XmlElement, source: &str) {
    let mut has_child_elements = false;
    for child in &mut element.children {
        match child {
            XmlValue::Element(_) => has_child_elements = true,
            XmlValue::Fragment(values) => {
                if values.iter().any(|value| matches!(value, XmlValue::Element(_))) {
                    has_child_elements = true;
                }
            }
            _ => {}
        }
        restore_leaf_text(child, source);
    }
    if has_child_elements {
        return;
    }
    let Some((start, end)) = element_content_range(element, source)
    else {
        return;
    };
    let decoded = decode_xml_entities(&source[start..end]);
    element.children.clear();
    if !decoded.is_empty() {
        element.children.push(XmlValue::Text(decoded));
    }
}

fn element_content_range(element: &XmlElement, source: &str) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let start = element.span.start;
    let end = element.span.end.min(source.len());
    if start >= end || bytes.get(start) != Some(&b'<') {
        return None;
    }
    let mut index = start;
    let mut quote = None;
    let content_start = loop {
        let byte = *bytes.get(index)?;
        match quote {
            Some(open) => {
                if byte == open {
                    quote = None;
                }
            }
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'>' => {
                if index > start && bytes[index - 1] == b'/' {
                    return None;
                }
                break index + 1;
            }
            None => {}
        }
        index += 1;
    };
    let tail = source.get(content_start..end)?;
    let tag_start = tail.rfind("</")? + content_start;
    Some((content_start, tag_start))
}

fn decode_xml_entities(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(position) = rest.find('&') {
        output.push_str(&rest[..position]);
        let candidate = &rest[position..];
        let Some(semicolon) = candidate.find(';')
        else {
            output.push_str(candidate);
            return output;
        };
        let entity = &candidate[1..semicolon];
        match entity {
            "amp" => output.push('&'),
            "lt" => output.push('<'),
            "gt" => output.push('>'),
            "quot" => output.push('"'),
            "apos" => output.push('\''),
            _ => match numeric_character_reference(entity) {
                Some(character) => output.push(character),
                None => output.push_str(&candidate[..=semicolon]),
            },
        }
        rest = &candidate[semicolon + 1..];
    }
    output.push_str(rest);
    output
}

fn numeric_character_reference(entity: &str) -> Option<char> {
    let digits = entity.strip_prefix('#')?;
    let code = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => digits.parse().ok()?,
    };
    char::from_u32(code)
}

/// Returns the local name after an optional namespace prefix.
pub(crate) fn local_name(name: &str) -> &str {
    name.rsplit_once(':').map(|(_, local)| local).unwrap_or(name)
}

/// Returns whether an element's local tag name matches `expected`.
pub(crate) fn element_is(element: &XmlElement, expected: &str) -> bool {
    local_name(&element.name) == expected
}

/// Returns the first element of `value`, skipping bare text nodes.
pub(crate) fn root_element(value: &XmlValue) -> Option<&XmlElement> {
    match value {
        XmlValue::Element(element) => Some(element),
        XmlValue::Fragment(values) => values.iter().find_map(root_element),
        _ => None,
    }
}

/// Returns the document root element or a parse error.
pub(crate) fn document_root<'a>(value: &'a XmlValue, format: &str) -> Result<&'a XmlElement, FormatError> {
    root_element(value).ok_or_else(|| FormatError::parse(format, "xml has no root element"))
}

/// Returns direct child elements that match the local name.
pub(crate) fn children_named<'a>(element: &'a XmlElement, name: &str) -> Vec<&'a XmlElement> {
    let mut elements = Vec::new();
    for child in &element.children {
        match child {
            XmlValue::Element(child) if element_is(child, name) => elements.push(child),
            XmlValue::Fragment(values) => {
                for value in values {
                    if let XmlValue::Element(child) = value {
                        if element_is(child, name) {
                            elements.push(child);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    elements
}

/// Returns the first direct child element with the local name.
pub(crate) fn child_named<'a>(element: &'a XmlElement, name: &str) -> Option<&'a XmlElement> {
    children_named(element, name).into_iter().next()
}

/// Collects descendant elements with the local name in document order.
pub(crate) fn descendants_named<'a>(element: &'a XmlElement, name: &str) -> Vec<&'a XmlElement> {
    let mut elements = Vec::new();
    collect_descendants(element, name, &mut elements);
    elements
}

fn collect_descendants<'a>(element: &'a XmlElement, name: &str, output: &mut Vec<&'a XmlElement>) {
    for child in &element.children {
        if let XmlValue::Element(child) = child {
            if element_is(child, name) {
                output.push(child);
            }
            collect_descendants(child, name, output);
        }
    }
}

/// Reads an attribute by local name and resolves XML entities in its value.
pub(crate) fn attribute_value(element: &XmlElement, expected: &str) -> Option<String> {
    element.attributes.iter().find(|attr| local_name(&attr.name) == expected).map(|attr| decode_xml_entities(&attr.value))
}

/// Parses a `u32` attribute by local name.
pub(crate) fn u32_attribute(element: &XmlElement, expected: &str) -> Option<u32> {
    attribute_value(element, expected)?.trim().parse().ok()
}

/// Parses a `f64` attribute by local name.
pub(crate) fn f64_attribute(element: &XmlElement, expected: &str) -> Option<f64> {
    attribute_value(element, expected)?.trim().parse().ok()
}

/// Reads a boolean attribute using the Office `1` / `true` convention.
pub(crate) fn bool_attribute(element: &XmlElement, expected: &str) -> Option<bool> {
    attribute_value(element, expected).map(|value| matches!(value.trim(), "1" | "true" | "on" | "yes"))
}

/// Concatenates direct and nested text leaves of an element.
pub(crate) fn element_text(element: &XmlElement) -> String {
    let mut text = String::new();
    collect_text(element, &mut text);
    text
}

fn collect_text(element: &XmlElement, output: &mut String) {
    for child in &element.children {
        match child {
            XmlValue::Text(value) | XmlValue::CData(value) => output.push_str(value),
            XmlValue::Element(child) => collect_text(child, output),
            XmlValue::Fragment(values) => {
                for value in values {
                    match value {
                        XmlValue::Text(text) | XmlValue::CData(text) => output.push_str(text),
                        XmlValue::Element(child) => collect_text(child, output),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

/// Collects text leaves while skipping every element in `skip`.
pub(crate) fn element_text_without(element: &XmlElement, skip: &[&str]) -> String {
    let mut text = String::new();
    collect_text_without(element, skip, &mut text);
    text
}

fn collect_text_without(element: &XmlElement, skip: &[&str], output: &mut String) {
    for child in &element.children {
        match child {
            XmlValue::Text(value) | XmlValue::CData(value) => output.push_str(value),
            XmlValue::Element(child) => {
                if !skip.iter().any(|name| local_name(&child.name) == *name) {
                    collect_text_without(child, skip, output);
                }
            }
            XmlValue::Fragment(values) => {
                for value in values {
                    match value {
                        XmlValue::Text(text) | XmlValue::CData(text) => output.push_str(text),
                        XmlValue::Element(child) => {
                            if !skip.iter().any(|name| local_name(&child.name) == *name) {
                                collect_text_without(child, skip, output);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

/// Escapes text for HTML and SVG output.
pub(crate) fn escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(character),
        }
    }
    output
}
