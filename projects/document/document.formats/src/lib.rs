#![deny(missing_docs)]
//! Document format import and export around `notedown-ir`.

mod capability;
pub mod detect;
mod error;
pub mod export;
pub mod import;
pub mod inspect;
#[cfg(any(feature = "docx", feature = "epub", feature = "html"))]
mod query_gate;
#[cfg(feature = "tex")]
pub mod tex_corpus;

pub use capability::{FormatCapability, FormatDirection, FormatStatus};
pub use error::{FormatError, NAMESPACE as DIAGNOSTIC_NAMESPACE, codes as diagnostic_codes, from_format_error, origin_formats};
pub use inspect::{ContainerLayout, DocumentInspect, DocxVerification, InspectBudget, InspectedPart, LayoutSpan};
#[cfg(any(feature = "docx", feature = "epub", feature = "html"))]
pub use query_gate::QueryLimits;

/// Lower a standalone TeX formula source into the document math semantics.
#[cfg(feature = "tex")]
pub use import::lower_tex_formula_source;

#[cfg(feature = "docx")]
pub use inspect::inspect_docx_bytes;
#[cfg(feature = "epub")]
pub use inspect::inspect_epub_bytes;
#[cfg(feature = "docx")]
pub use inspect::verify_docx_bytes;

/// Registered format capabilities for tooling probes.
///
/// This is the single capability table. `document-convert` derives route
/// availability from it (`supports_route`), so an `Unavailable` direction is
/// never advertised even when a module for it is still compiled: the entry
/// says "must not be routed", not "no bytes exist".
pub fn capabilities() -> Vec<FormatCapability> {
    let mut capabilities = Vec::new();
    #[cfg(feature = "notedown")]
    capabilities.push(FormatCapability { id: "notedown", direction: FormatDirection::Import, status: FormatStatus::Partial });
    #[cfg(feature = "markdown")]
    {
        capabilities.push(FormatCapability {
            id: "markdown",
            direction: FormatDirection::Import,
            status: FormatStatus::Partial,
        });
        capabilities.push(FormatCapability {
            id: "markdown",
            direction: FormatDirection::Export,
            status: FormatStatus::Partial,
        });
    }
    capabilities.push(FormatCapability { id: "html", direction: FormatDirection::Export, status: FormatStatus::Partial });
    #[cfg(feature = "html")]
    capabilities.push(FormatCapability { id: "html", direction: FormatDirection::Import, status: FormatStatus::Partial });
    #[cfg(feature = "docx")]
    {
        capabilities.push(FormatCapability { id: "docx", direction: FormatDirection::Import, status: FormatStatus::Partial });
        // The conservative DOCX exporter covers headings, paragraphs, lists,
        // tables, code, media, footnotes and OMML math. Nodes the IR cannot
        // express are written as visible text or kept as raw markup instead of
        // being silently dropped, so the route stays partially deliverable.
        capabilities.push(FormatCapability { id: "docx", direction: FormatDirection::Export, status: FormatStatus::Partial });
    }
    #[cfg(feature = "doc")]
    {
        capabilities.push(FormatCapability { id: "doc", direction: FormatDirection::Import, status: FormatStatus::Partial });
        capabilities.push(FormatCapability {
            id: "doc",
            direction: FormatDirection::Export,
            status: FormatStatus::Unavailable,
        });
    }
    #[cfg(feature = "pdf")]
    {
        capabilities.push(FormatCapability { id: "pdf", direction: FormatDirection::Import, status: FormatStatus::Partial });
        // The PDF export is delivered by the `layout → presentation →
        // renderer-pdf` chain, so it keeps layout, embeds glyphs for CJK text
        // and typesets math instead of printing its source.
        capabilities.push(FormatCapability { id: "pdf", direction: FormatDirection::Export, status: FormatStatus::Partial });
    }
    #[cfg(feature = "epub")]
    {
        capabilities.push(FormatCapability { id: "epub", direction: FormatDirection::Import, status: FormatStatus::Partial });
        capabilities.push(FormatCapability { id: "epub", direction: FormatDirection::Export, status: FormatStatus::Partial });
    }
    #[cfg(feature = "xlsx")]
    {
        // The SpreadsheetML reader resolves the workbook, sheets, cells,
        // styles, merges, comments and images. Charts, conditional
        // formatting, data validation, tables, pivots and embedded objects
        // stay refused with structured codes, so the import is partial.
        capabilities.push(FormatCapability { id: "xlsx", direction: FormatDirection::Import, status: FormatStatus::Partial });
        // No SpreadsheetML writer exists, so the export direction is opt-out.
        capabilities.push(FormatCapability {
            id: "xlsx",
            direction: FormatDirection::Export,
            status: FormatStatus::Unavailable,
        });
    }
    #[cfg(feature = "pptx")]
    {
        // The PresentationML reader resolves slides, layouts, masters,
        // placeholders, text, shapes, images, tables and notes. Charts,
        // SmartArt, OLE objects and media stay refused with structured codes.
        capabilities.push(FormatCapability { id: "pptx", direction: FormatDirection::Import, status: FormatStatus::Partial });
        // No PresentationML writer exists, so the export direction is opt-out.
        capabilities.push(FormatCapability {
            id: "pptx",
            direction: FormatDirection::Export,
            status: FormatStatus::Unavailable,
        });
    }
    #[cfg(feature = "tex")]
    capabilities.push(FormatCapability { id: "tex", direction: FormatDirection::Import, status: FormatStatus::Partial });
    #[cfg(feature = "typst")]
    capabilities.push(FormatCapability { id: "typst", direction: FormatDirection::Import, status: FormatStatus::Partial });
    capabilities
}
