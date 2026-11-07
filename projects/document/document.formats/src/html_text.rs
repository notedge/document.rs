//! Visible-text projection for authored raw HTML.

/// Visible text and element names of an authored raw HTML fragment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HtmlTextProjection {
    /// Visible characters in document order, with one line per authored block
    /// boundary.
    pub text: String,
    /// Lowercased element names in source order, duplicates collapsed.
    pub elements: Vec<String>,
}

impl HtmlTextProjection {
    /// Names the dropped element structure, e.g. `` `<div>`, `<b>` ``.
    ///
    /// A fragment without element markup still lost its raw-HTML form, so the
    /// description falls back to `raw HTML markup` instead of an empty list.
    #[must_use]
    pub fn element_description(&self) -> String {
        if self.elements.is_empty() {
            return "raw HTML markup".to_string();
        }
        self.elements.iter().map(|name| format!("`<{name}>`")).collect::<Vec<_>>().join(", ")
    }
}

/// Project authored raw HTML onto the characters a page or paragraph can carry.
///
/// A target without an arbitrary HTML slot still has to keep the authored
/// characters visible. Element markup is dropped, block boundaries become line
/// breaks, runs of whitespace collapse and character references decode to the
/// characters they stand for. The projection also names the elements it dropped
/// so the caller reports the missing structure instead of calling the export
/// clean.
#[must_use]
pub fn project(html: &str) -> HtmlTextProjection {
    let mut projection = HtmlTextProjection::default();
    let mut rest = html;
    let mut buffered = String::new();
    while !rest.is_empty() {
        let Some(start) = rest.find('<')
        else {
            buffered.push_str(rest);
            break;
        };
        buffered.push_str(&rest[..start]);
        let tag_source = &rest[start..];
        if let Some(after_open) = tag_source.strip_prefix("<!--") {
            match after_open.find("-->") {
                Some(end) => rest = &after_open[end + 3..],
                None => rest = "",
            }
            continue;
        }
        let Some(end) = tag_source.find('>')
        else {
            buffered.push_str(tag_source);
            break;
        };
        let mut inner = tag_source[1..end].trim();
        rest = &tag_source[end + 1..];
        if inner.starts_with('!') {
            continue;
        }
        let closing = inner.starts_with('/');
        inner = inner.trim_start_matches('/');
        let name: String = inner
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | ':' | '_'))
            .collect::<String>()
            .to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        if !closing && matches!(name.as_str(), "script" | "style") {
            let closing_tag = format!("</{name}");
            match rest.to_ascii_lowercase().find(&closing_tag) {
                Some(offset) => match rest[offset..].find('>') {
                    Some(end) => rest = &rest[offset + end + 1..],
                    None => rest = "",
                },
                None => rest = "",
            }
            continue;
        }
        if !closing && !projection.elements.iter().any(|seen| seen == &name) {
            projection.elements.push(name.clone());
        }
        if is_block_element(&name) {
            flush_text(&mut buffered, &mut projection.text);
            projection.text.push('\n');
        }
    }
    flush_text(&mut buffered, &mut projection.text);
    projection.text = normalize_lines(&projection.text);
    projection
}

const BLOCK_ELEMENTS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "br",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];

fn is_block_element(name: &str) -> bool {
    BLOCK_ELEMENTS.contains(&name)
}

fn flush_text(buffered: &mut String, out: &mut String) {
    if buffered.is_empty() {
        return;
    }
    out.push_str(&decode_entities(buffered));
    buffered.clear();
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let entity = &rest[amp..];
        let Some(semi) = entity.find(';')
        else {
            out.push_str(entity);
            return out;
        };
        let decoded = match &entity[1..semi] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            number => number.strip_prefix('#').and_then(numeric_reference),
        };
        match decoded {
            Some(ch) => {
                out.push(ch);
                rest = &entity[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &entity[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn numeric_reference(number: &str) -> Option<char> {
    let value = match number.strip_prefix('x').or_else(|| number.strip_prefix('X')) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => number.parse::<u32>().ok()?,
    };
    char::from_u32(value)
}

fn normalize_lines(text: &str) -> String {
    text.split('\n').map(collapse_whitespace).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n")
}

fn collapse_whitespace(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_space = false;
    for ch in line.chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        }
        else {
            out.push(ch);
            in_space = false;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::project;

    #[test]
    fn visible_text_keeps_order_across_inline_markup() {
        let projection = project("<div>可见文字 <b>加粗</b></div>");
        assert_eq!(projection.text, "可见文字 加粗");
        assert_eq!(projection.element_description(), "`<div>`, `<b>`");
    }

    #[test]
    fn block_boundaries_and_br_keep_lines_while_comments_and_scripts_stay_out() {
        let projection = project("<div>one<!-- skip --><br/>two</div><p>three</p><script>gone()</script>");
        assert_eq!(projection.text, "one\ntwo\nthree");
        assert!(!projection.text.contains("gone"));
    }

    #[test]
    fn character_references_decode_to_visible_characters() {
        assert_eq!(project("<div>a &amp; b &lt; c &#65;</div>").text, "a & b < c A");
    }
}
