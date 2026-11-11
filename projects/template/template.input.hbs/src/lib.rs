#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

mod ast;
pub mod codes;
mod eval;
mod parser;

use template::{
    RenderBudget, RenderOutput, RenderRequest, TemplateCapability, TemplateContentKind, TemplateEngine, TemplateError,
    TemplateStatus,
};

/// Handlebars syntax subset engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct HbsEngine;

impl HbsEngine {
    /// Create the engine.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl TemplateEngine for HbsEngine {
    fn id(&self) -> &'static str {
        "hbs"
    }

    fn capability(&self) -> TemplateCapability {
        TemplateCapability {
            id: "hbs",
            status: TemplateStatus::Partial,
            content: TemplateContentKind::Html,
            extensions: eval::EXTENSIONS,
            contract: "hbs/v0",
        }
    }

    fn render(&self, request: &RenderRequest<'_>) -> Result<RenderOutput, TemplateError> {
        RenderBudget::new(request.options.limits).source(request.source)?;
        let nodes = parser::parse(request.source)?;
        eval::render(request, &nodes)
    }
}
