use std::fmt;

use boost_diagnostic::{Diagnostic, DiagnosticSeverity, Message, MessageArg};
use crawler_types::diagnostics as pd;

/// Stable store-level errors exposed to Crawler core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Lease token expired or was reclaimed.
    StoreLeaseExpired,
    /// Claim token does not match the active frontier claim.
    StoreFencingMismatch,
    /// Compare-and-set or state conflict.
    StoreConflict,
    /// Namespace quota exceeded.
    StoreQuotaExceeded,
    /// Unrecoverable inconsistency.
    StoreCorrupt(String),
    /// Run or resource not found.
    StoreNotFound(String),
    /// Invalid operation for the current store state.
    StoreInvalidState(String),
}

impl StoreError {
    /// Project this store error into a structured [`Diagnostic`] using the
    /// stable `crawler.store.*` codes frozen in [`crawler_types::diagnostics`].
    ///
    /// Free-form string fields are carried through typed args rather than
    /// embedded directly in the code or origin, so downstream callers can
    /// react on stable identifiers without parsing human text.
    pub fn to_diagnostic(&self) -> Diagnostic {
        let (code, fallback) = match self {
            Self::StoreLeaseExpired => (pd::STORE_CONFLICT, "lease expired"),
            Self::StoreFencingMismatch => (pd::STORE_CONFLICT, "fencing mismatch"),
            Self::StoreConflict => (pd::STORE_CONFLICT, "transaction conflict"),
            Self::StoreQuotaExceeded => (pd::STORE_CONFLICT, "store quota exceeded"),
            Self::StoreCorrupt(message) => (pd::STORE_CORRUPT, message.as_str()),
            Self::StoreNotFound(message) => (pd::STORE_UNAVAILABLE, message.as_str()),
            Self::StoreInvalidState(message) => (pd::STORE_TRANSACTION_FAILED, message.as_str()),
        };
        let key = diagnostic_key_for(code);
        let mut message = Message::new(&format!("crawler.diagnostic.store.{key}"));
        if let Self::StoreCorrupt(_) | Self::StoreNotFound(_) | Self::StoreInvalidState(_) = self {
            // carry the free-form message as a typed arg for structured
            // consumers while also providing a human-readable fallback.
            let raw = match self {
                Self::StoreCorrupt(s) | Self::StoreNotFound(s) | Self::StoreInvalidState(s) => s.as_str(),
                _ => "",
            };
            message = message.with_arg("message", MessageArg::Text(raw.to_string()));
        }
        let fallback_owned = fallback.to_string();
        Diagnostic::new(pd::code(code), DiagnosticSeverity::Error, pd::origin_store(), message.with_fallback(fallback_owned))
    }
}

/// Map a stable store code back to a short message-id suffix for locale keys.
fn diagnostic_key_for(code: &'static str) -> &'static str {
    match code {
        pd::STORE_CONFLICT => "storeConflict",
        pd::STORE_CORRUPT => "storeCorrupt",
        pd::STORE_UNAVAILABLE => "storeUnavailable",
        pd::STORE_TRANSACTION_FAILED => "storeTransactionFailed",
        pd::STORE_RECOVERY_FAILED => "storeRecoveryFailed",
        _ => "storeError",
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StoreLeaseExpired => write!(f, "StoreLeaseExpired"),
            Self::StoreFencingMismatch => write!(f, "StoreFencingMismatch"),
            Self::StoreConflict => write!(f, "StoreConflict"),
            Self::StoreQuotaExceeded => write!(f, "StoreQuotaExceeded"),
            Self::StoreCorrupt(message) => write!(f, "StoreCorrupt: {message}"),
            Self::StoreNotFound(message) => write!(f, "StoreNotFound: {message}"),
            Self::StoreInvalidState(message) => write!(f, "StoreInvalidState: {message}"),
        }
    }
}

impl std::error::Error for StoreError {}

/// Convenient blanket conversion so core and frontend callers can
/// seamlessly turn any store error into a structured diagnostic
/// without having to know about `to_diagnostic`.
impl From<StoreError> for Diagnostic {
    fn from(value: StoreError) -> Self {
        value.to_diagnostic()
    }
}

/// Convenient result alias for store operations.
pub type Result<T> = std::result::Result<T, StoreError>;

#[cfg(test)]
mod tests {
    use super::*;
    use boost_diagnostic::DiagnosticCode;

    fn extract_code(error: &StoreError) -> String {
        let diag = error.to_diagnostic();
        diag.code().to_string()
    }

    #[test]
    fn variant_to_stable_code_mapping() {
        assert_eq!(extract_code(&StoreError::StoreConflict), pd::STORE_CONFLICT);
        assert_eq!(extract_code(&StoreError::StoreFencingMismatch), pd::STORE_CONFLICT);
        assert_eq!(extract_code(&StoreError::StoreLeaseExpired), pd::STORE_CONFLICT);
        assert_eq!(extract_code(&StoreError::StoreQuotaExceeded), pd::STORE_CONFLICT);
        assert_eq!(extract_code(&StoreError::StoreCorrupt("CRC mismatch".into())), pd::STORE_CORRUPT,);
        assert_eq!(extract_code(&StoreError::StoreInvalidState("closed connection".into())), pd::STORE_TRANSACTION_FAILED,);
    }

    #[test]
    fn corrupt_carries_message_as_typed_arg() {
        let diag = StoreError::StoreCorrupt("CRC mismatch".into()).to_diagnostic();
        assert_eq!(diag.origin().namespace(), "crawler");
        assert_eq!(diag.origin().component(), "store");
        // code is the stable dotted identifier, unchanged by refactors
        assert_eq!(diag.code(), &DiagnosticCode::new(pd::STORE_CORRUPT));
        // args include the free-form reason under a stable name
        let has_message = diag.message().args().iter().any(|(name, _)| name == "message");
        assert!(has_message, "StoreCorrupt must carry 'message' arg");
    }

    #[test]
    fn variant_without_string_still_has_fallback() {
        let diag = StoreError::StoreConflict.to_diagnostic();
        assert!(diag.message().fallback().is_some());
        assert_eq!(diag.code(), &DiagnosticCode::new(pd::STORE_CONFLICT));
    }
}
