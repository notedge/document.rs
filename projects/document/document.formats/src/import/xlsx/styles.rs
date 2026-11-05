//! `xl/styles.xml` and `xl/theme/theme1.xml` parsing.

use std::collections::BTreeMap;

use oak_xml::ast::{XmlElement, XmlValue};

use super::model::{
    BorderEdge, BorderEdges, CellStyle, HorizontalAlign, VerticalAlign, WorkbookStyles, XlsxIssue, codes,
};
use crate::import::office::{
    attribute_value, bool_attribute, child_named, children_named, document_root, element_text, f64_attribute, local_name,
    parse_xml, u32_attribute,
};
use crate::FormatError;

/// Parses the workbook style table, resolving cell formats against fonts, fills
/// and borders.
pub(crate) fn parse_styles(xml: &[u8], theme: Option<&[u8]>) -> Result<(WorkbookStyles, Vec<XlsxIssue>), FormatError> {
    let value = parse_xml(xml, "xlsx")?;
    let root = document_root(&value, "xlsx")?;
    let mut issues = Vec::new();
    let theme_colors = theme.map(parse_theme_colors).transpose()?.unwrap_or_default();

    let mut number_formats = BTreeMap::new();
    if let Some(block) = child_named(root, "numFmts") {
        for format in children_named(block, "numFmt") {
            if let (Some(id), Some(code)) = (u32_attribute(format, "numFmtId"), attribute_value(format, "formatCode")) {
                number_formats.insert(id, code);
            }
        }
    }

    let fonts = child_named(root, "fonts").map(parse_fonts).unwrap_or_default();
    let fills = child_named(root, "fills").map(parse_fills).unwrap_or_default();
    let borders = child_named(root, "borders").map(parse_borders).unwrap_or_default();

    let mut cell_formats = Vec::new();
    if let Some(block) = child_named(root, "cellXfs") {
        for format in children_named(block, "xf") {
            let font_index = u32_attribute(format, "fontId").unwrap_or(0) as usize;
            let fill_index = u32_attribute(format, "fillId").unwrap_or(0) as usize;
            let border_index = u32_attribute(format, "borderId").unwrap_or(0) as usize;
            let number_format_id = u32_attribute(format, "numFmtId").unwrap_or(0);
            let mut style = CellStyle {
                number_format_id,
                number_format: number_formats
                    .get(&number_format_id)
                    .cloned()
                    .or_else(|| builtin_number_format(number_format_id).map(str::to_owned)),
                ..CellStyle::default()
            };
            if let Some(font) = fonts.get(font_index) {
                style.bold = font.bold;
                style.italic = font.italic;
                style.underline = font.underline;
                style.strike = font.strike;
                style.font_name = font.name.clone();
                style.font_size = font.size;
                style.font_color = font.color.clone();
            }
            if let Some(fill) = fills.get(fill_index) {
                style.fill_color = fill.clone();
            }
            style.borders = borders.get(border_index).cloned().unwrap_or_default();
            if let Some(alignment) = child_named(format, "alignment") {
                style.horizontal = parse_horizontal(attribute_value(alignment, "horizontal").as_deref());
                style.vertical = parse_vertical(attribute_value(alignment, "vertical").as_deref());
                style.wrap_text = bool_attribute(alignment, "wrapText").unwrap_or(false);
            }
            cell_formats.push(style);
        }
    }

    let styles = WorkbookStyles { number_formats, cell_formats, theme_colors };
    for (index, style) in styles.cell_formats.iter().enumerate() {
        if style.font_color.as_deref().is_some_and(|color| resolve_color(color, &styles).is_none()) {
            issues.push(unresolved_theme(index, "font"));
        }
        if style.fill_color.as_deref().is_some_and(|color| resolve_color(color, &styles).is_none()) {
            issues.push(unresolved_theme(index, "fill"));
        }
    }

    Ok((styles, issues))
}

fn unresolved_theme(index: usize, slot: &str) -> XlsxIssue {
    XlsxIssue {
        code: codes::THEME_COLOR.into(),
        message: format!("cell format {index} {slot} color references an unresolved theme slot"),
        status: super::model::IssueStatus::Partial,
        sheet: None,
        cell: None,
    }
}

#[derive(Debug, Default, Clone)]
struct FontRecord {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    name: Option<String>,
    size: Option<f32>,
    color: Option<String>,
}

fn parse_fonts(block: &XmlElement) -> Vec<FontRecord> {
    children_named(block, "font")
        .into_iter()
        .map(|font| FontRecord {
            bold: child_named(font, "b").is_some(),
            italic: child_named(font, "i").is_some(),
            underline: child_named(font, "u").is_some(),
            strike: child_named(font, "strike").is_some(),
            name: child_named(font, "name").and_then(|name| attribute_value(name, "val")),
            size: child_named(font, "sz").and_then(|size| f64_attribute(size, "val")).map(|size| size as f32),
            color: child_named(font, "color").and_then(parse_color),
        })
        .collect()
}

fn parse_fills(block: &XmlElement) -> Vec<Option<String>> {
    children_named(block, "fill")
        .into_iter()
        .map(|fill| {
            let pattern = child_named(fill, "patternFill")?;
            if attribute_value(pattern, "patternType").as_deref() != Some("solid") {
                return None;
            }
            child_named(pattern, "fgColor").and_then(parse_color)
        })
        .collect()
}

fn parse_borders(block: &XmlElement) -> Vec<BorderEdges> {
    children_named(block, "border")
        .into_iter()
        .map(|border| BorderEdges {
            top: parse_edge(border, "top"),
            right: parse_edge(border, "right"),
            bottom: parse_edge(border, "bottom"),
            left: parse_edge(border, "left"),
        })
        .collect()
}

fn parse_edge(border: &XmlElement, name: &str) -> Option<BorderEdge> {
    let edge = child_named(border, name)?;
    let style = attribute_value(edge, "style")?;
    if style == "none" {
        return None;
    }
    Some(BorderEdge { style, color: child_named(edge, "color").and_then(parse_color) })
}

fn parse_horizontal(value: Option<&str>) -> HorizontalAlign {
    match value {
        Some("left") => HorizontalAlign::Left,
        Some("center") => HorizontalAlign::Center,
        Some("right") => HorizontalAlign::Right,
        Some("justify") => HorizontalAlign::Justify,
        Some("distributed") => HorizontalAlign::Distributed,
        Some("fill") => HorizontalAlign::Fill,
        _ => HorizontalAlign::General,
    }
}

fn parse_vertical(value: Option<&str>) -> VerticalAlign {
    match value {
        Some("top") => VerticalAlign::Top,
        Some("center") => VerticalAlign::Center,
        Some("justify") => VerticalAlign::Justify,
        Some("distributed") => VerticalAlign::Distributed,
        _ => VerticalAlign::Bottom,
    }
}

/// Parses a color child into `#RRGGBB`, a theme reference or an indexed color.
fn parse_color(element: &XmlElement) -> Option<String> {
    if let Some(rgb) = attribute_value(element, "rgb") {
        return Some(normalize_argb(&rgb));
    }
    if let Some(theme) = attribute_value(element, "theme") {
        let tint = attribute_value(element, "tint");
        return Some(match tint {
            Some(tint) => format!("theme:{theme}:{tint}"),
            None => format!("theme:{theme}"),
        });
    }
    if let Some(indexed) = u32_attribute(element, "indexed") {
        return Some(indexed_color(indexed as usize));
    }
    None
}

fn normalize_argb(value: &str) -> String {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() == 8 {
        format!("#{}", &hex[2..])
    }
    else {
        format!("#{hex}")
    }
}

fn indexed_color(index: usize) -> String {
    const PALETTE: [&str; 66] = [
        "#000000", "#FFFFFF", "#FF0000", "#00FF00", "#0000FF", "#FFFF00", "#FF00FF", "#00FFFF", "#000000", "#FFFFFF",
        "#FF0000", "#00FF00", "#0000FF", "#FFFF00", "#FF00FF", "#00FFFF", "#800000", "#008000", "#000080", "#808000",
        "#800080", "#008080", "#C0C0C0", "#808080", "#9999FF", "#993366", "#FFFFCC", "#CCFFFF", "#660066", "#FF8080",
        "#0066CC", "#CCCCFF", "#000080", "#FF00FF", "#FFFF00", "#00FFFF", "#800080", "#800000", "#008080", "#0000FF",
        "#00CCFF", "#CCFFFF", "#CCFFCC", "#FFFF99", "#99CCFF", "#FF99CC", "#CC99FF", "#FFCC99", "#3366FF", "#33CCCC",
        "#99CC00", "#FFCC00", "#FF9900", "#FF6600", "#666699", "#969696", "#003366", "#339966", "#003300", "#333300",
        "#993300", "#993366", "#333399", "#333333", "#000000", "#FFFFFF",
    ];
    PALETTE.get(index).copied().unwrap_or("#000000").to_owned()
}

/// Resolves `theme:<index>[:<tint>]` against the theme color table.
pub(crate) fn resolve_color(value: &str, styles: &WorkbookStyles) -> Option<String> {
    if !value.starts_with("theme:") {
        return Some(value.to_owned());
    }
    let mut parts = value.split(':');
    parts.next();
    let slot = parts.next()?;
    let tint = parts.next().and_then(|tint| tint.parse::<f64>().ok());
    let base = theme_slot(slot, styles)?;
    Some(match tint {
        Some(tint) => apply_tint(&base, tint),
        None => base,
    })
}

fn theme_slot(slot: &str, styles: &WorkbookStyles) -> Option<String> {
    let name = match slot {
        "0" => "lt1",
        "1" => "dk1",
        "2" => "lt2",
        "3" => "dk2",
        "4" => "accent1",
        "5" => "accent2",
        "6" => "accent3",
        "7" => "accent4",
        "8" => "accent5",
        "9" => "accent6",
        "10" => "hlink",
        "11" => "folHlink",
        other => other,
    };
    styles.theme_colors.get(name).cloned()
}

fn apply_tint(base: &str, tint: f64) -> String {
    let hex = base.trim_start_matches('#');
    if hex.len() != 6 {
        return base.to_owned();
    }
    let parse = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).unwrap_or(0) as f64;
    let adjust = |channel: f64| {
        let shifted = if tint < 0.0 {
            channel * (1.0 + tint)
        }
        else {
            channel * (1.0 - tint) + 255.0 * tint
        };
        shifted.clamp(0.0, 255.0) as u8
    };
    format!("#{:02X}{:02X}{:02X}", adjust(parse(0..2)), adjust(parse(2..4)), adjust(parse(4..6)))
}

/// Parses `theme1.xml` `clrScheme` slots.
fn parse_theme_colors(xml: &[u8]) -> Result<BTreeMap<String, String>, FormatError> {
    let value = parse_xml(xml, "xlsx")?;
    let root = document_root(&value, "xlsx")?;
    let mut colors = BTreeMap::new();
    let Some(scheme) = descendant_named(root, "clrScheme")
    else {
        return Ok(colors);
    };
    for slot in children_elements(scheme) {
        let name = local_name(&slot.name).to_owned();
        let rgb = children_elements(slot)
            .into_iter()
            .find_map(|child| {
                let last = attribute_value(child, "lastClr");
                last.or_else(|| attribute_value(child, "val"))
            })
            .map(|value| normalize_argb(&value));
        if let Some(color) = rgb {
            colors.insert(name, color);
        }
    }
    Ok(colors)
}

fn descendant_named<'a>(element: &'a XmlElement, name: &str) -> Option<&'a XmlElement> {
    for child in children_elements(element) {
        if local_name(&child.name) == name {
            return Some(child);
        }
        if let Some(found) = descendant_named(child, name) {
            return Some(found);
        }
    }
    None
}

fn children_elements(element: &XmlElement) -> Vec<&XmlElement> {
    element
        .children
        .iter()
        .filter_map(|child| match child {
            XmlValue::Element(child) => Some(child),
            XmlValue::Fragment(values) => values.iter().find_map(|value| match value {
                XmlValue::Element(child) => Some(child),
                _ => None,
            }),
            _ => None,
        })
        .collect()
}

/// Built-in number format codes defined by ECMA-376 §18.8.30.
pub(crate) fn builtin_number_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "mm-dd-yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// Reads a `t`-style text leaf from one element.
#[allow(dead_code)]
pub(crate) fn text_value(element: &XmlElement) -> String {
    element_text(element)
}
