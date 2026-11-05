//! MathML lowering shared by HTML and EPUB import.
//!
//! The element tree comes from `oak-html`. MathML is projected to a LaTeX
//! body and then lowered through the same [`crate::import::tex_math`] path as
//! authored TeX, so the IR keeps one math representation. Constructs without a
//! mapping keep their readable text and are reported by the caller.

use std::borrow::Cow;

use document_ir::{MathDisplay, MathFormula};
use oak_html::ast::{Element, HtmlNode};

use super::math_latex::{delimiter_spelling, escape_text, formula_from_latex, latex_operator};

/// Rewrite self-closing empty MathML elements into void HTML placeholders.
///
/// `oak-html` applies HTML tree construction rules, where a trailing slash on a
/// non-void element is ignored. `<mspace/>` therefore stays open and swallows
/// the rest of the document as its children. The placeholders below are void
/// HTML elements, so the following siblings keep their real nesting. Their
/// spellings are padded with spaces to preserve every byte offset, which keeps
/// the source spans pointing at the original text.
pub(crate) fn normalize_self_closing_math_elements(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let mut replacement: Option<String> = None;
    let mut copied = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        let name_start = index + 1;
        let mut name_end = name_start;
        while name_end < bytes.len() && (bytes[name_end].is_ascii_alphanumeric() || bytes[name_end] == b'-' || bytes[name_end] == b':') {
            name_end += 1;
        }
        let name = text[name_start..name_end].to_ascii_lowercase();
        let placeholder = match name.as_str() {
            "mspace" => "<br/>",
            "none" | "mprescripts" | "malignmark" => "<wbr/>",
            _ => {
                index += 1;
                continue;
            }
        };
        let Some(offset) = text[name_end..].find('>') else {
            break;
        };
        let tag_end = name_end + offset + 1;
        if !text[name_end..tag_end - 1].trim_end().ends_with('/') {
            index += 1;
            continue;
        }
        let owned = replacement.get_or_insert_with(|| String::with_capacity(text.len()));
        owned.push_str(&text[copied..index]);
        let length = tag_end - index;
        owned.push_str(placeholder);
        owned.push_str(&" ".repeat(length.saturating_sub(placeholder.len())));
        copied = tag_end;
        index = tag_end;
    }
    match replacement {
        Some(mut owned) => {
            owned.push_str(&text[copied..]);
            Cow::Owned(owned)
        }
        None => Cow::Borrowed(text),
    }
}

/// Rewrite self-closing elements into void HTML placeholders before parsing.
///
/// `oak-html` applies HTML tree construction rules, where a trailing slash on a
/// non-void element is ignored. MathML empty elements and SVG children both
/// rely on the slash to stay empty, so without this pass they remain open and
/// swallow every following sibling. Placeholders keep the byte length, which
/// keeps source spans pointing at the original text.
pub(crate) fn normalize_self_closing_elements(text: &str) -> Cow<'_, str> {
    let math = normalize_self_closing_math_elements(text);
    let svg = normalize_self_closing_svg_elements(math.as_ref());
    if let Cow::Owned(owned) = svg {
        return Cow::Owned(owned);
    }
    match math {
        Cow::Owned(owned) => Cow::Owned(owned),
        Cow::Borrowed(_) => Cow::Borrowed(text),
    }
}

/// Rewrite self-closing elements into explicit end tags before parsing.
///
/// The XHTML reader keeps the element tree, so it cannot use the placeholder
/// pass: expanding `<rect .../>` to `<rect ...></rect>` keeps the attributes
/// and children visible while `</svg>` closes the subtree again. Offsets shift,
/// which is fine because the EPUB reader only walk the tree and restores
/// whitespace against the same expanded text.
pub(crate) fn normalize_self_closing_elements_expanded(text: &str) -> Cow<'_, str> {
    let math = normalize_self_closing_math_elements(text);
    let svg = expand_self_closing_svg_elements(math.as_ref());
    if let Cow::Owned(owned) = svg {
        return Cow::Owned(owned);
    }
    match math {
        Cow::Owned(owned) => Cow::Owned(owned),
        Cow::Borrowed(_) => Cow::Borrowed(text),
    }
}

/// Rewrite self-closing SVG elements into void HTML placeholders.
///
/// An authored `<svg><rect .../></svg>` is valid HTML: inside foreign content
/// the trailing slash closes the element. `oak-html` ignores it, so `<rect>`
/// stays open, `</svg>` no longer closes the subtree, and the rest of the
/// document is swallowed into the raw SVG markup. A self-closing `<svg/>`
/// swallows the same way. Both shapes are replaced by a void `<br>`, padded to
/// the authored byte length so every span keeps pointing at the original text.
pub(crate) fn normalize_self_closing_svg_elements(text: &str) -> Cow<'_, str> {
    let mut replacements = Vec::new();
    collect_svg_replacements(text, &mut replacements);
    if replacements.is_empty() {
        return Cow::Borrowed(text);
    }
    replacements.sort_unstable();
    replacements.dedup();
    let mut owned = String::with_capacity(text.len());
    let mut copied = 0usize;
    for (start, end) in replacements {
        owned.push_str(&text[copied..start]);
        owned.push_str("<br>");
        owned.push_str(&" ".repeat((end - start).saturating_sub("<br>".len())));
        copied = end;
    }
    owned.push_str(&text[copied..]);
    Cow::Owned(owned)
}

fn collect_svg_replacements(text: &str, out: &mut Vec<(usize, usize)>) {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let Some(offset) = text[index..].find('<') else {
            break;
        };
        let start = index + offset;
        if text[start..].starts_with("<!--") {
            match text[start..].find("-->") {
                Some(end) => {
                    index = start + end + 3;
                    continue;
                }
                None => break,
            }
        }
        let Some((name, name_end)) = tag_name(text, start) else {
            index = start + 1;
            continue;
        };
        let Some(tag_end) = find_tag_end(text, name_end) else {
            break;
        };
        if name.eq_ignore_ascii_case("svg") {
            if is_self_closing_start_tag(text, name_end, tag_end) {
                out.push((start, tag_end + 1));
            }
            else if let Some(region_end) = find_svg_close_end(text, tag_end + 1) {
                collect_self_closing_tags(text, tag_end + 1, region_end, out);
                index = region_end;
                continue;
            }
        }
        index = tag_end + 1;
    }
}

fn expand_self_closing_svg_elements(text: &str) -> Cow<'_, str> {
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    collect_svg_expansions(text, &mut replacements);
    if replacements.is_empty() {
        return Cow::Borrowed(text);
    }
    replacements.sort_by(|left, right| left.0.cmp(&right.0));
    replacements.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    let mut owned = String::with_capacity(text.len() + replacements.len() * 8);
    let mut copied = 0usize;
    for (start, end, replacement) in replacements {
        if start < copied {
            continue;
        }
        owned.push_str(&text[copied..start]);
        owned.push_str(&replacement);
        copied = end;
    }
    owned.push_str(&text[copied..]);
    Cow::Owned(owned)
}

fn collect_svg_expansions(text: &str, out: &mut Vec<(usize, usize, String)>) {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let Some(offset) = text[index..].find('<') else {
            break;
        };
        let start = index + offset;
        if text[start..].starts_with("<!--") {
            match text[start..].find("-->") {
                Some(end) => {
                    index = start + end + 3;
                    continue;
                }
                None => break,
            }
        }
        let Some((name, name_end)) = tag_name(text, start) else {
            index = start + 1;
            continue;
        };
        let Some(tag_end) = find_tag_end(text, name_end) else {
            break;
        };
        if name.eq_ignore_ascii_case("svg") {
            if is_self_closing_start_tag(text, name_end, tag_end) {
                out.push((start, tag_end + 1, expanded_tag(text, start, name_end, tag_end)));
            }
            else if let Some(region_end) = find_svg_close_end(text, tag_end + 1) {
                collect_inner_expansions(text, tag_end + 1, region_end, out);
                index = region_end;
                continue;
            }
        }
        index = tag_end + 1;
    }
}

fn collect_inner_expansions(text: &str, from: usize, to: usize, out: &mut Vec<(usize, usize, String)>) {
    let mut index = from;
    while index < to {
        let Some(offset) = text[index..to].find('<') else {
            break;
        };
        let start = index + offset;
        if text[start..].starts_with("<!--") {
            match text[start..to].find("-->") {
                Some(end) => {
                    index = start + end + 3;
                    continue;
                }
                None => break,
            }
        }
        let Some((name, name_end)) = tag_name(text, start) else {
            index = start + 1;
            continue;
        };
        let Some(tag_end) = find_tag_end(text, name_end) else {
            break;
        };
        if tag_end >= to {
            break;
        }
        if is_self_closing_start_tag(text, name_end, tag_end) && !is_void_html_element(&name) {
            out.push((start, tag_end + 1, expanded_tag(text, start, name_end, tag_end)));
        }
        index = tag_end + 1;
    }
}

/// `<rect .../>` becomes `<rect ...></rect>`, keeping attributes and casing.
fn expanded_tag(text: &str, start: usize, name_end: usize, tag_end: usize) -> String {
    let name = &text[start + 1..name_end];
    let mut slash = tag_end;
    while slash > name_end && text.as_bytes()[slash - 1].is_ascii_whitespace() {
        slash -= 1;
    }
    format!("{}{}</{}>", &text[start..slash - 1], &text[slash..tag_end + 1], name)
}

fn is_void_html_element(name: &str) -> bool {
    matches!(
        name,
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "param" | "source" | "track" | "wbr"
    )
}

fn collect_self_closing_tags(text: &str, from: usize, to: usize, out: &mut Vec<(usize, usize)>) {
    let mut index = from;
    while index < to {
        let Some(offset) = text[index..to].find('<') else {
            break;
        };
        let start = index + offset;
        if text[start..].starts_with("<!--") {
            match text[start..to].find("-->") {
                Some(end) => {
                    index = start + end + 3;
                    continue;
                }
                None => break,
            }
        }
        let Some((_, name_end)) = tag_name(text, start) else {
            index = start + 1;
            continue;
        };
        let Some(tag_end) = find_tag_end(text, name_end) else {
            break;
        };
        if tag_end >= to {
            break;
        }
        if is_self_closing_start_tag(text, name_end, tag_end) {
            out.push((start, tag_end + 1));
        }
        index = tag_end + 1;
    }
}

/// The lowercase tag name and the offset just past it.
fn tag_name(text: &str, start: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    let mut index = start + 1;
    if bytes.get(index) == Some(&b'/') {
        return None;
    }
    let name_start = index;
    while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'-' || bytes[index] == b':') {
        index += 1;
    }
    if index == name_start {
        return None;
    }
    Some((text[name_start..index].to_ascii_lowercase(), index))
}

fn is_self_closing_start_tag(text: &str, name_end: usize, tag_end: usize) -> bool {
    text[name_end..tag_end].trim_end().ends_with('/')
}

/// The offset of the `>` that ends the start tag, ignoring quoted attributes.
pub(crate) fn find_tag_end(text: &str, from: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (offset, byte) in text[from..].bytes().enumerate() {
        match byte {
            b'"' | b'\'' => {
                if quote == Some(byte) {
                    quote = None;
                }
                else if quote.is_none() {
                    quote = Some(byte);
                }
            }
            b'>' if quote.is_none() => return Some(from + offset),
            _ => {}
        }
    }
    None
}

/// The offset just past the `</svg>` that closes the region opening at `from`.
fn find_svg_close_end(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut index = from;
    while index < bytes.len() {
        let Some(offset) = text[index..].find('<') else {
            return None;
        };
        let start = index + offset;
        if text[start..].starts_with("<!--") {
            match text[start..].find("-->") {
                Some(end) => {
                    index = start + end + 3;
                    continue;
                }
                None => return None,
            }
        }
        if bytes.get(start + 1) == Some(&b'/') {
            let name_start = start + 2;
            let mut name_end = name_start;
            while name_end < bytes.len()
                && (bytes[name_end].is_ascii_alphanumeric() || bytes[name_end] == b'-' || bytes[name_end] == b':')
            {
                name_end += 1;
            }
            if text[name_start..name_end].eq_ignore_ascii_case("svg") {
                let tag_end = find_tag_end(text, name_end)?;
                if depth == 0 {
                    return Some(tag_end + 1);
                }
                depth -= 1;
                index = tag_end + 1;
                continue;
            }
        }
        else if let Some((name, name_end)) = tag_name(text, start) {
            let tag_end = find_tag_end(text, name_end)?;
            if name.eq_ignore_ascii_case("svg") && !is_self_closing_start_tag(text, name_end, tag_end) {
                depth += 1;
            }
            index = tag_end + 1;
            continue;
        }
        index = start + 1;
    }
    None
}

/// Lower one `<math>` element into the document math representation.
///
/// Returns the formula plus the MathML constructs that had no mapping. Callers
/// record the latter as import losses, so a downgraded formula never looks
/// lossless.
pub(crate) fn lower_math_element(element: &Element, display: MathDisplay) -> (MathFormula, Vec<String>) {
    let mut builder = Builder::default();
    builder.lower_children(element);
    let body = builder.out.trim().to_string();
    let fallback = element_text(element);
    let body = if body.is_empty() {
        if fallback.trim().is_empty() { String::new() } else { format!("\\text{{{}}}", escape_text(fallback.trim())) }
    }
    else {
        body
    };
    let formula = formula_from_latex(&body, display);
    (formula, builder.unsupported)
}

/// The authored `display` intent of a MathML element.
pub(crate) fn math_display(element: &Element) -> MathDisplay {
    match attribute_value(element, "display").as_deref() {
        Some("block") => MathDisplay::Display,
        _ => MathDisplay::Inline,
    }
}

/// Whether an element is MathML that asks for display placement.
pub(crate) fn is_display_math(element: &Element) -> bool {
    tag_of(element) == "math" && math_display(element) == MathDisplay::Display
}

/// Returns the only child of a paragraph when it is display MathML.
pub(crate) fn single_display_math_child(element: &Element) -> Option<&Element> {
    let mut found = None;
    for child in &element.children {
        match child {
            HtmlNode::Element(child) if is_display_math(child) => {
                if found.is_some() {
                    return None;
                }
                found = Some(child);
            }
            HtmlNode::Element(_) => return None,
            HtmlNode::Text(text) if text.content.trim().is_empty() => {}
            HtmlNode::Comment(_) => {}
            HtmlNode::Text(_) => return None,
        }
    }
    found
}

#[derive(Debug, Default)]
struct Builder {
    out: String,
    unsupported: Vec<String>,
}

impl Builder {
    fn note_unsupported(&mut self, kind: impl Into<String>) {
        let kind = kind.into();
        if !self.unsupported.contains(&kind) {
            self.unsupported.push(kind);
        }
    }

    fn lower_children(&mut self, element: &Element) {
        for child in &element.children {
            if let HtmlNode::Element(child) = child {
                self.lower(child);
            }
        }
    }

    fn lower(&mut self, element: &Element) {
        let tag = tag_of(element);
        match tag.as_str() {
            "math" | "mrow" | "mstyle" | "mphantom" | "menclose" | "mbox" | "mstack" | "msgroup" | "mfrac"
            | "msup" | "msub" | "msubsup" | "msqrt" | "mroot" | "mfenced" | "mtable" | "mtr" | "mtd" | "mover"
            | "munder" | "munderover" | "mspace" | "mtext" | "mi" | "mn" | "mo" | "semantics" | "none" | "mpadded"
            | "merror" | "br" | "wbr" => self.lower_supported(tag.as_str(), element),
            other => self.lower_unknown(other, element),
        }
    }

    fn lower_supported(&mut self, tag: &str, element: &Element) {
        match tag {
            "math" | "mrow" | "mstyle" | "mphantom" | "menclose" | "mbox" | "mstack" | "msgroup" | "mpadded"
            | "merror" | "mtr" | "mtd" => self.lower_children(element),
            "semantics" => {
                // The first child is the presentation tree. `annotation` and
                // `annotation-xml` carry the authored source instead, so they
                // never become visible atoms.
                if let Some(first) = element.children.iter().filter_map(as_element).next() {
                    self.lower(first);
                }
            }
            "none" => {}
            "mi" => {
                let text = element_text(element).trim().to_string();
                if text.chars().count() == 1 {
                    self.out.push_str(&latex_operator(&text));
                }
                else if text.is_empty() {
                    self.note_unsupported("mi.empty");
                }
                else {
                    self.out.push_str(&format!("\\operatorname{{{}}}", escape_text(&text)));
                }
            }
            "mn" => self.out.push_str(&latex_operator(element_text(element).trim())),
            "mo" => self.out.push_str(&latex_operator(element_text(element).trim())),
            "mtext" => {
                self.out.push_str(&format!("\\text{{{}}}", escape_text(element_text(element).trim())));
            }
            "mspace" => self.out.push_str("\\quad "),
            // Placeholders produced by `normalize_self_closing_math_elements`.
            "br" => self.out.push_str("\\quad "),
            "wbr" => {},
            "mfrac" => {
                let (numerator, denominator) = first_two_children(element);
                self.out.push_str("\\frac{");
                self.lower_optional(numerator);
                self.out.push_str("}{");
                self.lower_optional(denominator);
                self.out.push('}');
            }
            "msup" => {
                let (base, superscript) = first_two_children(element);
                self.out.push('{');
                self.lower_optional(base);
                self.out.push_str("}^{");
                self.lower_optional(superscript);
                self.out.push('}');
            }
            "msub" => {
                let (base, subscript) = first_two_children(element);
                self.out.push('{');
                self.lower_optional(base);
                self.out.push_str("}_{");
                self.lower_optional(subscript);
                self.out.push('}');
            }
            "msubsup" => {
                let (base, subscript, superscript) = first_three_children(element);
                self.out.push('{');
                self.lower_optional(base);
                self.out.push_str("}_{");
                self.lower_optional(subscript);
                self.out.push_str("}^{");
                self.lower_optional(superscript);
                self.out.push('}');
            }
            "msqrt" => {
                self.out.push_str("\\sqrt{");
                self.lower_children(element);
                self.out.push('}');
            }
            "mroot" => {
                let (radicand, index) = first_two_children(element);
                self.out.push_str("\\sqrt[");
                self.lower_optional(index);
                self.out.push_str("]{");
                self.lower_optional(radicand);
                self.out.push('}');
            }
            "mfenced" => self.lower_fenced(element),
            "mtable" => self.lower_table(element),
            "mover" => {
                let (base, over) = first_two_children(element);
                self.out.push_str("\\overset{");
                self.lower_optional(over);
                self.out.push_str("}{");
                self.lower_optional(base);
                self.out.push('}');
            }
            "munder" => {
                let (base, under) = first_two_children(element);
                self.out.push_str("\\underset{");
                self.lower_optional(under);
                self.out.push_str("}{");
                self.lower_optional(base);
                self.out.push('}');
            }
            "munderover" => {
                let (base, under, over) = first_three_children(element);
                self.out.push_str("\\underset{");
                self.lower_optional(under);
                self.out.push_str("}{\\overset{");
                self.lower_optional(over);
                self.out.push_str("}{");
                self.lower_optional(base);
                self.out.push_str("}}");
            }
            _ => self.lower_children(element),
        }
    }

    fn lower_fenced(&mut self, element: &Element) {
        let open = attribute_value(element, "open").unwrap_or_else(|| "(".into());
        let close = attribute_value(element, "close").unwrap_or_else(|| ")".into());
        let separator = attribute_value(element, "separators").unwrap_or_else(|| ",".into());
        let mut separator = separator.chars().next().map(|character| character.to_string()).unwrap_or_default();
        if separator.is_empty() {
            separator = ",".into();
        }
        self.out.push_str("\\left");
        self.out.push_str(&delimiter_spelling(&open));
        self.out.push(' ');
        let mut first = true;
        for child in element.children.iter().filter_map(as_element) {
            if !first {
                self.out.push_str(&latex_operator(&separator));
                self.out.push(' ');
            }
            first = false;
            self.lower(child);
        }
        self.out.push_str(" \\right");
        self.out.push_str(&delimiter_spelling(&close));
    }

    fn lower_table(&mut self, element: &Element) {
        let rows: Vec<&Element> = element.children.iter().filter_map(as_element).filter(|child| tag_of(child) == "mtr").collect();
        if rows.is_empty() {
            self.note_unsupported("mtable.empty");
            self.lower_children(element);
            return;
        }
        self.out.push_str("\\begin{matrix}");
        for (row_index, row) in rows.iter().enumerate() {
            if row_index > 0 {
                self.out.push_str(" \\\\ ");
            }
            let cells: Vec<&Element> =
                row.children.iter().filter_map(as_element).filter(|child| tag_of(child) == "mtd").collect();
            if cells.is_empty() {
                self.lower_children(row);
            }
            else {
                for (cell_index, cell) in cells.iter().enumerate() {
                    if cell_index > 0 {
                        self.out.push_str(" & ");
                    }
                    self.lower_children(cell);
                }
            }
        }
        self.out.push_str("\\end{matrix}");
    }

    fn lower_unknown(&mut self, tag: &str, element: &Element) {
        self.note_unsupported(format!("mathml.{tag}"));
        let text = element_text(element);
        if !text.trim().is_empty() {
            self.out.push_str(&format!("\\text{{{}}}", escape_text(text.trim())));
        }
        else {
            self.lower_children(element);
        }
    }

    fn lower_optional(&mut self, element: Option<&Element>) {
        if let Some(element) = element {
            self.lower(element);
        }
    }
}

fn as_element(node: &HtmlNode) -> Option<&Element> {
    match node {
        HtmlNode::Element(element) => Some(element),
        _ => None,
    }
}

fn tag_of(element: &Element) -> String {
    element.tag_name.rsplit_once(':').map(|(_, local)| local).unwrap_or(&element.tag_name).to_ascii_lowercase()
}

fn attribute_value(element: &Element, name: &str) -> Option<String> {
    element.attributes.iter().find(|attribute| attribute.name.eq_ignore_ascii_case(name)).and_then(|attribute| attribute.value.clone())
}

fn first_two_children(element: &Element) -> (Option<&Element>, Option<&Element>) {
    let mut children = element.children.iter().filter_map(as_element);
    (children.next(), children.next())
}

fn first_three_children(element: &Element) -> (Option<&Element>, Option<&Element>, Option<&Element>) {
    let mut children = element.children.iter().filter_map(as_element);
    (children.next(), children.next(), children.next())
}

fn element_text(element: &Element) -> String {
    let mut text = String::new();
    collect_text(element, &mut text);
    text
}

fn collect_text(element: &Element, out: &mut String) {
    for child in &element.children {
        match child {
            HtmlNode::Text(node) => out.push_str(&node.content),
            HtmlNode::Element(child) => collect_text(child, out),
            HtmlNode::Comment(_) => {}
        }
    }
}
