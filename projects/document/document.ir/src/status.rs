use serde::{Deserialize, Serialize};

/// Explicit semantic or conversion state on IR objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticStatus {
    /// Fully resolved semantics.
    Resolved,
    /// Intentionally unresolved target or value.
    Unresolved,
    /// Partially mapped semantics.
    ///
    /// An accepted construct whose content is kept while part of its semantics
    /// is dropped, such as an image that keeps its target but loses its
    /// authored size.
    Partial,
    /// Refused construct: none of its authored content reaches the output.
    ///
    /// Import may keep an inert shell or an unrendered opaque marker for
    /// provenance, and the projection skips it, so readers never see the
    /// construct. Use it only when the content is dropped as a whole. When the
    /// content is kept in the output and only some semantics are missing,
    /// record [`SemanticStatus::Partial`] or [`SemanticStatus::Lossy`] instead,
    /// so consumers that read the status do not report emitted content as a
    /// refusal.
    Unsupported,
    /// Semantics inferred rather than authored.
    Inferred,
    /// Semantics preserved with known loss.
    Lossy,
}

/// Writer-facing loss marker attached during import or export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LossMarker {
    /// Stable loss code for tooling and diagnostics.
    pub code: String,
    /// Human-readable loss explanation.
    pub message: String,
    /// Severity or resolution state of the loss.
    pub status: SemanticStatus,
}
