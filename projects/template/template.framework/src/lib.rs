#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

use serde_json::Value;
use template::{RenderOptions, TemplateContentKind, TemplateError, TemplateId, TemplateLoader, TemplateStatus};

/// One build framework that drives template input engines.
///
/// A framework decides page data, picks the entry template and orders layouts,
/// but it never owns template syntax: rendering is delegated to the input
/// engines named by [`FrameworkCapability::engines`].
pub trait TemplateFramework {
    /// Stable framework id, matching the sub-namespace (`doki`).
    fn id(&self) -> &'static str;

    /// This framework's declared capability.
    fn capability(&self) -> FrameworkCapability;

    /// Render one page through the framework's own layout rules.
    fn render(&self, request: &FrameworkRequest<'_>) -> Result<FrameworkOutput, TemplateError>;
}

/// One framework's declared capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameworkCapability {
    /// Framework id, matching the sub-namespace (`template.framework.<id>`).
    pub id: &'static str,
    /// Coverage status.
    pub status: TemplateStatus,
    /// Input engine ids this framework can drive, such as `["dejavu"]`.
    pub engines: &'static [&'static str],
    /// Framework contract version, such as `doki/v0`.
    pub contract: &'static str,
}

/// One framework render request.
#[derive(Debug)]
pub struct FrameworkRequest<'a> {
    /// Entry page or layout reference, resolved by the framework's own rules.
    pub entry: &'a str,
    /// Loader handed to the input engine.
    pub loader: &'a dyn TemplateLoader,
    /// Page data (`site` / `page` / `content` …).
    pub data: &'a Value,
    /// Strictness and budget ceilings.
    pub options: &'a RenderOptions,
}

/// One successful framework render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkOutput {
    /// Rendered text.
    pub text: String,
    /// What the framework produced, so callers pick the existing input adapter.
    pub content: TemplateContentKind,
    /// Every template resolved during the render, entry first when known.
    pub dependencies: Vec<TemplateId>,
}
