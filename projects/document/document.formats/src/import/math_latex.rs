//! Shared LaTeX spelling helpers for document formats that author math
//! natively instead of as TeX.
//!
//! DOCX stores OMML and HTML/EPUB store MathML. Both are lowered to the same
//! LaTeX body and then handed to [`crate::import::tex_math`], so the document
//! IR carries exactly one math representation (`MathFormula`). This module
//! never parses math itself and never synthesizes a second semantic tree.

use document_ir::{DocumentGraph, LossMarker, MathDisplay, MathFormula, SemanticStatus};

/// Record math constructs that had no structured mapping.
pub(crate) fn record_math_unsupported(graph: &mut DocumentGraph, code: &str, constructs: &[String]) {
    if constructs.is_empty() {
        return;
    }
    graph.push_loss(LossMarker {
        code: code.to_string(),
        message: format!(
            "math constructs {} have no structured mapping and were kept as readable text inside the formula",
            constructs.join(", ")
        ),
        status: SemanticStatus::Partial,
    });
}

/// Build a [`MathFormula`] from a generated LaTeX body.
pub(crate) fn formula_from_latex(source: &str, display: MathDisplay) -> MathFormula {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return MathFormula::unsupported(String::new(), Some("latex".into()), display, "empty");
    }
    crate::import::tex_math::lower_formula_source(trimmed, display, "latex")
}

/// Escape text that enters a TeX `\text{...}` argument.
pub(crate) fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '$' => out.push_str("\\$"),
            '%' => out.push_str("\\%"),
            '&' => out.push_str("\\&"),
            '#' => out.push_str("\\#"),
            '_' => out.push_str("\\_"),
            '^' => out.push_str("\\textasciicircum{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            other => out.push(other),
        }
    }
    out
}

/// Escape authored math text while keeping `^` and `_` as script syntax.
///
/// DOCX authors routinely write `E = m c^2` as one `m:t` run, so the two
/// script markers must reach the TeX parser unchanged. Only characters that
/// would break a formula body are re-spelled.
pub(crate) fn escape_math_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\backslash"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '$' => out.push_str("\\$"),
            '%' => out.push_str("\\%"),
            '&' => out.push_str("\\&"),
            '#' => out.push_str("\\#"),
            other => match latex_char(other) {
                Some(command) => out.push_str(command),
                None => out.push(other),
            },
        }
    }
    out
}

/// Spell one operator or symbol as a LaTeX command when one exists.
pub(crate) fn latex_operator(text: &str) -> String {
    if let Some(command) = operator_command(text) {
        return command.to_string();
    }
    escape_math_text(text)
}

/// Map one authored glyph to its TeX spelling.
fn latex_char(character: char) -> Option<&'static str> {
    operator_command(&character.to_string())
}

fn operator_command(text: &str) -> Option<&'static str> {
    match text {
        "−" | "-" => Some("-"),
        "×" => Some("\\times"),
        "·" | "⋅" => Some("\\cdot"),
        "÷" => Some("\\div"),
        "±" => Some("\\pm"),
        "∓" => Some("\\mp"),
        "≤" => Some("\\le"),
        "≥" => Some("\\ge"),
        "≠" => Some("\\ne"),
        "≈" => Some("\\approx"),
        "≡" => Some("\\equiv"),
        "→" => Some("\\to"),
        "←" => Some("\\leftarrow"),
        "↔" => Some("\\leftrightarrow"),
        "⇒" => Some("\\Rightarrow"),
        "∈" => Some("\\in"),
        "∉" => Some("\\notin"),
        "⊂" => Some("\\subset"),
        "⊆" => Some("\\subseteq"),
        "⊃" => Some("\\supset"),
        "⊇" => Some("\\supseteq"),
        "∪" => Some("\\cup"),
        "∩" => Some("\\cap"),
        "∑" => Some("\\sum"),
        "∏" => Some("\\prod"),
        "∮" => Some("\\oint"),
        "∫" => Some("\\int"),
        "∬" => Some("\\iint"),
        "∞" => Some("\\infty"),
        "∂" => Some("\\partial"),
        "∇" => Some("\\nabla"),
        "∀" => Some("\\forall"),
        "∃" => Some("\\exists"),
        "∅" => Some("\\emptyset"),
        "…" => Some("\\ldots"),
        "⋯" => Some("\\cdots"),
        "⋱" => Some("\\ddots"),
        "′" => Some("\\prime"),
        "⟨" => Some("\\langle"),
        "⟩" => Some("\\rangle"),
        "‖" => Some("\\Vert"),
        "∣" => Some("|"),
        _ => None,
    }
}

/// Spell a delimiter character for `\left` / `\right`.
///
/// An empty spelling or a lone `.` means an invisible delimiter, which TeX
/// spells as `.` as well.
pub(crate) fn delimiter_spelling(text: &str) -> String {
    let text = text.trim();
    match text {
        "" | "." => ".".to_string(),
        "{" => "\\{".to_string(),
        "}" => "\\}".to_string(),
        "‖" => "\\|".to_string(),
        "⟨" | "<" => "\\langle".to_string(),
        "⟩" | ">" => "\\rangle".to_string(),
        other => other.to_string(),
    }
}
