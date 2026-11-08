/// Whether one engine covers its full documented surface or a declared subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateStatus {
    /// The engine implements its full documented surface.
    Ready,
    /// The engine implements a documented subset and rejects the rest with stable codes.
    Partial,
}

/// What a renderer produces, so the caller can pick the existing input adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateContentKind {
    /// Output is HTML and enters `document.formats::import::html`.
    Html,
    /// Output is plain text and enters a text-shaped input adapter.
    Text,
}

/// One engine's declared capability.
///
/// The engine crate is the only source of truth; the core contract keeps no
/// aggregate table of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateCapability {
    /// Engine id, matching the sub-namespace (`hbs`, `jinja`).
    pub id: &'static str,
    /// Coverage status.
    pub status: TemplateStatus,
    /// Output content kind.
    pub content: TemplateContentKind,
    /// File extensions the loader probes for this engine.
    pub extensions: &'static [&'static str],
    /// Engine contract version, such as `hbs/v0`.
    pub contract: &'static str,
}
