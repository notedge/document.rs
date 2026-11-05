#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

mod report;

use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    path::Path,
};

use document_compose::{ConversionReport, Diagnostic, Severity};
use document_formats::detect::detect_bytes;
pub use document_formats::detect::{ContainerKind, DetectionConfidence, FormatDetection};
use document_formats::import::{
    doc::import_doc_bytes, docx::import_docx_bytes, epub::import_epub_bytes, html::import_html_bytes, pdf::import_pdf_bytes,
};
use document_ir::DocumentGraph;

/// Stable diagnostic codes emitted by this profile.
pub mod codes {
    /// No container header or content signature named a format, so the source
    /// cannot be routed without an explicit format.
    pub const UNKNOWN_FORMAT: &str = "vutex.input.formats.unknown_format";
    /// The bytes name several candidates and no extension hint resolved them.
    pub const AMBIGUOUS_FORMAT: &str = "vutex.input.formats.ambiguous_format";
    /// The bytes resolve to a format this profile does not import.
    pub const UNSUPPORTED_FORMAT: &str = "vutex.input.formats.unsupported_format";
    /// Declared format and the probed bytes disagree.
    pub const FORMAT_MISMATCH: &str = "vutex.input.formats.format_mismatch";
    /// The source file could not be read.
    pub const FILE_READ_FAILED: &str = "vutex.input.formats.file_read_failed";
    /// The source is larger than the profile's byte budget.
    pub const SOURCE_TOO_LARGE: &str = "vutex.input.formats.source_too_large";
    /// The underlying format reader rejected the source.
    pub const IMPORT_FAILED: &str = "vutex.input.formats.import_failed";
    /// The imported graph violates a `notedown-ir` structural invariant.
    pub const INVALID_GRAPH: &str = "vutex.input.formats.invalid_graph";
    /// A block was retained as an opaque boundary instead of being lowered.
    pub const OPAQUE_BLOCK: &str = "vutex.input.formats.opaque_block";
}

/// Largest source, in bytes, this profile accepts.
///
/// The same budget is used by the Markdown, TeX, and Typst profiles so a host
/// can enforce one input limit across every entry point.
pub const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;

/// Container document formats this profile imports.
///
/// `markdown`, `tex`, and `typst` stay in their own language profiles. The
/// internal `notedown` semantic model is not an external import format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// HTML text imported into semantic blocks.
    Html,
    /// OPC WordprocessingML package.
    Docx,
    /// OCF EPUB package.
    Epub,
    /// PDF stream, text level only.
    Pdf,
    /// Legacy OLE Word document, extracted text only.
    Doc,
}

impl Format {
    /// Formats this profile imports.
    pub const ALL: &'static [Self] = &[Self::Html, Self::Docx, Self::Epub, Self::Pdf, Self::Doc];

    /// Stable format id used by reports and diagnostics.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Docx => "docx",
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Doc => "doc",
        }
    }

    /// Extension used to resolve package candidates inside a shared container.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Docx => "docx",
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Doc => "doc",
        }
    }

    /// Resolves a stable format id handled by this profile.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "html" => Some(Self::Html),
            "docx" => Some(Self::Docx),
            "epub" => Some(Self::Epub),
            "pdf" => Some(Self::Pdf),
            "doc" => Some(Self::Doc),
            _ => None,
        }
    }
}

/// Outcome of a container document import: graph, report, diagnostics, and the
/// probe that routed the source.
#[derive(Debug)]
pub struct ImportedDocument {
    /// Format selected for the import.
    pub format: Format,
    /// Probe result that routed the source.
    pub detection: FormatDetection,
    /// The imported semantic graph consumed by the layout stage.
    pub graph: DocumentGraph,
    /// Normalized coverage and loss report for the import.
    pub report: ConversionReport,
    /// Diagnostics collected while importing.
    pub diagnostics: Vec<Diagnostic>,
}

/// Why a container document could not be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError {
    diagnostics: Vec<Diagnostic>,
}

impl InputError {
    /// Stable diagnostic code of the first failure.
    #[must_use]
    pub fn code(&self) -> &str {
        self.diagnostics.first().map_or("", |diagnostic| diagnostic.code.as_str())
    }

    /// Human-readable message of the first failure.
    #[must_use]
    pub fn message(&self) -> &str {
        self.diagnostics.first().map_or("", |diagnostic| diagnostic.message.as_str())
    }

    /// All diagnostics associated with the failure.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

impl From<Diagnostic> for InputError {
    fn from(diagnostic: Diagnostic) -> Self {
        Self { diagnostics: vec![diagnostic] }
    }
}

impl From<Vec<Diagnostic>> for InputError {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        Self { diagnostics }
    }
}

impl Display for InputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(formatter, "{}: {}", self.code(), self.message())
    }
}

impl Error for InputError {}

/// Probes bytes without importing them.
///
/// The extension hint is passed as given, with or without a leading dot.
#[must_use]
pub fn probe(bytes: &[u8], extension_hint: Option<&str>) -> FormatDetection {
    detect_bytes(bytes, extension_hint)
}

/// Imports a source whose format was selected by the host.
///
/// The declared format is checked against the probe first. A ZIP source
/// declared as DOCX or EPUB is accepted, because the package parts can only be
/// separated by the reader. A source that the probe resolves to another format
/// fails before any reader runs.
pub fn import_bytes(format: Format, label: &str, bytes: &[u8]) -> Result<ImportedDocument, InputError> {
    ensure_source_budget(bytes)?;
    let detection = detect_bytes(bytes, Some(format.extension()));
    if let Some(actual) = detection.format {
        if actual != format.id() {
            return Err(mismatch(format, actual, label));
        }
    }
    else if detection.confidence == DetectionConfidence::Ambiguous && !detection.candidates.contains(&format.id()) {
        return Err(mismatch(format, detection.candidates.join(" / ").as_str(), label));
    }
    import_with_detection(format, label, bytes, detection)
}

/// Probes the source and imports it when exactly one candidate is named.
///
/// An unresolved format is refused with a structured diagnostic instead of
/// defaulting to Markdown, because a wrong fallback would produce a document
/// from bytes that were never recognized.
pub fn import_auto(label: &str, bytes: &[u8], extension_hint: Option<&str>) -> Result<ImportedDocument, InputError> {
    ensure_source_budget(bytes)?;
    let detection = detect_bytes(bytes, extension_hint);
    let Some(id) = detection.format else {
        return Err(probe_failure(&detection, label));
    };
    let Some(format) = Format::from_id(id) else {
        return Err(InputError::from(Diagnostic {
            code: codes::UNSUPPORTED_FORMAT.to_owned(),
            severity: Severity::Error,
            message: format!(
                "{label}: probed `{id}`, which belongs to a text-language profile rather than this container profile",
            ),
            span: None,
        }));
    };
    import_with_detection(format, label, bytes, detection)
}

/// Reads a file and imports it.
///
/// With `format = None` the path extension is used as a probe hint, so a ZIP
/// package is routed to DOCX or EPUB. An extension that does not resolve the
/// container is refused through [`import_auto`].
pub fn import_file(path: impl AsRef<Path>, format: Option<Format>) -> Result<ImportedDocument, InputError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|error| {
        InputError::from(Diagnostic {
            code: codes::FILE_READ_FAILED.to_owned(),
            severity: Severity::Error,
            message: format!("failed to read {}: {error}", path.display()),
            span: None,
        })
    })?;
    let label = path.display().to_string();
    let extension = path.extension().and_then(|extension| extension.to_str());
    match format {
        Some(format) => import_bytes(format, &label, &bytes),
        None => import_auto(&label, &bytes, extension),
    }
}

fn import_with_detection(
    format: Format,
    label: &str,
    bytes: &[u8],
    detection: FormatDetection,
) -> Result<ImportedDocument, InputError> {
    let graph = match format {
        Format::Html => import_html_bytes(label, bytes),
        Format::Docx => import_docx_bytes(label, bytes),
        Format::Epub => import_epub_bytes(label, bytes),
        Format::Pdf => import_pdf_bytes(label, bytes),
        Format::Doc => import_doc_bytes(label, bytes),
    }
    .map_err(|error| {
        InputError::from(Diagnostic {
            code: codes::IMPORT_FAILED.to_owned(),
            severity: Severity::Error,
            message: format!("`{}` reader rejected `{label}`: {error}", format.id()),
            span: None,
        })
    })?;
    finish(format, detection, graph)
}

fn finish(format: Format, detection: FormatDetection, graph: DocumentGraph) -> Result<ImportedDocument, InputError> {
    let validation = graph.validate();
    let (report, diagnostics) = report::analyze(&graph);
    if !validation.is_valid() {
        let mut collected: Vec<Diagnostic> = validation
            .issues
            .iter()
            .map(|issue| Diagnostic {
                code: codes::INVALID_GRAPH.to_owned(),
                severity: Severity::Error,
                message: format!("{issue:?}"),
                span: None,
            })
            .collect();
        collected.extend(diagnostics);
        return Err(InputError::from(collected));
    }
    if !report.unsupported.is_empty() {
        return Err(InputError::from(diagnostics));
    }
    Ok(ImportedDocument { format, detection, graph, report, diagnostics })
}

fn ensure_source_budget(bytes: &[u8]) -> Result<(), InputError> {
    if bytes.len() <= MAX_SOURCE_BYTES {
        return Ok(());
    }
    Err(InputError::from(Diagnostic {
        code: codes::SOURCE_TOO_LARGE.to_owned(),
        severity: Severity::Error,
        message: format!("source is {} bytes, above the {MAX_SOURCE_BYTES} byte budget", bytes.len()),
        span: None,
    }))
}

fn mismatch(declared: Format, actual: &str, label: &str) -> InputError {
    InputError::from(Diagnostic {
        code: codes::FORMAT_MISMATCH.to_owned(),
        severity: Severity::Error,
        message: format!("{label}: declared `{}` but the probe resolved `{actual}`", declared.id()),
        span: None,
    })
}

fn probe_failure(detection: &FormatDetection, label: &str) -> InputError {
    let (code, message) = match detection.confidence {
        DetectionConfidence::Ambiguous => (
            codes::AMBIGUOUS_FORMAT,
            format!(
                "{label}: bytes stay ambiguous between {} and no extension hint resolved them",
                detection.candidates.join(" / ")
            ),
        ),
        _ => (
            codes::UNKNOWN_FORMAT,
            format!("{label}: {} so an explicit format is required", detection.evidence),
        ),
    };
    InputError::from(Diagnostic { code: code.to_owned(), severity: Severity::Error, message, span: None })
}

#[cfg(test)]
mod tests;
