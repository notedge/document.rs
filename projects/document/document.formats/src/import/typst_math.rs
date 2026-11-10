//! Typst math body lowering into [`document_ir::MathFormula`].
//!
//! `oak-typst` carries no math mode: one `$ ... $` region arrives as a flat
//! sequence of text leaves, `^` is glued to the token that follows it and an
//! unpaired `_` is captured as emphasis. The structure therefore cannot come
//! from that AST, and the contract forbids spelling the body as TeX and
//! handing it to the TeX parser. This module is the Typst-side lowering: it
//! reads the authored body of one region with Typst's own math syntax and
//! produces the shared MathIR.
//!
//! Atom classes, the named-symbol table and the script-folding rules come
//! from [`super::math_common`], the same tables the TeX lowering consumes,
//! so one authored construct lands on identical MathIR nodes regardless of
//! the spelling language. Constructs outside the current subset become
//! [`MathExpression::Unsupported`] nodes and yield one [`MathLoss`] each:
//! the import keeps going and the page still shows the fragment.

use document_ir::{MathAtom, MathDisplay, MathExpression, MathFormula};

use super::math_common::{append_expression, attach_script, char_expression, sequence, span_from, symbol_for, unsupported};

/// One construct the Typst math subset cannot express, located in the body.
#[derive(Debug)]
pub(crate) struct MathLoss {
    /// Construct name for the `import.typst.math.<construct>` loss code.
    pub construct: String,
    /// The authored slice kept visible as source text.
    pub snippet: String,
    /// Body-relative byte range of the construct.
    pub span: (usize, usize),
}

/// Lower one authored `$ ... $` body into the shared math semantics.
///
/// `body` is the region content after the delimiters were removed and
/// trimmed, so it is also the [`MathFormula::source`]: every recorded span
/// indexes it directly. Each construct outside the subset yields one
/// [`MathLoss`] whose `span` is relative to the same body.
pub(crate) fn lower_typst_math(body: &str, display: MathDisplay) -> (MathFormula, Vec<MathLoss>) {
    let mut lowerer = TypstMathLowerer { body, cursor: 0, losses: Vec::new() };
    let expression = lowerer.lower_until(None);
    (MathFormula { source: body.to_string(), language: Some("typst".into()), display, expression }, lowerer.losses)
}

struct TypstMathLowerer<'a> {
    body: &'a str,
    cursor: usize,
    losses: Vec<MathLoss>,
}

impl<'a> TypstMathLowerer<'a> {
    fn peek(&self) -> Option<char> {
        self.body[self.cursor..].chars().next()
    }

    fn skip_space(&mut self) {
        while let Some(c) = self.peek() {
            if !c.is_whitespace() {
                break;
            }
            self.cursor += c.len_utf8();
        }
    }

    /// The maximal ASCII-letter run at the cursor, without consuming it.
    fn letter_run(&self) -> &'a str {
        let rest = &self.body[self.cursor..];
        let end = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
        &rest[..end]
    }

    /// Consume the maximal run of `take` at the cursor and return the slice.
    fn take_run(&mut self, take: impl Fn(char) -> bool) -> &'a str {
        let start = self.cursor;
        let rest = &self.body[start..];
        let end = rest.find(|c: char| !take(c)).unwrap_or(rest.len());
        self.cursor = start + end;
        &self.body[start..self.cursor]
    }

    fn push_loss(&mut self, construct: &str, start: usize, end: usize) {
        self.losses.push(MathLoss {
            construct: construct.to_string(),
            snippet: self.body[start..end].to_string(),
            span: (start, end),
        });
    }

    /// The offset just past the delimiter matching the one at `open`, or
    /// `None` when the body never closes the group.
    fn matching(&self, open: usize, close: char) -> Option<usize> {
        let open_char = self.body[open..].chars().next()?;
        let mut depth = 0usize;
        let mut index = open;
        while index < self.body.len() {
            let c = self.body[index..].chars().next()?;
            index += c.len_utf8();
            if c == open_char {
                depth += 1;
            }
            else if c == close {
                depth -= 1;
                if depth == 0 {
                    return Some(index - c.len_utf8());
                }
            }
        }
        None
    }

    /// Lower until the body ends (`stop` `None`) or until `stop` is seen.
    /// The stop character is not consumed.
    fn lower_until(&mut self, stop: Option<char>) -> MathExpression {
        let mut items: Vec<MathExpression> = Vec::new();
        loop {
            self.skip_space();
            let Some(c) = self.peek()
            else {
                break;
            };
            if stop == Some(c) {
                break;
            }
            let start = self.cursor;
            match c {
                '^' | '_' => {
                    self.cursor += 1;
                    match self.script_operand() {
                        Some(operand) if c == '^' => attach_script(&mut items, Some(operand), None),
                        Some(operand) => attach_script(&mut items, None, Some(operand)),
                        None => {
                            self.push_loss("script-operand", start, self.cursor);
                            append_expression(&mut items, unsupported(start, self.cursor, self.body, "script-operand", 0));
                        }
                    }
                }
                '#' => {
                    self.cursor += 1;
                    self.take_run(|c| !c.is_whitespace());
                    self.push_loss("calc", start, self.cursor);
                    append_expression(&mut items, unsupported(start, self.cursor, self.body, "calc", 0));
                }
                '\\' => {
                    self.cursor += 1;
                    let construct = if self.peek() == Some('\\') {
                        self.cursor += 1;
                        "linebreak"
                    }
                    else {
                        if let Some(c) = self.peek() {
                            self.cursor += c.len_utf8();
                        }
                        "escape"
                    };
                    self.push_loss(construct, start, self.cursor);
                    append_expression(&mut items, unsupported(start, self.cursor, self.body, construct, 0));
                }
                '[' | '{' => {
                    let close = if c == '[' { ']' } else { '}' };
                    let end = self.matching(start, close).map(|end| end + close.len_utf8()).unwrap_or(self.body.len());
                    self.cursor = end;
                    self.push_loss("content", start, end);
                    append_expression(&mut items, unsupported(start, end, self.body, "content", 0));
                }
                ']' | '}' => {
                    self.cursor += c.len_utf8();
                    self.push_loss("content", start, self.cursor);
                    append_expression(&mut items, unsupported(start, self.cursor, self.body, "content", 0));
                }
                c if c.is_ascii_alphabetic() => {
                    let run = self.letter_run();
                    if let Some((text, class)) = symbol_for(run) {
                        self.cursor += run.len();
                        append_expression(
                            &mut items,
                            MathExpression::Atom(MathAtom {
                                text: text.to_string(),
                                class,
                                span: span_from(0, start, self.cursor),
                            }),
                        );
                    }
                    else if self.body[self.cursor + run.len()..].starts_with('.') {
                        // A dotted symbol path (`arrow.r`, `tilde.a`) names a
                        // Typst symbol the shared table does not carry yet.
                        let end = self.dotted_path_end();
                        self.cursor = end;
                        self.push_loss("symbol", start, end);
                        append_expression(&mut items, unsupported(start, end, self.body, "symbol", 0));
                    }
                    else {
                        // Unknown names are plain identifiers: each letter is
                        // one italic atom, exactly as the TeX profile splits
                        // an unknown text token.
                        for (offset, ch) in run.char_indices() {
                            append_expression(
                                &mut items,
                                char_expression(ch, span_from(0, start + offset, start + offset + ch.len_utf8())),
                            );
                        }
                        self.cursor += run.len();
                    }
                }
                c if c.is_ascii_digit() => {
                    let digits = self.take_run(|c| c.is_ascii_digit());
                    let end = self.cursor;
                    append_expression(
                        &mut items,
                        MathExpression::Number { text: digits.to_string(), span: span_from(0, start, end) },
                    );
                }
                _ => {
                    self.cursor += c.len_utf8();
                    append_expression(&mut items, char_expression(c, span_from(0, start, self.cursor)));
                }
            }
        }
        sequence(items)
    }

    /// The offset just past a dotted symbol path such as `arrow.r`.
    fn dotted_path_end(&self) -> usize {
        let rest = &self.body[self.cursor..];
        let end = rest
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-')))
            .map(|(offset, _)| offset)
            .unwrap_or(rest.len());
        self.cursor + end
    }

    /// One script operand: a parenthesised group, a digit run, a named
    /// symbol, a single letter, or a single operator character.
    fn script_operand(&mut self) -> Option<MathExpression> {
        self.skip_space();
        let start = self.cursor;
        let c = self.peek()?;
        match c {
            '^' | '_' | '#' | '\\' | '[' | ']' | '{' | '}' | '$' => None,
            '(' => {
                let inner_start = start + 1;
                let close = self.matching(start, ')')?;
                self.cursor = inner_start;
                let inner = self.lower_until(Some(')'));
                self.cursor = close + 1;
                // The TeX profile wraps a braced script body in a semantic
                // group; the Typst parenthesised operand is the same construct.
                Some(MathExpression::Group { body: Box::new(inner) })
            }
            c if c.is_ascii_digit() => {
                let digits = self.take_run(|c| c.is_ascii_digit());
                Some(MathExpression::Number { text: digits.to_string(), span: span_from(0, start, self.cursor) })
            }
            c if c.is_ascii_alphabetic() => {
                let run = self.letter_run();
                if let Some((text, class)) = symbol_for(run) {
                    self.cursor += run.len();
                    return Some(MathExpression::Atom(MathAtom {
                        text: text.to_string(),
                        class,
                        span: span_from(0, start, self.cursor),
                    }));
                }
                if self.body[self.cursor + run.len()..].starts_with('.') {
                    let end = self.dotted_path_end();
                    self.cursor = end;
                    self.push_loss("symbol", start, end);
                    return Some(unsupported(start, end, self.body, "symbol", 0));
                }
                // Only the first letter is the operand; the rest are siblings.
                self.cursor = start + c.len_utf8();
                Some(char_expression(c, span_from(0, start, self.cursor)))
            }
            _ => {
                self.cursor += c.len_utf8();
                Some(char_expression(c, span_from(0, start, self.cursor)))
            }
        }
    }
}
