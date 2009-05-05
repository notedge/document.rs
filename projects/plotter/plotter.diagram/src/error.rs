//! Structured diagram parsing and lowering failures.

use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    ops::Range,
};

use plotter_types::{
    Diagnostic, DiagnosticLabel, DiagnosticLocation, DiagnosticSeverity, LabelRole, Message, NAMESPACE, SourceRef, codes,
    diagnostic,
};

/// A fatal diagram parsing or lowering failure.
///
/// The small code-and-message surface mirrors [`plotter_scene::SceneError`].
/// The full structured record is available through
/// [`DiagramError::diagnostic`]. Frontends attach the offending byte range with
/// [`DiagramError::with_span`] so rejections name the exact source text
/// instead of only reporting that something is unsupported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramError {
    code: &'static str,
    message: String,
    span: Option<Range<usize>>,
    diagnostic: Box<Diagnostic>,
}

impl DiagramError {
    /// Create an error from a stable diagnostic code and message.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        let diagnostic = diagnostic("diagram", code, DiagnosticSeverity::Error, message.clone());
        Self { code, message, span: None, diagnostic: Box::new(diagnostic) }
    }

    /// Attach the offending byte range.
    pub fn with_span(mut self, span: Range<usize>) -> Self {
        if span.start < span.end {
            self.span = Some(span);
        }
        self
    }

    /// Attach the offending byte range when the frontend could locate one.
    pub fn with_optional_span(self, span: Option<Range<usize>>) -> Self {
        match span {
            Some(span) => self.with_span(span),
            None => self,
        }
    }

    /// The offending byte range, when the frontend could identify one.
    pub fn span(&self) -> Option<Range<usize>> {
        self.span.clone()
    }

    /// The stable `davinci.*` diagnostic code.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The human-readable failure description.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The structured diagnostic record for this failure.
    pub fn diagnostic(&self) -> &Diagnostic {
        &self.diagnostic
    }

    /// The structured diagnostic with a primary text label covering
    /// [`DiagramError::span`].
    ///
    /// Without a span this returns the unlabelled record. The source name is
    /// the frontend family, so a DOT rejection points at `dot` and a Mermaid
    /// rejection at `mermaid`.
    pub fn labelled(&self, source: &str) -> Diagnostic {
        let Some(span) = &self.span
        else {
            return (*self.diagnostic).clone();
        };
        let Ok(source_ref) = SourceRef::new(NAMESPACE, source)
        else {
            return (*self.diagnostic).clone();
        };
        let Ok(range) = plotter_types::ByteRange::new(span.start as u64, span.end as u64)
        else {
            return (*self.diagnostic).clone();
        };
        (*self.diagnostic).clone().with_primary(DiagnosticLabel::new(
            DiagnosticLocation::Text { source: source_ref, range },
            Message::new(self.code).with_fallback(self.message.clone()),
            LabelRole::Primary,
        ))
    }

    /// 解析失败。
    pub fn parse_failed(message: impl Into<String>) -> Self {
        Self::new(codes::PARSE_FAILED, message)
    }

    /// AST 不可用或为空。
    pub fn ast_unavailable(message: impl Into<String>) -> Self {
        Self::new(codes::AST_UNAVAILABLE, message)
    }

    /// 语法可解析但不支持的构造。
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(codes::UNSUPPORTED, message)
    }

    /// 图为空（无节点 / 无边等）。
    pub fn empty_diagram(message: impl Into<String>) -> Self {
        Self::new(codes::EMPTY_DIAGRAM, message)
    }
}

impl Display for DiagramError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for DiagramError {}
