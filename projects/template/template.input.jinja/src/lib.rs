#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

mod ast;
pub mod codes;
mod eval;
mod parse;

use template::{
    RenderBudget, RenderOutput, RenderRequest, TemplateCapability, TemplateContentKind, TemplateEngine, TemplateError,
    TemplateStatus,
};

/// Jinja2 syntax subset engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct JinjaEngine;

impl JinjaEngine {
    /// Create the engine.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl TemplateEngine for JinjaEngine {
    fn id(&self) -> &'static str {
        "jinja"
    }

    fn capability(&self) -> TemplateCapability {
        TemplateCapability {
            id: "jinja",
            status: TemplateStatus::Partial,
            content: TemplateContentKind::Html,
            extensions: eval::EXTENSIONS,
            contract: "jinja/v0",
        }
    }

    fn render(&self, request: &RenderRequest<'_>) -> Result<RenderOutput, TemplateError> {
        RenderBudget::new(request.options.limits).source(request.source)?;
        let nodes = parse::parse(request.source)?;
        eval::render(request, &nodes)
    }
}
