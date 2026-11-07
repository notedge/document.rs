//! Page geometry and default character formatting declared by the DOCX package.
//!
//! Word derives page size, margins, column count, body size and line spacing
//! from `word/document.xml` (`w:sectPr`) and `word/styles.xml`. Neither part
//! existed before this module, so every one of those five facts fell back to
//! the consumer's own defaults.

use std::fmt::Write as _;

use document_ir::DocumentGraph;

/// Points per inch divided by twips per inch: every Word length is in twips.
const TWIPS_PER_POINT: f64 = 20.0;

/// Page size, margins, columns and default character formatting for one
/// section.
///
/// Lengths are in PostScript points because that is the unit the layout
/// pipeline and the PDF text layer already use. The XML writer converts to
/// twips at the boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DocxPageSetup {
    /// Page width in points.
    pub page_width: f64,
    /// Page height in points.
    pub page_height: f64,
    /// Space above the text box in points.
    pub margin_top: f64,
    /// Space right of the text box in points.
    pub margin_right: f64,
    /// Space below the text box in points.
    pub margin_bottom: f64,
    /// Space left of the text box in points.
    pub margin_left: f64,
    /// Column count in the section.
    pub columns: u16,
    /// Gap between columns in points.
    pub column_spacing: f64,
    /// Body font size in points.
    pub base_font_size: f64,
    /// Baseline distance as a multiple of the font size.
    pub line_spacing: f64,
}

impl DocxPageSetup {
    /// A4 portrait with 72 pt margins, one column, 11 pt body and 1.21
    /// leading.
    ///
    /// These are the same defaults `document-layout` uses, so a document
    /// rendered by Word and by the bundled renderer starts from one geometry
    /// instead of two.
    pub const fn a4() -> Self {
        Self {
            page_width: 595.276,
            page_height: 841.89,
            margin_top: 72.0,
            margin_right: 72.0,
            margin_bottom: 72.0,
            margin_left: 72.0,
            columns: 1,
            column_spacing: 36.0,
            base_font_size: 11.0,
            line_spacing: 1.21,
        }
    }

    /// Page setup declared by the graph, with A4 filling every gap.
    ///
    /// A source that names only part of its geometry keeps this exporter
    /// honest: the declared facts are repeated verbatim and the rest falls back
    /// to [`DocxPageSetup::a4`], so a partially declared page never turns into
    /// a fabricated one.
    pub fn from_graph(graph: &DocumentGraph) -> Self {
        let mut setup = Self::a4();
        let Some(page) = graph.page_setup
        else {
            return setup;
        };
        setup.page_width = page.page_width.unwrap_or(setup.page_width);
        setup.page_height = page.page_height.unwrap_or(setup.page_height);
        setup.margin_top = page.margin_top.unwrap_or(setup.margin_top);
        setup.margin_right = page.margin_right.unwrap_or(setup.margin_right);
        setup.margin_bottom = page.margin_bottom.unwrap_or(setup.margin_bottom);
        setup.margin_left = page.margin_left.unwrap_or(setup.margin_left);
        setup.columns = page.columns.unwrap_or(setup.columns).max(1);
        setup.base_font_size = page.base_font_size.unwrap_or(setup.base_font_size);
        setup.line_spacing = page.line_height.unwrap_or(setup.line_spacing);
        setup
    }
}

impl Default for DocxPageSetup {
    fn default() -> Self {
        Self::a4()
    }
}

/// Render `w:sectPr` for the declared page geometry.
pub fn sect_pr_xml(setup: &DocxPageSetup) -> String {
    let mut xml = String::from("<w:sectPr>");
    let _ = write!(xml, "<w:pgSz w:w=\"{}\" w:h=\"{}\"/>", twips(setup.page_width), twips(setup.page_height));
    let _ = write!(
        xml,
        "<w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{}\" w:footer=\"{}\" w:gutter=\"0\"/>",
        twips(setup.margin_top),
        twips(setup.margin_right),
        twips(setup.margin_bottom),
        twips(setup.margin_left),
        twips(36.0),
        twips(36.0),
    );
    let _ = write!(xml, "<w:cols w:num=\"{}\" w:space=\"{}\"/>", setup.columns.max(1), twips(setup.column_spacing));
    xml.push_str("</w:sectPr>");
    xml
}

/// Render `word/styles.xml` for the declared body formatting.
///
/// The body declares the paragraph, heading, quote and inline-code styles the
/// body already references, so no `w:pStyle` value is left dangling. Diagram
/// and chart paragraphs carry their language in the style id (`Diagram:<lang>`
/// / `Chart:<lang>`), so those ids arrive from the graph as `custom_style_ids`
/// and are declared here instead of falling back to Word's silent default.
pub fn styles_xml(setup: &DocxPageSetup, custom_style_ids: &[String]) -> String {
    let half_points = (setup.base_font_size * 2.0).round() as i64;
    let line = (setup.line_spacing * 240.0).round() as i64;
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:docDefaults>
    <w:rPrDefault><w:rPr><w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman" w:eastAsia="SimSun" w:cs="Times New Roman"/><w:sz w:val=""#,
    );
    let _ = write!(xml, "{half_points}\"/><w:szCs w:val=\"{half_points}\"/></w:rPr></w:rPrDefault>");
    let _ = write!(xml, "\n    <w:pPrDefault><w:pPr><w:spacing w:line=\"{line}\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault>");
    xml.push_str("\n  </w:docDefaults>");
    xml.push_str(
        "\n  <w:style w:type=\"paragraph\" w:styleId=\"Normal\" w:default=\"1\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>",
    );
    for level in 1..=6_u8 {
        let scale = heading_scale(level);
        let heading_half_points = (setup.base_font_size * scale * 2.0).round() as i64;
        let _ = write!(
            xml,
            "\n  <w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\"><w:name w:val=\"heading {level}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"120\"/><w:outlineLvl w:val=\"{}\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"{heading_half_points}\"/><w:szCs w:val=\"{heading_half_points}\"/></w:rPr></w:style>",
            level - 1
        );
    }
    xml.push_str(
        "\n  <w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/><w:pPr><w:ind w:left=\"720\" w:right=\"720\"/></w:pPr><w:rPr><w:i/></w:rPr></w:style>",
    );
    xml.push_str(
        "\n  <w:style w:type=\"paragraph\" w:styleId=\"Caption\"><w:name w:val=\"caption\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/><w:pPr><w:spacing w:before=\"120\" w:after=\"120\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr><w:i/><w:sz w:val=\"18\"/><w:szCs w:val=\"18\"/></w:rPr></w:style>",
    );
    xml.push_str(
        "\n  <w:style w:type=\"character\" w:styleId=\"VerbatimChar\"><w:name w:val=\"Verbatim Char\"/><w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\"/><w:sz w:val=\"",
    );
    let _ = write!(xml, "{half_points}\"/></w:rPr></w:style>");
    for style_id in custom_style_ids {
        let escaped = escape_xml_attr(style_id);
        let _ = write!(
            xml,
            "\n  <w:style w:type=\"paragraph\" w:styleId=\"{escaped}\"><w:name w:val=\"{escaped}\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/><w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\"/></w:rPr></w:style>"
        );
    }
    xml.push_str("\n</w:styles>");
    xml
}

fn escape_xml_attr(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Replace the empty section properties the writer emitted with the declared
/// geometry.
///
/// The body writer owns `w:sectPr` placement, so this keeps the document tree
/// in one place and only fills the values in.
pub fn apply_page_setup(document_xml: &str, setup: &DocxPageSetup) -> String {
    document_xml.replacen("<w:sectPr/>", &sect_pr_xml(setup), 1)
}

/// Heading size as a multiple of the body size, shrinking with depth.
fn heading_scale(level: u8) -> f64 {
    match level {
        1 => 2.0,
        2 => 1.6,
        3 => 1.4,
        4 => 1.25,
        5 => 1.12,
        _ => 1.0,
    }
}

pub(super) fn twips(points: f64) -> i64 {
    (points * TWIPS_PER_POINT).round() as i64
}

#[cfg(test)]
mod tests {
    use super::{DocxPageSetup, apply_page_setup, sect_pr_xml, styles_xml};

    #[test]
    fn a4_section_declares_page_size_and_margins() {
        let xml = sect_pr_xml(&DocxPageSetup::a4());
        assert!(xml.contains("<w:pgSz w:w=\"11906\" w:h=\"16838\"/>"), "{xml}");
        assert!(xml.contains("w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\""), "{xml}");
        assert!(xml.contains("<w:cols w:num=\"1\""), "{xml}");
    }

    #[test]
    fn two_columns_are_written_when_declared() {
        let setup = DocxPageSetup { columns: 2, column_spacing: 36.0, ..DocxPageSetup::a4() };
        let xml = sect_pr_xml(&setup);
        assert!(xml.contains("<w:cols w:num=\"2\" w:space=\"720\"/>"), "{xml}");
    }

    #[test]
    fn styles_declare_body_size_and_line_spacing() {
        let xml = styles_xml(&DocxPageSetup::a4(), &[]);
        assert!(xml.contains("<w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/>"), "{xml}");
        assert!(xml.contains("<w:spacing w:line=\"290\" w:lineRule=\"auto\"/>"), "{xml}");
        for level in 1..=6 {
            assert!(xml.contains(&format!("w:styleId=\"Heading{level}\"")), "{xml}");
        }
        assert!(xml.contains("w:styleId=\"VerbatimChar\""), "{xml}");
    }

    #[test]
    fn a_smaller_body_text_shrinks_the_declared_size() {
        let setup = DocxPageSetup { base_font_size: 9.5, line_spacing: 1.03, ..DocxPageSetup::a4() };
        let xml = styles_xml(&setup, &[]);
        assert!(xml.contains("<w:sz w:val=\"19\"/><w:szCs w:val=\"19\"/>"), "{xml}");
        assert!(xml.contains("<w:spacing w:line=\"247\" w:lineRule=\"auto\"/>"), "{xml}");
    }

    #[test]
    fn custom_style_ids_are_declared_verbatim() {
        let xml = styles_xml(&DocxPageSetup::a4(), &["Diagram:mermaid".to_string(), "Chart:vega-lite".to_string()]);
        assert!(xml.contains("w:styleId=\"Diagram:mermaid\""), "{xml}");
        assert!(xml.contains("w:styleId=\"Chart:vega-lite\""), "{xml}");
        assert!(xml.contains("<w:basedOn w:val=\"Normal\"/>"), "{xml}");
    }

    #[test]
    fn page_setup_replaces_the_empty_section_properties() {
        let document = "<w:body><w:p/></w:body><w:sectPr/>";
        let filled = apply_page_setup(document, &DocxPageSetup::a4());
        assert!(!filled.contains("<w:sectPr/>"), "{filled}");
        assert!(filled.contains("<w:pgMar"), "{filled}");
    }
}
