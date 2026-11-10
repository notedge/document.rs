//! Shared MathIR helpers for the language math lowerings.
//!
//! The TeX and Typst import profiles lower their own syntax into the same
//! `document_ir::math` tree. Contract 28 requires one authored construct to
//! land on identical nodes regardless of the spelling language, so the atom
//! classes, the named-symbol table and the sequence-folding rules live here
//! once and both lowerings consume them. This module owns no syntax: it never
//! sees tokens or parsers, only characters, names and [`MathExpression`] nodes.

use document_ir::{MathAtom, MathClass, MathExpression, MathSpan};

/// Byte span into the formula source for an absolute CST range.
pub(crate) fn span_from(base: usize, start: usize, end: usize) -> Option<MathSpan> {
    if end <= start {
        return None;
    }
    Some(MathSpan { start: start.saturating_sub(base) as u32, end: end.saturating_sub(base) as u32 })
}

/// The authored slice, clamped to the source bounds.
pub(crate) fn slice(source: &str, start: usize, end: usize) -> String {
    source.get(start..end).unwrap_or("").to_string()
}

/// A fragment the lowering profile could not express.
pub(crate) fn unsupported(start: usize, end: usize, source: &str, kind: &str, base: usize) -> MathExpression {
    MathExpression::Unsupported { kind: kind.to_string(), source: slice(source, start, end), span: span_from(base, start, end) }
}

/// Build one sequence, collapsing a single expression to itself.
pub(crate) fn sequence(mut expressions: Vec<MathExpression>) -> MathExpression {
    if expressions.len() == 1 {
        return expressions.pop().expect("one expression");
    }
    MathExpression::Sequence(expressions)
}

/// Append an expression, flattening nested sequences into the enclosing list.
pub(crate) fn append_expression(expressions: &mut Vec<MathExpression>, expression: MathExpression) {
    match expression {
        MathExpression::Sequence(items) => {
            for item in items {
                append_expression(expressions, item);
            }
        }
        other => expressions.push(other),
    }
}

/// Wrap a base with its scripts, or return the base when no script was authored.
pub(crate) fn script_expression(
    base: MathExpression,
    superscript: Option<MathExpression>,
    subscript: Option<MathExpression>,
) -> MathExpression {
    if superscript.is_none() && subscript.is_none() {
        return base;
    }
    MathExpression::Script { base: Box::new(base), superscript: superscript.map(Box::new), subscript: subscript.map(Box::new) }
}

/// Fold a script onto the expression it modifies.
pub(crate) fn attach_script(
    expressions: &mut Vec<MathExpression>,
    superscript: Option<MathExpression>,
    subscript: Option<MathExpression>,
) {
    let Some(last) = expressions.pop()
    else {
        expressions.push(MathExpression::Unsupported { kind: "script-base".into(), source: String::new(), span: None });
        return;
    };
    match last {
        MathExpression::Script { base, superscript: existing_sup, subscript: existing_sub } => {
            expressions.push(script_expression(
                *base,
                superscript.or(existing_sup.map(|value| *value)),
                subscript.or(existing_sub.map(|value| *value)),
            ));
        }
        base => expressions.push(script_expression(base, superscript, subscript)),
    }
}

/// One character atom with its mathematical class.
pub(crate) fn char_atom(c: char, class: MathClass, span: Option<MathSpan>) -> MathExpression {
    MathExpression::Atom(MathAtom { text: c.to_string(), class, span })
}

/// Lower one character: digits become numbers, everything else an atom.
pub(crate) fn char_expression(c: char, span: Option<MathSpan>) -> MathExpression {
    if c.is_ascii_digit() {
        return MathExpression::Number { text: c.to_string(), span };
    }
    let class = class_for_char(c);
    char_atom(c, class, span)
}

/// The mathematical class TeX assigns to a literal character.
pub(crate) fn class_for_char(c: char) -> MathClass {
    match c {
        '0'..='9' => MathClass::Number,
        'a'..='z' | 'A'..='Z' => MathClass::Identifier,
        '+' | '-' | '*' | '/' | '×' | '·' | '±' | '∓' | '÷' => MathClass::Binary,
        '=' | '<' | '>' => MathClass::Relation,
        '(' | '[' => MathClass::Opening,
        ')' | ']' => MathClass::Closing,
        ',' | ';' => MathClass::Punctuation,
        c if is_greek(c) => MathClass::Identifier,
        _ => MathClass::Ordinary,
    }
}

/// Whether a character is a Greek letter.
fn is_greek(c: char) -> bool {
    matches!(c, '\u{0391}'..='\u{03A9}' | '\u{03B1}'..='\u{03C9}' | '\u{03D1}' | '\u{03D5}' | '\u{03D6}' | '\u{03F0}' | '\u{03F1}' | '\u{03F5}')
}

/// The Unicode symbol one named construct renders as, with its class.
///
/// TeX spells these as commands (`\alpha`), Typst as bare names (`alpha`).
/// Both lowerings resolve the same name to the same atom through this table,
/// which is what keeps identical constructs on identical MathIR nodes.
pub(crate) fn symbol_for(name: &str) -> Option<(&'static str, MathClass)> {
    let symbol = match name {
        "alpha" => ("α", MathClass::Ordinary),
        "beta" => ("β", MathClass::Ordinary),
        "gamma" => ("γ", MathClass::Ordinary),
        "delta" => ("δ", MathClass::Ordinary),
        // TeX's plain command names the variant glyph, and the `var` form
        // names the ordinary letter.
        "epsilon" => ("ϵ", MathClass::Ordinary),
        "varepsilon" => ("ε", MathClass::Ordinary),
        "zeta" => ("ζ", MathClass::Ordinary),
        "eta" => ("η", MathClass::Ordinary),
        "theta" => ("θ", MathClass::Ordinary),
        "vartheta" => ("ϑ", MathClass::Ordinary),
        "iota" => ("ι", MathClass::Ordinary),
        "kappa" => ("κ", MathClass::Ordinary),
        "varkappa" => ("ϰ", MathClass::Ordinary),
        "lambda" => ("λ", MathClass::Ordinary),
        "mu" => ("μ", MathClass::Ordinary),
        "nu" => ("ν", MathClass::Ordinary),
        "xi" => ("ξ", MathClass::Ordinary),
        "omicron" => ("ο", MathClass::Ordinary),
        "pi" => ("π", MathClass::Ordinary),
        "varpi" => ("ϖ", MathClass::Ordinary),
        "rho" => ("ρ", MathClass::Ordinary),
        "varrho" => ("ϱ", MathClass::Ordinary),
        "sigma" => ("σ", MathClass::Ordinary),
        "varsigma" => ("ς", MathClass::Ordinary),
        "tau" => ("τ", MathClass::Ordinary),
        "upsilon" => ("υ", MathClass::Ordinary),
        "phi" => ("ϕ", MathClass::Ordinary),
        "varphi" => ("φ", MathClass::Ordinary),
        "chi" => ("χ", MathClass::Ordinary),
        "psi" => ("ψ", MathClass::Ordinary),
        "omega" => ("ω", MathClass::Ordinary),
        "Gamma" => ("Γ", MathClass::Ordinary),
        "Delta" => ("Δ", MathClass::Ordinary),
        "Theta" => ("Θ", MathClass::Ordinary),
        "Lambda" => ("Λ", MathClass::Ordinary),
        "Xi" => ("Ξ", MathClass::Ordinary),
        "Pi" => ("Π", MathClass::Ordinary),
        "Sigma" => ("Σ", MathClass::Ordinary),
        "Upsilon" => ("Υ", MathClass::Ordinary),
        "Phi" => ("Φ", MathClass::Ordinary),
        "Psi" => ("Ψ", MathClass::Ordinary),
        "Omega" => ("Ω", MathClass::Ordinary),
        "sum" => ("∑", MathClass::LargeOperator),
        "prod" => ("∏", MathClass::LargeOperator),
        "coprod" => ("∐", MathClass::LargeOperator),
        "int" => ("∫", MathClass::LargeOperator),
        "iint" => ("∬", MathClass::LargeOperator),
        "oint" => ("∮", MathClass::LargeOperator),
        "pm" => ("±", MathClass::Binary),
        "mp" => ("∓", MathClass::Binary),
        "times" => ("×", MathClass::Binary),
        "cdot" => ("⋅", MathClass::Binary),
        "div" => ("÷", MathClass::Binary),
        "ast" => ("∗", MathClass::Binary),
        "star" => ("⋆", MathClass::Binary),
        "bullet" => ("∙", MathClass::Binary),
        "cup" => ("∪", MathClass::Binary),
        "cap" => ("∩", MathClass::Binary),
        "circ" => ("∘", MathClass::Binary),
        "le" | "leq" => ("≤", MathClass::Relation),
        "ge" | "geq" => ("≥", MathClass::Relation),
        "ne" | "neq" => ("≠", MathClass::Relation),
        "approx" => ("≈", MathClass::Relation),
        "equiv" => ("≡", MathClass::Relation),
        "sim" => ("∼", MathClass::Relation),
        "propto" => ("∝", MathClass::Relation),
        "to" | "rightarrow" => ("→", MathClass::Relation),
        "leftarrow" => ("←", MathClass::Relation),
        "leftrightarrow" => ("↔", MathClass::Relation),
        "Rightarrow" => ("⇒", MathClass::Relation),
        "in" => ("∈", MathClass::Relation),
        "notin" => ("∉", MathClass::Relation),
        "subset" => ("⊂", MathClass::Relation),
        "subseteq" => ("⊆", MathClass::Relation),
        "supset" => ("⊃", MathClass::Relation),
        "perp" => ("⊥", MathClass::Relation),
        "parallel" => ("∥", MathClass::Relation),
        "infty" => ("∞", MathClass::Ordinary),
        "partial" => ("∂", MathClass::Ordinary),
        "nabla" => ("∇", MathClass::Ordinary),
        "forall" => ("∀", MathClass::Ordinary),
        "exists" => ("∃", MathClass::Ordinary),
        "emptyset" => ("∅", MathClass::Ordinary),
        "ldots" => ("…", MathClass::Ordinary),
        "cdots" => ("⋯", MathClass::Ordinary),
        "ddots" => ("⋱", MathClass::Ordinary),
        "prime" => ("′", MathClass::Ordinary),
        "angle" => ("∠", MathClass::Ordinary),
        "degree" => ("°", MathClass::Ordinary),
        // Delimiters usable without `\left` / `\right`. TeX gives each one its
        // own class so spacing around it stays that of an opening or closing
        // atom; only `\vert` and `\Vert` are ordinary.
        "langle" => ("⟨", MathClass::Opening),
        "rangle" => ("⟩", MathClass::Closing),
        "lvert" => ("|", MathClass::Opening),
        "rvert" => ("|", MathClass::Closing),
        "lVert" => ("‖", MathClass::Opening),
        "rVert" => ("‖", MathClass::Closing),
        "vert" => ("|", MathClass::Ordinary),
        "Vert" => ("‖", MathClass::Ordinary),
        "lbrack" => ("[", MathClass::Opening),
        "rbrack" => ("]", MathClass::Closing),
        "lbrace" => ("{", MathClass::Opening),
        "rbrace" => ("}", MathClass::Closing),
        "lfloor" => ("⌊", MathClass::Opening),
        "rfloor" => ("⌋", MathClass::Closing),
        "lceil" => ("⌈", MathClass::Opening),
        "rceil" => ("⌉", MathClass::Closing),
        _ => return None,
    };
    Some(symbol)
}
