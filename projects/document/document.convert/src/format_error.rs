use document_formats::{FormatError, from_format_error};
use document_types::{AdapterError, Result};

pub(crate) fn fail<T>(error: AdapterError) -> Result<T> {
    document_diagnostic::log_adapter_error(&error);
    Err(error)
}

pub(crate) fn propagate<T>(result: Result<T>) -> Result<T> {
    if let Err(error) = &result {
        document_diagnostic::log_adapter_error(error);
    }
    result
}

/// Convert a Notedown [`FormatError`] into an [`AdapterError`] while preserving
/// the structured diagnostic code, typed args, and origin from the formats
/// layer. This avoids the previous pattern of stringifying the error into a
/// free-form message and losing the stable wire identifiers.
pub(crate) fn map_format_error(error: FormatError) -> AdapterError {
    let diagnostic = from_format_error(&error);
    AdapterError::diagnostic(diagnostic)
}
