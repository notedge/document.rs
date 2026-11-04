#![deny(missing_docs)]
//! Document format import and export around `notedown-ir`.

mod capability;
mod error;
pub mod export;
pub mod import;
pub mod inspect;
#[cfg(feature = "tex")]
pub mod tex_corpus;

pub use capability::{FormatCapability, FormatDirection, FormatStatus};
pub use error::{FormatError, NAMESPACE as DIAGNOSTIC_NAMESPACE, codes as diagnostic_codes, from_format_error, origin_formats};
pub use inspect::{DocumentInspect, InspectBudget, InspectedPart};

/// Lower a standalone TeX formula source into the document math semantics.
#[cfg(feature = "tex")]
pub use import::lower_tex_formula_source;

#[cfg(feature = "docx")]
pub use inspect::inspect_docx_bytes;

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
        // The conservative DOCX exporter is wired but not deliverable as a
        // general target: `Block::Math`, `Block::RawHtml` and opaque blocks
        // return an unsupported error instead of being represented. Marking the
        // export `Unavailable` withdraws the route while keeping the writer.
        capabilities.push(FormatCapability {
            id: "docx",
            direction: FormatDirection::Export,
            status: FormatStatus::Unavailable,
        });
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
        // `export::pdf` is wired but not deliverable: it hard-errors on every
        // character outside WinAnsiEncoding (all CJK), prints math as source
        // text, and ignores layout entirely. The deliverable PDF path is
        // `layout → presentation → renderer-pdf`, so the direct export must not
        // be advertised as a route.
        capabilities.push(FormatCapability {
            id: "pdf",
            direction: FormatDirection::Export,
            status: FormatStatus::Unavailable,
        });
    }
    #[cfg(feature = "epub")]
    {
        capabilities.push(FormatCapability { id: "epub", direction: FormatDirection::Import, status: FormatStatus::Partial });
        capabilities.push(FormatCapability { id: "epub", direction: FormatDirection::Export, status: FormatStatus::Partial });
    }
    #[cfg(feature = "tex")]
    capabilities.push(FormatCapability { id: "tex", direction: FormatDirection::Import, status: FormatStatus::Partial });
    #[cfg(feature = "typst")]
    capabilities.push(FormatCapability { id: "typst", direction: FormatDirection::Import, status: FormatStatus::Partial });
    capabilities
}
