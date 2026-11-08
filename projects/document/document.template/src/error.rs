use std::fmt::{Display, Formatter, Result as FmtResult};

use boost_diagnostic::{Diagnostic, DiagnosticCode, DiagnosticOrigin, DiagnosticSeverity, Message, MessageArg};

/// Producer namespace for template diagnostics.
pub const NAMESPACE: &str = "document";

/// Producer component for template diagnostics.
pub const COMPONENT: &str = "template";

/// Build the diagnostic origin used by every template failure.
pub fn origin() -> DiagnosticOrigin {
    DiagnosticOrigin::new(NAMESPACE, COMPONENT)
}

/// One failed template render.
///
/// The first diagnostic is the primary failure; its code is the stable machine
/// identifier callers switch on. Arguments travel as typed `args`, so callers
/// never parse the human-readable fallback text.
#[derive(Debug, Clone)]
pub struct TemplateError {
    diagnostics: Vec<Diagnostic>,
}

impl TemplateError {
    /// Build a single-diagnostic failure with no structured arguments.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self::with_args(code, message, Vec::new())
    }

    /// Build a single-diagnostic failure with text arguments.
    ///
    /// Arguments keep stable names (`ref`, `from`, `engine`, `construct`, …) and
    /// become `args` on the wire diagnostic.
    pub fn with_args(code: &'static str, message: impl Into<String>, args: Vec<(&'static str, String)>) -> Self {
        let fallback = message.into();
        let mut payload = Message::new(code);
        for (name, value) in args {
            payload = payload.with_arg(name, MessageArg::Text(value));
        }
        let diagnostic =
            Diagnostic::new(DiagnosticCode::new(code), DiagnosticSeverity::Error, origin(), payload.with_fallback(fallback));
        Self { diagnostics: vec![diagnostic] }
    }

    /// Stable code of the primary failure.
    #[must_use]
    pub fn code(&self) -> &str {
        self.diagnostics.first().map_or("", |diagnostic| diagnostic.code().as_str())
    }

    /// All diagnostics that belong to this failure.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Consume the failure and return its diagnostics.
    #[must_use]
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

impl Display for TemplateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for TemplateError {}
