/// Whether a format module reads or writes documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatDirection {
    /// Import external bytes into `notedown-ir`.
    Import,
    /// Export `notedown-ir` into external bytes.
    Export,
}

/// Implementation readiness for a format module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatStatus {
    /// Production-ready for the advertised surface.
    Ready,
    /// Usable with known semantic gaps.
    Partial,
    /// Scaffolded but not yet wired.
    Planned,
    /// Explicitly not available to the routing layer.
    ///
    /// Covers both "no module exists" and "a module exists but cannot deliver
    /// this direction" — for example the conservative DOCX and PDF exporters.
    /// A route must never advertise a direction marked with this status.
    Unavailable,
}

/// One import or export capability entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatCapability {
    /// Stable format identifier such as `markdown` or `docx`.
    pub id: &'static str,
    /// Whether this entry describes import or export.
    pub direction: FormatDirection,
    /// Implementation readiness for the advertised direction.
    pub status: FormatStatus,
}
