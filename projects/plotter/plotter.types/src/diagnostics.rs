//! Stable diagnostic codes and constructors built on the workspace
//! `boost-diagnostic` model.

pub use boost_diagnostic::{
    ByteRange, Diagnostic, DiagnosticLabel, DiagnosticLocation, DiagnosticOrigin, DiagnosticSeverity, LabelRole, Message,
    SourceRef,
};

/// Product namespace for every DaVinci diagnostic.
pub const NAMESPACE: &str = "davinci";

/// Stable dotted diagnostic codes emitted by the DaVinci subsystem.
///
/// Codes use the `davinci.<component>.<failure>` shape and must not be renamed
/// after a wire contract consumes them.
///
/// Two rules keep the table meaningful for a consumer that classifies on codes:
///
/// - A fatal code (`parse-failed`, `ast-unavailable`, `unsupported`,
///   `empty-diagram`) means the whole diagram is refused and no scene exists.
/// - A non-fatal code (`approximated`) means the diagram is accepted and the
///   scene exists, but part of the authored meaning was lost or changed.
///
/// `unsupported` therefore never appears on an accepted diagram. A construct
/// that lowers with loss is recorded as `approximated` naming the exact loss, so
/// a host reading `unsupported` can refuse the source and fall back to it
/// without re-deriving whether the scene is usable.
pub mod codes {
    /// Geometry carries a non-finite or negative extent.
    pub const INVALID_GEOMETRY: &str = "davinci.scene.invalid-geometry";
    /// A style value is outside its valid range.
    pub const INVALID_STYLE: &str = "davinci.scene.invalid-style";
    /// A transform component is not finite.
    pub const INVALID_TRANSFORM: &str = "davinci.scene.invalid-transform";
    /// A scene could not be serialized.
    pub const SERIALIZATION_FAILED: &str = "davinci.scene.serialization-failed";
    /// A diagram source could not be parsed.
    pub const PARSE_FAILED: &str = "davinci.diagram.parse-failed";
    /// The Oak Mermaid frontend produced no usable AST.
    pub const AST_UNAVAILABLE: &str = "davinci.diagram.ast-unavailable";
    /// A diagram family or construct has no lowering, so the whole source is
    /// refused and no scene is produced.
    ///
    /// Never emitted on an accepted diagram: a construct that lowers with loss
    /// uses [`APPROXIMATED`] instead.
    pub const UNSUPPORTED: &str = "davinci.diagram.unsupported";
    /// The diagram is accepted but a construct lowers with a named loss, such
    /// as an approximated shape, a skipped reference, or a flattened frame.
    pub const APPROXIMATED: &str = "davinci.diagram.approximated";
    /// A diagram has no nodes or edges to lay out.
    pub const EMPTY_DIAGRAM: &str = "davinci.diagram.empty-diagram";
}

/// Build a diagnostic origin for a DaVinci component.
pub fn origin(component: &str) -> DiagnosticOrigin {
    DiagnosticOrigin::new(NAMESPACE, component)
}

/// Build a diagnostic with a stable code, component origin, severity, and
/// human-readable fallback text.
pub fn diagnostic(component: &str, code: &str, severity: DiagnosticSeverity, fallback: impl Into<String>) -> Diagnostic {
    let message = Message::new(code).with_fallback(fallback.into());
    Diagnostic::new(code.into(), severity, origin(component), message)
}
