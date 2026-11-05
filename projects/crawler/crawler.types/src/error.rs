use std::fmt;

use boost_diagnostic::{Diagnostic, MessageArg};

/// Crawler orchestration error — structured diagnostic with stable code,
/// typed args, cause chain, and origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrawlError {
    /// Structured diagnostic — the only error variant.
    Diagnostic(Diagnostic),
}

impl CrawlError {
    /// Extract the stable diagnostic code.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Diagnostic(d) => Some(d.code().as_str()),
        }
    }

    /// Extract the diagnostic message key.
    pub fn message_key(&self) -> Option<&str> {
        match self {
            Self::Diagnostic(d) => Some(d.message().key()),
        }
    }

    /// Read a text argument attached to the diagnostic message.
    pub fn text_arg(&self, name: &str) -> Option<&str> {
        match self {
            Self::Diagnostic(d) => {
                d.message().args().iter().find_map(|(key, value)| (key.as_str() == name).then_some(value)).and_then(|value| {
                    match value {
                        MessageArg::Text(text) => Some(text.as_str()),
                        _ => None,
                    }
                })
            }
        }
    }
}

impl fmt::Display for CrawlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diagnostic(d) => {
                if let Some(fallback) = d.message().fallback() {
                    write!(f, "{}", fallback)
                }
                else {
                    write!(f, "{}/{}", d.origin().namespace(), d.code())
                }
            }
        }
    }
}

impl std::error::Error for CrawlError {}

impl From<Diagnostic> for CrawlError {
    fn from(value: Diagnostic) -> Self {
        Self::Diagnostic(value)
    }
}

/// Crawler result alias.
pub type Result<T> = std::result::Result<T, CrawlError>;
