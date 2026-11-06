//! Byte-level format detection used by the input orchestration layer.
//!
//! This module only reads container headers and the leading bytes of a source.
//! It never decodes a document and never runs a parser, so a detection result
//! states what the bytes prove, not what they are expected to contain.
//!
//! The rule is that every claim carries evidence and an unresolved container
//! stays ambiguous. A ZIP stream cannot say whether it is an OPC Word package
//! or an OCF ebook package before the package parts are read, so the probe
//! returns both candidates instead of guessing from a filename.

/// Container family observed from the leading bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    /// No container header: the source is plain text or an unqualified stream.
    Plain,
    /// ZIP container (`PK\x03\x04`), shared by DOCX and EPUB.
    Zip,
    /// OLE compound file (`D0 CF 11 E0 A1 B1 1A E1`), used by legacy DOC.
    OleCompound,
    /// PDF header (`%PDF-`).
    Pdf,
}

/// How strongly the evidence supports the reported format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionConfidence {
    /// The bytes alone identify the format family without a parser.
    Verified,
    /// A content signature or an explicit extension hint selected one candidate.
    Hinted,
    /// The bytes name more than one candidate and no hint resolved it.
    Ambiguous,
    /// No candidate could be named.
    Unknown,
}

/// Format probe result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatDetection {
    /// Resolved format id when the evidence selects exactly one candidate.
    pub format: Option<&'static str>,
    /// Candidate ids in preference order. Empty only when nothing was named.
    pub candidates: Vec<&'static str>,
    /// Container family observed from the leading bytes.
    pub container: ContainerKind,
    /// Strength of the evidence behind `format`.
    pub confidence: DetectionConfidence,
    /// Human-readable evidence, kept stable for reports and tests.
    pub evidence: String,
}

impl FormatDetection {
    fn resolved(
        format: &'static str,
        container: ContainerKind,
        confidence: DetectionConfidence,
        evidence: impl Into<String>,
    ) -> Self {
        Self { format: Some(format), candidates: vec![format], container, confidence, evidence: evidence.into() }
    }

    fn ambiguous(candidates: Vec<&'static str>, container: ContainerKind, evidence: impl Into<String>) -> Self {
        Self { format: None, candidates, container, confidence: DetectionConfidence::Ambiguous, evidence: evidence.into() }
    }

    fn unknown(container: ContainerKind, evidence: impl Into<String>) -> Self {
        Self {
            format: None,
            candidates: Vec::new(),
            container,
            confidence: DetectionConfidence::Unknown,
            evidence: evidence.into(),
        }
    }
}

/// Detects one document format from bytes and an optional extension hint.
///
/// Content evidence outranks an extension hint: a `.md` file whose first
/// non-blank bytes open an `<html>` element is reported as HTML, and the
/// evidence records the override. An extension hint only resolves formats the
/// bytes cannot separate, such as DOCX and EPUB inside a ZIP container.
#[must_use]
pub fn detect_bytes(bytes: &[u8], extension_hint: Option<&str>) -> FormatDetection {
    if bytes.is_empty() {
        return FormatDetection::unknown(ContainerKind::Plain, "empty input carries no container header or format signature");
    }
    if bytes.starts_with(b"%PDF-") {
        return FormatDetection::resolved(
            "pdf",
            ContainerKind::Pdf,
            DetectionConfidence::Verified,
            "header `%PDF-` identifies a PDF stream",
        );
    }
    if looks_like_ole(bytes) {
        return FormatDetection::resolved(
            "doc",
            ContainerKind::OleCompound,
            DetectionConfidence::Verified,
            "OLE compound header identifies a legacy Word document",
        );
    }
    if looks_like_zip(bytes) {
        return detect_zip(extension_hint);
    }
    detect_text(bytes, extension_hint)
}

fn detect_zip(extension_hint: Option<&str>) -> FormatDetection {
    match normalize_hint(extension_hint) {
        Some("docx") => FormatDetection::resolved(
            "docx",
            ContainerKind::Zip,
            DetectionConfidence::Hinted,
            "ZIP container plus a `.docx` extension hint selects the OPC Word package",
        ),
        Some("epub") => FormatDetection::resolved(
            "epub",
            ContainerKind::Zip,
            DetectionConfidence::Hinted,
            "ZIP container plus an `.epub` extension hint selects the OCF ebook package",
        ),
        Some(other) => FormatDetection::ambiguous(
            vec!["docx", "epub"],
            ContainerKind::Zip,
            format!("ZIP container cannot be separated by the `{other}` extension hint, so DOCX and EPUB both remain"),
        ),
        None => FormatDetection::ambiguous(
            vec!["docx", "epub"],
            ContainerKind::Zip,
            "ZIP container carries either an OPC Word package or an OCF ebook package, so both candidates remain",
        ),
    }
}

fn detect_text(bytes: &[u8], extension_hint: Option<&str>) -> FormatDetection {
    if let Some(format) = textual_signature(bytes) {
        return FormatDetection::resolved(
            format,
            ContainerKind::Plain,
            DetectionConfidence::Hinted,
            describe_text_signature(format),
        );
    }
    if let Some(format) = normalize_hint(extension_hint) {
        if matches!(format, "markdown" | "notedown" | "html" | "tex" | "typst") {
            return FormatDetection::resolved(
                format,
                ContainerKind::Plain,
                DetectionConfidence::Hinted,
                format!("plain text with a `.{format}` extension hint and no stronger content signature"),
            );
        }
    }
    FormatDetection::unknown(ContainerKind::Plain, "plain text has no container header and no recognizable format signature")
}

fn textual_signature(bytes: &[u8]) -> Option<&'static str> {
    let head = leading_text(bytes, 64);
    let trimmed = head.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("<!doctype html")
        || lower.starts_with("<html")
        || lower.starts_with("<head")
        || lower.starts_with("<body")
    {
        return Some("html");
    }
    if trimmed.starts_with("\\documentclass") || trimmed.starts_with("\\begin{document") || trimmed.starts_with("\\section{") {
        return Some("tex");
    }
    None
}

fn leading_text(bytes: &[u8], limit: usize) -> String {
    let end = bytes.len().min(limit);
    let slice = &bytes[..end];
    let valid = slice.iter().position(|byte| *byte >= 0x80).unwrap_or(slice.len());
    String::from_utf8_lossy(&slice[..valid]).into_owned()
}

fn describe_text_signature(format: &'static str) -> &'static str {
    match format {
        "html" => "leading `<html`/`<head`/`<body`/`<!doctype html` signature identifies HTML text",
        "tex" => "leading `\\documentclass`/`\\begin{document`/`\\section{` signature identifies TeX source",
        _ => "text signature identifies the format",
    }
}

fn normalize_hint(extension_hint: Option<&str>) -> Option<&'static str> {
    let hint = extension_hint?.trim().trim_start_matches('.').to_ascii_lowercase();
    let format = match hint.as_str() {
        "md" | "markdown" | "mdown" | "mkd" => "markdown",
        "nd" | "notedown" => "notedown",
        "html" | "htm" | "xhtml" => "html",
        "tex" | "latex" => "tex",
        "typ" | "typst" => "typst",
        "docx" => "docx",
        "doc" => "doc",
        "epub" => "epub",
        "pdf" => "pdf",
        _ => return None,
    };
    Some(format)
}

fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06")
}

fn looks_like_ole(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
}

#[cfg(test)]
mod tests {
    use super::{ContainerKind, DetectionConfidence, detect_bytes};

    #[test]
    fn pdf_header_is_verified() {
        let detection = detect_bytes(b"%PDF-1.4\n", None);
        assert_eq!(detection.format, Some("pdf"));
        assert_eq!(detection.container, ContainerKind::Pdf);
        assert_eq!(detection.confidence, DetectionConfidence::Verified);
    }

    #[test]
    fn ole_header_is_a_verified_doc() {
        let detection = detect_bytes(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0], None);
        assert_eq!(detection.format, Some("doc"));
        assert_eq!(detection.container, ContainerKind::OleCompound);
    }

    #[test]
    fn zip_without_a_hint_keeps_both_package_candidates() {
        let detection = detect_bytes(b"PK\x03\x04docx-or-epub", None);
        assert_eq!(detection.format, None);
        assert_eq!(detection.candidates, vec!["docx", "epub"]);
        assert_eq!(detection.confidence, DetectionConfidence::Ambiguous);
    }

    #[test]
    fn zip_hint_resolves_the_package_candidate() {
        assert_eq!(detect_bytes(b"PK\x03\x04", Some("docx")).format, Some("docx"));
        assert_eq!(detect_bytes(b"PK\x03\x04", Some(".epub")).format, Some("epub"));
    }

    #[test]
    fn html_signature_overrides_a_markdown_extension() {
        let detection = detect_bytes(b"  <!doctype html><html></html>", Some("md"));
        assert_eq!(detection.format, Some("html"));
        assert!(detection.evidence.contains("html"));
    }

    #[test]
    fn extension_hint_selects_plain_text_format() {
        assert_eq!(detect_bytes(b"# Title\n\nBody.\n", Some("md")).format, Some("markdown"));
        assert_eq!(detect_bytes(b"= Title\n", Some("typ")).format, Some("typst"));
    }

    #[test]
    fn text_without_a_signature_or_hint_is_unknown() {
        let detection = detect_bytes(b"just words", None);
        assert_eq!(detection.format, None);
        assert!(detection.candidates.is_empty());
        assert_eq!(detection.confidence, DetectionConfidence::Unknown);
    }

    #[test]
    fn empty_input_is_unknown() {
        let detection = detect_bytes(b"", Some("md"));
        assert_eq!(detection.format, None);
        assert_eq!(detection.confidence, DetectionConfidence::Unknown);
    }
}
