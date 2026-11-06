//! HTML character-encoding detection for fetched bodies.
//!
//! The extractor owns the decoding step so `notedown-formats` always receives
//! UTF-8 bytes. Precedence follows the HTML standard's transport-level order:
//! BOM, `Content-Type` charset, `<meta>` charset, then UTF-8.

use crawler_types::FetchArtifact;
use encoding_rs::{Encoding, UTF_8};

/// Why an HTML body could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DecodeError {
    /// The artifact has no inline body bytes.
    MissingBody,
    /// The selected encoding reported malformed byte sequences.
    Malformed {
        /// Encoding label that was applied.
        encoding: &'static str,
    },
}

impl DecodeError {
    /// Human-readable message used in the extract loss marker.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::MissingBody => "HTML body bytes are not inline and cannot be decoded".to_string(),
            Self::Malformed { encoding } => format!("HTML body contains malformed `{encoding}` byte sequences"),
        }
    }
}

/// Decode an HTML artifact into UTF-8 text using standard charset precedence.
pub(crate) fn decode_html(artifact: &FetchArtifact) -> Result<String, DecodeError> {
    let bytes = artifact.body_bytes().ok_or(DecodeError::MissingBody)?;
    decode_bytes(bytes, artifact.content_type.as_deref())
}

fn decode_bytes(bytes: &[u8], content_type: Option<&str>) -> Result<String, DecodeError> {
    if let Some((encoding, bom_len)) = Encoding::for_bom(bytes) {
        return decode_with(encoding, &bytes[bom_len..]);
    }
    let encoding = content_type
        .and_then(charset_from_content_type)
        .and_then(|label| Encoding::for_label(label.as_bytes()))
        .or_else(|| meta_charset(bytes).and_then(|label| Encoding::for_label(label.as_bytes())))
        .unwrap_or(UTF_8);
    decode_with(encoding, bytes)
}

fn decode_with(encoding: &'static Encoding, bytes: &[u8]) -> Result<String, DecodeError> {
    let (text, had_errors) = encoding.decode_without_bom_handling(bytes);
    if had_errors {
        return Err(DecodeError::Malformed { encoding: encoding.name() });
    }
    Ok(text.into_owned())
}

/// Extract a `charset` parameter from a `Content-Type` value.
fn charset_from_content_type(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let after = lower.split_once("charset")?.1;
    parameter_value(after, &[';', ','])
}

/// Scan the first 1024 bytes for a `<meta>` charset declaration.
fn meta_charset(bytes: &[u8]) -> Option<String> {
    let window = &bytes[..bytes.len().min(1024)];
    let lower = String::from_utf8_lossy(window).to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(offset) = lower[cursor..].find("<meta") {
        let start = cursor + offset;
        let Some(end) = lower[start..].find('>')
        else {
            break;
        };
        let tag = &lower[start..start + end];
        if let Some(label) = charset_attribute(tag) {
            return Some(label);
        }
        cursor = start + end + 1;
    }
    None
}

fn charset_attribute(tag: &str) -> Option<String> {
    parameter_value(tag.split_once("charset")?.1, &['>', '/'])
}

/// Parse the value after a `charset` token.
///
/// Accepts an optional opening quote and stops at a closing quote, a listed
/// terminator, or ASCII whitespace. The caller passes ASCII-lowercased input.
fn parameter_value(after: &str, terminators: &[char]) -> Option<String> {
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    let after = after.strip_prefix(['"', '\'']).unwrap_or(after);
    let end = after
        .find(|ch: char| ch == '"' || ch == '\'' || ch.is_ascii_whitespace() || terminators.contains(&ch))
        .unwrap_or(after.len());
    let label = after[..end].trim_matches(|ch| ch == '"' || ch == '\'');
    (!label.is_empty()).then(|| label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_overrides_content_type_and_meta() {
        let bytes = b"\xEF\xBB\xBF<html><head><meta charset=\"iso-8859-1\"></head></html>";
        let text = decode_bytes(bytes, Some("text/html; charset=iso-8859-1")).expect("utf8 bom wins");
        assert!(text.starts_with("<html>"));
    }

    #[test]
    fn content_type_charset_overrides_meta() {
        let html: &[u8] = b"<html><head><meta charset=\"utf-8\"></head><body>caf\xE9</body></html>";
        let text = decode_bytes(html, Some("text/html; charset=windows-1252")).expect("transport charset wins");
        assert!(text.contains("café"), "{text}");
        assert_eq!(charset_from_content_type("text/html; charset=ISO-8859-1"), Some("iso-8859-1".into()));
    }

    #[test]
    fn meta_charset_is_used_without_transport_charset() {
        let html: &[u8] = b"<html><head><meta charset=\"windows-1252\"></head><body>caf\xE9</body></html>";
        let text = decode_bytes(html, Some("text/html")).expect("meta decode");
        assert!(text.contains("café"), "{text}");
    }

    #[test]
    fn defaults_to_utf8_and_rejects_malformed_bytes() {
        let text = decode_bytes("<html><body>plain</body></html>".as_bytes(), None).expect("default utf8");
        assert!(text.contains("plain"));
        let error = decode_bytes(b"<html>\xFF\x80</html>", None).expect_err("invalid utf8");
        assert!(matches!(error, DecodeError::Malformed { encoding: "UTF-8" }));
    }
}
