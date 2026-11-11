use std::fmt::{Display, Formatter, Result as FmtResult};

use boost_diagnostic::{Diagnostic, DiagnosticCode, DiagnosticOrigin, DiagnosticSeverity, Message, MessageArg};

/// Producer namespace for template diagnostics.
pub const NAMESPACE: &str = "template";

/// Producer component for template diagnostics.
pub const COMPONENT: &str = "core";

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
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::with_args(code, message, Vec::new())
    }

    /// Build a single-diagnostic failure with text arguments.
    ///
    /// Arguments keep stable names (`ref`, `from`, `engine`, `construct`, …) and
    /// become `args` on the wire diagnostic. Engine crates whose code set is
    /// declared elsewhere map their own stable codes in here unchanged.
    pub fn with_args(code: impl Into<String>, message: impl Into<String>, args: Vec<(&'static str, String)>) -> Self {
        let owned = args.into_iter().map(|(name, value)| (name.to_string(), value)).collect();
        Self::with_owned_args(code, message, owned)
    }

    /// Build a single-diagnostic failure with owned argument names.
    ///
    /// Engine crates that translate their own diagnostic maps need dynamic names;
    /// this is the same shape as [`TemplateError::with_args`] without the
    /// `'static` restriction.
    pub fn with_owned_args(code: impl Into<String>, message: impl Into<String>, args: Vec<(String, String)>) -> Self {
        let code: String = code.into();
        let fallback = message.into();
        let mut payload = Message::new(code.clone());
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
