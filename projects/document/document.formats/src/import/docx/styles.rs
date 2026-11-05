//! Default character and paragraph formatting declared by `word/styles.xml`.
//!
//! Word writes the body size and the default line spacing once, in
//! `w:docDefaults`, and every unstyled paragraph inherits them. The importer
//! reads those two facts so the page setup it records matches what Word shows
//! instead of the importer's own defaults.
//!
//! | Word unit | Meaning | Points |
//! |-----------|---------|--------|
//! | `w:sz` | half-points | `val / 2` |
//! | `w:line` with `auto` | 240ths of a single line | `val / 240` |
//! | `w:line` with `exact` or `atLeast` | twips | `val / 20` |

use super::xml_extensions::{attribute_value, child_element, document_root, elements_by_local_name, parse_xml_bytes};

/// Half-points per point in `w:sz`.
const HALF_POINTS_PER_POINT: f64 = 2.0;

/// Twentieths of a point per twip.
const TWIPS_PER_POINT: f64 = 20.0;

/// `w:line` units for one single-spaced line.
const LINE_UNITS_PER_LINE: f64 = 240.0;

/// Body facts inherited from `w:docDefaults`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StyleDefaults {
    /// Body text size in points from `rPrDefault/rPr/sz`.
    pub base_font_size: Option<f64>,
    /// Line box height as a multiple of the body size from
    /// `pPrDefault/pPr/spacing`.
    pub line_height: Option<f64>,
}

/// Read the document defaults, treating an unreadable part as absent.
///
/// `word/styles.xml` is optional and no semantic content depends on it, so a
/// parse failure leaves both fields `None` rather than failing the import.
pub fn parse_styles_xml_lossy(xml: &[u8]) -> StyleDefaults {
    let Ok(value) = parse_xml_bytes(xml)
    else {
        return StyleDefaults::default();
    };
    let Ok(root) = document_root(&value)
    else {
        return StyleDefaults::default();
    };
    let Some(defaults) = elements_by_local_name(root, "docDefaults").first().copied()
    else {
        return StyleDefaults::default();
    };
    let base_font_size = child_element(defaults, "rPrDefault")
        .and_then(|defaults| child_element(defaults, "rPr"))
        .and_then(|run_properties| child_element(run_properties, "sz"))
        .and_then(|size| attribute_value(size, "val"))
        .and_then(|value| value.parse::<f64>().ok())
        .map(|half_points| half_points / HALF_POINTS_PER_POINT)
        .filter(|size| *size > 0.0);
    let spacing = child_element(defaults, "pPrDefault")
        .and_then(|defaults| child_element(defaults, "pPr"))
        .and_then(|paragraph_properties| child_element(paragraph_properties, "spacing"));
    let line_height = spacing.and_then(|spacing| {
        let rule = line_rule(spacing);
        attribute_value(spacing, "line").and_then(|value| value.parse::<f64>().ok()).and_then(|line| match rule {
            LineRule::Multiple => (line > 0.0).then_some(line / LINE_UNITS_PER_LINE),
            LineRule::Length => {
                base_font_size.filter(|size| *size > 0.0).filter(|_| line > 0.0).map(|size| (line / TWIPS_PER_POINT) / size)
            }
        })
    });
    StyleDefaults { base_font_size, line_height }
}

enum LineRule {
    /// `w:line` counts 240ths of a line, the default when no rule is declared.
    Multiple,
    /// `w:line` is a twip length, used by `exact` and `atLeast`.
    Length,
}

fn line_rule(spacing: &oak_xml::ast::XmlElement) -> LineRule {
    match attribute_value(spacing, "lineRule").as_deref() {
        Some("exact" | "atLeast") => LineRule::Length,
        _ => LineRule::Multiple,
    }
}

#[cfg(test)]
mod tests {
    use super::{StyleDefaults, parse_styles_xml_lossy};

    #[test]
    fn doc_defaults_declare_body_size_and_line_spacing() {
        let xml = br#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:docDefaults>
              <w:rPrDefault><w:rPr><w:sz w:val="25"/></w:rPr></w:rPrDefault>
              <w:pPrDefault><w:pPr><w:spacing w:line="302" w:lineRule="auto"/></w:pPr></w:pPrDefault>
            </w:docDefaults>
          </w:styles>"#;
        let defaults = parse_styles_xml_lossy(xml);
        assert_eq!(defaults.base_font_size, Some(12.5));
        assert_eq!(defaults.line_height, Some(302.0 / 240.0));
    }

    #[test]
    fn exact_line_spacing_is_a_length_over_the_body_size() {
        let xml = br#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:docDefaults>
              <w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault>
              <w:pPrDefault><w:pPr><w:spacing w:line="290" w:lineRule="exact"/></w:pPr></w:pPrDefault>
            </w:docDefaults>
          </w:styles>"#;
        let defaults = parse_styles_xml_lossy(xml);
        assert_eq!(defaults.base_font_size, Some(11.0));
        assert_eq!(defaults.line_height, Some(14.5 / 11.0));
    }

    #[test]
    fn missing_styles_part_yields_no_defaults() {
        assert_eq!(parse_styles_xml_lossy(b""), StyleDefaults::default());
        assert_eq!(parse_styles_xml_lossy(b"<w:styles/>"), StyleDefaults::default());
    }
}
