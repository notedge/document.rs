//! Office Math Markup Language (OMML) lowering for DOCX import.
//!
//! The OMML tree is projected to a LaTeX body and then lowered through the
//! same [`crate::import::tex_math`] path as authored TeX. No second math IR is
//! introduced. Constructs without a mapping keep their readable text and are
//! reported as `import.docx.math_unsupported` losses by the caller.

use document_ir::{MathDisplay, MathFormula};
use oak_xml::ast::{XmlElement, XmlValue};

use super::xml_extensions::{attribute_value, child_element, element_text, local_name};
use crate::import::math_latex::{escape_math_text, escape_text, formula_from_latex, latex_operator};

/// Lower one `m:oMath` element.
///
/// Returns the formula plus the OMML constructs that had no mapping. An empty
/// `m:oMath` still yields a formula carrying the readable fallback text so a
/// math paragraph never disappears silently.
pub(crate) fn lower_omml(element: &XmlElement, display: MathDisplay) -> (MathFormula, Vec<String>) {
    let mut builder = Builder::default();
    builder.lower_children(element);
    let body = builder.out.trim().to_string();
    let body = if body.is_empty() {
        let text = element_text(element);
        if text.trim().is_empty() { String::new() } else { format!("\\text{{{}}}", escape_text(text.trim())) }
    }
    else {
        body
    };
    let formula = formula_from_latex(&body, display);
    (formula, builder.unsupported)
}

/// The first `m:oMath` descendant of an `m:oMathPara`.
pub(crate) fn omath_children(element: &XmlElement) -> Vec<&XmlElement> {
    element.children.iter().filter_map(XmlValue::as_element).filter(|child| local_name(&child.name) == "oMath").collect()
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

    fn lower_children(&mut self, element: &XmlElement) {
        for child in element.children.iter().filter_map(XmlValue::as_element) {
            self.lower(child);
        }
    }

    fn lower(&mut self, element: &XmlElement) {
        let tag = local_name(&element.name).to_string();
        match tag.as_str() {
            "oMath" | "oMathPara" | "e" | "num" | "den" | "sup" | "sub" | "deg" | "fName" | "lim" | "box" | "borderBox"
            | "phant" | "m" | "mr" | "eqArr" | "r" => self.lower_container(&tag, element),
            "sSup" | "sSub" | "sSubSup" | "sPre" => self.lower_script(&tag, element),
            "f" => self.lower_fraction(element),
            "rad" => self.lower_radical(element),
            "d" => self.lower_delimiter(element),
            "nary" => self.lower_nary(element),
            "func" => self.lower_function(element),
            "acc" => self.lower_accent(element),
            "bar" => self.lower_bar(element),
            "groupChr" => self.lower_group_character(element),
            "limLow" => self.lower_limit("underset", element),
            "limUpp" => self.lower_limit("overset", element),
            other => self.lower_unknown(other, element),
        }
    }

    fn lower_container(&mut self, tag: &str, element: &XmlElement) {
        match tag {
            "r" => self.lower_run(element),
            "m" => self.lower_matrix(element),
            "eqArr" => self.lower_equation_array(element),
            _ => self.lower_children(element),
        }
    }

    fn lower_run(&mut self, element: &XmlElement) {
        let mut text = String::new();
        for child in element.children.iter().filter_map(XmlValue::as_element) {
            if local_name(&child.name) == "t" {
                text.push_str(&element_text(child));
            }
        }
        if text.trim().is_empty() {
            return;
        }
        let style = child_element(element, "rPr")
            .and_then(|properties| child_element(properties, "sty"))
            .and_then(|sty| attribute_value(sty, "val"))
            .unwrap_or_else(|| "default".into());
        let body = escape_math_text(&text);
        match style.as_str() {
            "p" => self.out.push_str(&format!("\\mathrm{{{body}}}")),
            "b" => self.out.push_str(&format!("\\mathbf{{{body}}}")),
            "i" => self.out.push_str(&format!("\\mathit{{{body}}}")),
            "bi" => self.out.push_str(&format!("\\mathbf{{\\mathit{{{body}}}}}")),
            _ => self.out.push_str(&body),
        }
    }

    fn lower_script(&mut self, tag: &str, element: &XmlElement) {
        let base = child_element(element, "e");
        let subscript = child_element(element, "sub");
        let superscript = child_element(element, "sup");
        match tag {
            "sSup" => {
                self.out.push('{');
                self.lower_optional(base);
                self.out.push_str("}^{");
                self.lower_optional(superscript);
                self.out.push('}');
            }
            "sSub" => {
                self.out.push('{');
                self.lower_optional(base);
                self.out.push_str("}_{");
                self.lower_optional(subscript);
                self.out.push('}');
            }
            "sSubSup" => {
                self.out.push('{');
                self.lower_optional(base);
                self.out.push_str("}_{");
                self.lower_optional(subscript);
                self.out.push_str("}^{");
                self.lower_optional(superscript);
                self.out.push('}');
            }
            "sPre" => {
                self.out.push_str("{}_{");
                self.lower_optional(subscript);
                self.out.push_str("}^{");
                self.lower_optional(superscript);
                self.out.push_str("{");
                self.lower_optional(base);
                self.out.push('}');
            }
            _ => self.lower_children(element),
        }
    }

    fn lower_fraction(&mut self, element: &XmlElement) {
        let fraction_type = child_element(element, "fPr")
            .and_then(|properties| child_element(properties, "type"))
            .and_then(|kind| attribute_value(kind, "val"))
            .unwrap_or_else(|| "bar".into());
        let numerator = child_element(element, "num");
        let denominator = child_element(element, "den");
        if fraction_type == "noBar" {
            self.out.push_str("\\binom{");
        }
        else {
            self.out.push_str("\\frac{");
        }
        self.lower_optional(numerator);
        self.out.push_str("}{");
        self.lower_optional(denominator);
        self.out.push('}');
    }

    fn lower_radical(&mut self, element: &XmlElement) {
        let degree = child_element(element, "deg").filter(|degree| !element_text(degree).trim().is_empty());
        let radicand = child_element(element, "e");
        self.out.push_str("\\sqrt");
        if let Some(degree) = degree {
            self.out.push('[');
            self.lower(degree);
            self.out.push(']');
        }
        self.out.push('{');
        self.lower_optional(radicand);
        self.out.push('}');
    }

    fn lower_delimiter(&mut self, element: &XmlElement) {
        let properties = child_element(element, "dPr");
        let open = properties
            .and_then(|value| child_element(value, "begChr"))
            .and_then(|value| attribute_value(value, "val"))
            .unwrap_or_else(|| "(".into());
        let close = properties
            .and_then(|value| child_element(value, "endChr"))
            .and_then(|value| attribute_value(value, "val"))
            .unwrap_or_else(|| ")".into());
        let separator = properties
            .and_then(|value| child_element(value, "sepChr"))
            .and_then(|value| attribute_value(value, "val"))
            .unwrap_or_else(|| "|".into());
        self.out.push_str("\\left");
        self.out.push_str(&crate::import::math_latex::delimiter_spelling(&open));
        self.out.push(' ');
        let mut first = true;
        for child in element.children.iter().filter_map(XmlValue::as_element) {
            if local_name(&child.name) != "e" {
                continue;
            }
            if !first {
                self.out.push_str(&latex_operator(&separator));
                self.out.push(' ');
            }
            first = false;
            self.lower_children(child);
        }
        self.out.push_str(" \\right");
        self.out.push_str(&crate::import::math_latex::delimiter_spelling(&close));
    }

    fn lower_nary(&mut self, element: &XmlElement) {
        let character = child_element(element, "naryPr")
            .and_then(|properties| child_element(properties, "chr"))
            .and_then(|chr| attribute_value(chr, "val"))
            .unwrap_or_else(|| "\u{222B}".into());
        self.out.push_str(&latex_operator(&character));
        if let Some(subscript) = child_element(element, "sub") {
            if !element_text(subscript).trim().is_empty() {
                self.out.push_str("_{");
                self.lower_children(subscript);
                self.out.push('}');
            }
        }
        if let Some(superscript) = child_element(element, "sup") {
            if !element_text(superscript).trim().is_empty() {
                self.out.push_str("^{");
                self.lower_children(superscript);
                self.out.push('}');
            }
        }
        self.out.push(' ');
        if let Some(body) = child_element(element, "e") {
            self.lower_children(body);
        }
    }

    fn lower_function(&mut self, element: &XmlElement) {
        let name = child_element(element, "fName");
        let argument = child_element(element, "e");
        if let Some(name) = name {
            let text = element_text(name).trim().to_string();
            if text.is_empty() {
                self.lower(name);
            }
            else {
                self.out.push_str(&format!("\\operatorname{{{}}}", escape_text(&text)));
            }
        }
        else {
            self.note_unsupported("omml.func.fName");
        }
        self.out.push('{');
        self.lower_optional(argument);
        self.out.push('}');
    }

    fn lower_accent(&mut self, element: &XmlElement) {
        let accent = child_element(element, "accPr")
            .and_then(|properties| child_element(properties, "chr"))
            .and_then(|chr| attribute_value(chr, "val"))
            .unwrap_or_default();
        let command = accent_command(&accent);
        let body = child_element(element, "e");
        match command {
            Some(command) => {
                self.out.push_str(&format!("\\{command}{{"));
                self.lower_optional(body);
                self.out.push('}');
            }
            None => {
                self.out.push_str("\\overset{");
                if accent.is_empty() {
                    self.out.push_str("\\ ");
                }
                else {
                    self.out.push_str(&latex_operator(&accent));
                }
                self.out.push_str("}{");
                self.lower_optional(body);
                self.out.push('}');
            }
        }
    }

    fn lower_bar(&mut self, element: &XmlElement) {
        let position = child_element(element, "barPr")
            .and_then(|properties| child_element(properties, "pos"))
            .and_then(|pos| attribute_value(pos, "val"))
            .unwrap_or_else(|| "top".into());
        let command = if position.eq_ignore_ascii_case("bot") { "underset" } else { "overline" };
        if command == "overline" {
            self.out.push_str("\\overline{");
            if let Some(body) = child_element(element, "e") {
                self.lower_children(body);
            }
            self.out.push('}');
        }
        else {
            self.out.push_str("\\underset{\\text{\u{00AF}}}{");
            if let Some(body) = child_element(element, "e") {
                self.lower_children(body);
            }
            self.out.push('}');
        }
    }

    fn lower_group_character(&mut self, element: &XmlElement) {
        let position = child_element(element, "groupChrPr")
            .and_then(|properties| child_element(properties, "pos"))
            .and_then(|pos| attribute_value(pos, "val"))
            .unwrap_or_else(|| "top".into());
        let (command, glyph) =
            if position.eq_ignore_ascii_case("bot") { ("underset", "\u{23DF}") } else { ("overset", "\u{23DE}") };
        self.out.push_str(&format!("\\{command}{{\\text{{{glyph}}}}}{{"));
        if let Some(body) = child_element(element, "e") {
            self.lower_children(body);
        }
        self.out.push('}');
    }

    fn lower_limit(&mut self, command: &str, element: &XmlElement) {
        let body = child_element(element, "e");
        let limit = child_element(element, "lim");
        self.out.push_str(&format!("\\{command}{{"));
        self.lower_optional(limit);
        self.out.push_str("}{");
        self.lower_optional(body);
        self.out.push('}');
    }

    fn lower_matrix(&mut self, element: &XmlElement) {
        let rows: Vec<&XmlElement> =
            element.children.iter().filter_map(XmlValue::as_element).filter(|child| local_name(&child.name) == "mr").collect();
        if rows.is_empty() {
            self.note_unsupported("omml.m.empty");
            self.lower_children(element);
            return;
        }
        self.out.push_str("\\begin{matrix}");
        for (row_index, row) in rows.iter().enumerate() {
            if row_index > 0 {
                self.out.push_str(" \\\\ ");
            }
            let mut first = true;
            for cell in row.children.iter().filter_map(XmlValue::as_element) {
                if local_name(&cell.name) != "e" {
                    continue;
                }
                if !first {
                    self.out.push_str(" & ");
                }
                first = false;
                self.lower_children(cell);
            }
        }
        self.out.push_str("\\end{matrix}");
    }

    fn lower_equation_array(&mut self, element: &XmlElement) {
        let rows: Vec<&XmlElement> =
            element.children.iter().filter_map(XmlValue::as_element).filter(|child| local_name(&child.name) == "e").collect();
        if rows.is_empty() {
            self.lower_children(element);
            return;
        }
        self.out.push_str("\\begin{aligned}");
        for (row_index, row) in rows.iter().enumerate() {
            if row_index > 0 {
                self.out.push_str(" \\\\ ");
            }
            self.lower_children(row);
        }
        self.out.push_str("\\end{aligned}");
    }

    fn lower_unknown(&mut self, tag: &str, element: &XmlElement) {
        // Control properties describe presentation and carry no math of their
        // own, so they are skipped without a loss marker.
        if is_control_property(tag) {
            return;
        }
        self.note_unsupported(format!("omml.{tag}"));
        let text = element_text(element);
        if !text.trim().is_empty() {
            self.out.push_str(&format!("\\text{{{}}}", escape_text(text.trim())));
        }
        else {
            self.lower_children(element);
        }
    }

    fn lower_optional(&mut self, element: Option<&XmlElement>) {
        if let Some(element) = element {
            self.lower_children(element);
        }
    }
}

fn is_control_property(tag: &str) -> bool {
    matches!(
        tag,
        "rPr"
            | "ctrlPr"
            | "argPr"
            | "oMathParaPr"
            | "fPr"
            | "dPr"
            | "naryPr"
            | "accPr"
            | "barPr"
            | "groupChrPr"
            | "radPr"
            | "funcPr"
            | "sSupPr"
            | "sSubPr"
            | "sSubSupPr"
            | "sPrePr"
            | "mPr"
            | "limLowPr"
            | "limUppPr"
            | "eqArrPr"
            | "boxPr"
            | "borderBoxPr"
            | "phantPr"
    )
}

fn accent_command(accent: &str) -> Option<&'static str> {
    match accent {
        "\u{0302}" => Some("hat"),
        "\u{0303}" => Some("tilde"),
        "\u{0304}" => Some("bar"),
        "\u{0305}" => Some("overline"),
        "\u{0306}" => Some("breve"),
        "\u{0307}" => Some("dot"),
        "\u{0308}" => Some("ddot"),
        "\u{030C}" => Some("check"),
        "\u{20D7}" => Some("vec"),
        _ => None,
    }
}
