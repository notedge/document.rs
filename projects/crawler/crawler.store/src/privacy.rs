//! Secret-material guards for browser persistence.
//!
//! Browser archives may contain rendered HTML, network traces and screenshots.
//! They must never carry cookie jars, authorization headers or session tokens,
//! and a persisted session may only reference an external vault handle. These
//! guards run before any write so a leak fails closed instead of landing.

use crate::{
    error::{Result, StoreError},
    types::{BrowserSessionPersistence, BrowserSessionRecord},
};

/// Credential-bearing header or field markers rejected in browser archives.
const ARCHIVE_MARKERS: &[&str] =
    &["set-cookie:", "cookie:", "authorization:", "proxy-authorization:", "www-authenticate:", "bearer "];

/// Words that make a vault handle look like an inlined secret.
const SECRET_WORDS: &[&str] = &["cookie", "authorization", "bearer", "password", "passwd", "secret", "token"];

/// Reject an archive that still carries credential plaintext.
pub(crate) fn ensure_archive_is_redacted(archive: &[u8]) -> Result<()> {
    for marker in ARCHIVE_MARKERS {
        if contains_ascii_case_insensitive(archive, marker) {
            return Err(StoreError::StoreInvalidState(format!(
                "credential plaintext is not allowed in a browser archive: found `{marker}`"
            )));
        }
    }
    Ok(())
}

/// Reject a vault reference that is actually an inlined secret.
pub(crate) fn ensure_vault_ref_is_opaque(reference: &str) -> Result<()> {
    if reference.trim().is_empty() {
        return Err(StoreError::StoreInvalidState("vault session reference must not be empty".into()));
    }
    if reference.chars().any(char::is_whitespace) || reference.contains('=') {
        return Err(StoreError::StoreInvalidState(
            "vault session reference must be an opaque handle rather than an inlined cookie or token".into(),
        ));
    }
    let lower = reference.to_ascii_lowercase();
    if let Some(word) = SECRET_WORDS.iter().find(|word| lower.contains(*word)) {
        return Err(StoreError::StoreInvalidState(format!(
            "vault session reference must be an opaque handle but contains `{word}`"
        )));
    }
    Ok(())
}

/// Validate that a persisted session only keeps an external vault handle.
pub(crate) fn ensure_session_record(record: &BrowserSessionRecord) -> Result<()> {
    match (&record.persistence, &record.vault_session_ref) {
        (BrowserSessionPersistence::ExternalVault, Some(reference)) => ensure_vault_ref_is_opaque(reference),
        (BrowserSessionPersistence::ExternalVault, None) => {
            Err(StoreError::StoreInvalidState("external vault persistence requires a vault session reference".into()))
        }
        (_, Some(_)) => {
            Err(StoreError::StoreInvalidState("only external vault persistence may store a vault session reference".into()))
        }
        (_, None) => Ok(()),
    }
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &str) -> bool {
    let needle = needle.as_bytes();
    haystack.windows(needle.len()).any(|window| window.eq_ignore_ascii_case(needle))
}
