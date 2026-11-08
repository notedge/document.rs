use serde_json::Value;

use crate::{RenderOptions, TemplateCapability, TemplateError, TemplateId, TemplateLoader};

/// One successful render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOutput {
    /// Rendered text, handed to the matching input adapter by the caller.
    pub text: String,
    /// Every template resolved during the render; the entry id comes first when known.
    pub dependencies: Vec<TemplateId>,
}

/// One render request.
#[derive(Debug)]
pub struct RenderRequest<'a> {
    /// Entry template source.
    pub source: &'a str,
    /// Entry template id, used for `./` and `../` references.
    pub entry: Option<&'a TemplateId>,
    /// Loader used by partials and includes.
    pub loader: &'a dyn TemplateLoader,
    /// Render data.
    pub data: &'a Value,
    /// Strictness and budget ceilings.
    pub options: &'a RenderOptions,
}

/// One template engine.
pub trait TemplateEngine {
    /// Stable engine id, matching the sub-namespace.
    fn id(&self) -> &'static str;

    /// This engine's declared capability.
    fn capability(&self) -> TemplateCapability;

    /// Render one source.
    fn render(&self, request: &RenderRequest<'_>) -> Result<RenderOutput, TemplateError>;
}
