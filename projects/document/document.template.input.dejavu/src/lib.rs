#![doc = include_str!("../readme.md")]

//! Dejavu 模板语言（`template.input.dejavu`）。
//!
//! 语法、IR、渲染与冻结诊断码在本 crate 内自洽；共享的 loader / 预算 / 错误通道
//! 来自 `document-template`。`DejavuEngine` 实现共享 `TemplateEngine`，宿主也可以
//! 直接用 IR 级入口（parse / render）做 AOT 或工具链。

pub mod builtins;
pub mod catalog;
pub mod diagnostic;
pub mod language;
pub mod native;
pub mod normalize;
pub mod registry;
pub mod render;
mod template_bridge;
pub mod trim;
pub mod types;

pub use catalog::{
    CatalogFiles, CatalogLoader, CatalogRoot, ResolveHit, ResolveRoot, TemplateCatalog, canonical_id, resolve_ref,
};
pub use diagnostic::{ArgValue, Diagnostic, RelatedSpan, Severity, Span, Stage, codes};
pub use language::{ParseError, ParseResult, parse, parse_named, parse_native, parse_native_with, parse_with};
pub use native::{NativeExpr, NativeImportName, NativeKind, NativeNode, NativeParam, NativeTemplate};
pub use normalize::normalize_value;
pub use registry::{Capability, CapabilitySet, CustomEntry, Namespace, validate_custom_entries};
pub use render::{
    CustomError, CustomRegistry, RenderOptions, ResolvedTemplate, TemplateLoader, WarningSink, check_source_budget, render_ir,
    render_ir_opts, render_ir_with_loader,
};
pub use trim::{level_of, normalized_children, trim_prefix, trim_suffix};
pub use types::{IR_VERSION, IrDocument, IrNode, Language, TemplateConfig, default_language};

pub use document_template::RenderLimits;

use std::cell::RefCell;

use document_template::{
    RenderBudget, RenderOutput, RenderRequest, TemplateCapability, TemplateContentKind, TemplateEngine, TemplateError,
    TemplateId, TemplateStatus,
};
use serde_json::Value;

/// Extensions the engine probes for dejavu sources.
pub const EXTENSIONS: &[&str] = &["", ".doki", ".dejavu", ".html"];

/// Errors produced by IR encode/decode/render.
///
/// - `Json`：serde_json 错误（宿主输入 / IR wire 问题）
/// - `Diagnostic`：模板域错误的唯一形态（code + args + span + stage）
#[derive(Debug)]
pub enum IrError {
    Json(serde_json::Error),
    /// 结构化诊断（code + args + span + stage）。
    Diagnostic(Diagnostic),
}

impl std::fmt::Display for IrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrError::Json(e) => write!(f, "json error: {e}"),
            IrError::Diagnostic(d) => d.fmt(f),
        }
    }
}

impl std::error::Error for IrError {}

impl From<serde_json::Error> for IrError {
    fn from(e: serde_json::Error) -> Self {
        IrError::Json(e)
    }
}

impl From<Diagnostic> for IrError {
    fn from(d: Diagnostic) -> Self {
        IrError::Diagnostic(d)
    }
}

impl IrError {
    /// 在 Diagnostic 变体上加一个 arg。其他变体保持不变。
    pub fn arg(self, key: impl Into<String>, value: impl Into<diagnostic::ArgValue>) -> Self {
        match self {
            IrError::Diagnostic(d) => IrError::Diagnostic(d.arg(key, value)),
            other => other,
        }
    }

    /// 在 Diagnostic 变体上加 span。
    pub fn span(self, start: usize, end: usize) -> Self {
        match self {
            IrError::Diagnostic(d) => IrError::Diagnostic(d.span(start, end)),
            other => other,
        }
    }

    /// 在 Diagnostic 变体上加 path。
    pub fn path(self, p: impl Into<String>) -> Self {
        match self {
            IrError::Diagnostic(d) => IrError::Diagnostic(d.path(p)),
            other => other,
        }
    }

    /// 在 Diagnostic 变体上加 template_id。
    pub fn template_id(self, id: impl Into<String>) -> Self {
        match self {
            IrError::Diagnostic(d) => IrError::Diagnostic(d.template_id(id)),
            other => other,
        }
    }
}

/// 快速构造 render 阶段的错误（直接返回 IrError，避免 blanket Into 歧义）。
pub fn diag_render(code: &str) -> IrError {
    IrError::Diagnostic(Diagnostic::new(Severity::Error, code, Stage::Render))
}

/// 快速构造 load 阶段的错误（直接返回 IrError，避免 blanket Into 歧义）。
pub fn diag_load(code: &str) -> IrError {
    IrError::Diagnostic(Diagnostic::new(Severity::Error, code, Stage::Load))
}

/// Encode a native T1 AST into an IR document.
pub fn encode_native(template: &NativeTemplate) -> IrDocument {
    IrDocument { ir_version: IR_VERSION.into(), language: default_language(), body: native::encode_template(template) }
}

/// Decode IR JSON into typed IR, then to native AST.
pub fn decode_ir_json(json: &str) -> Result<NativeTemplate, IrError> {
    let doc: IrDocument = serde_json::from_str(json)?;
    Ok(native::decode_template(&doc.body))
}

/// Render IR JSON with a JSON object context.
pub fn render_ir_json(ir_json: &str, ctx: &Value) -> Result<String, IrError> {
    let doc: IrDocument = serde_json::from_str(ir_json)?;
    render_ir(&doc, ctx)
}

/// Normalize an IR JSON value for semantic equality.
pub fn normalize_ir_json(json: &str) -> Result<Value, IrError> {
    let value: Value = serde_json::from_str(json)?;
    Ok(normalize_value(value))
}

/// Convert a parser failure into the cross-host diagnostic contract.
pub fn parse_error_diagnostic(e: ParseError) -> Diagnostic {
    let mut diag = Diagnostic::new(Severity::Error, e.code_or_default(), Stage::Parse).message(e.message).path(e.file);
    if e.span.len() > 0 {
        diag = diag.span(e.span.start, e.span.end);
    }
    for (key, value) in e.args {
        diag = diag.arg(key, value);
    }
    diag
}

/// Resolve the language profile for one template path.
///
/// `.doki` / `.dj` use `{% %}` code and `{# #}` comments; every other
/// extension (including `.dejavu`, `.html` and no extension) uses the dejavu
/// default `<% %>` profile.
#[must_use]
pub fn language_for_path(path: &str) -> Language {
    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    match extension {
        Some("doki") | Some("dj") => Language {
            syntax_mode: "template".into(),
            template: TemplateConfig {
                code_start: "{%".into(),
                code_end: "%}".into(),
                comment_start: "{#".into(),
                comment_end: "#}".into(),
                support_filter_pipe: true,
                legacy_for: false,
            },
        },
        _ => default_language(),
    }
}

/// The `template.input.dejavu` engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct DejavuEngine;

impl DejavuEngine {
    /// Create the engine.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl TemplateEngine for DejavuEngine {
    fn id(&self) -> &'static str {
        "dejavu"
    }

    fn capability(&self) -> TemplateCapability {
        TemplateCapability {
            id: "dejavu",
            status: TemplateStatus::Ready,
            content: TemplateContentKind::Html,
            extensions: EXTENSIONS,
            contract: "dejavu/v0",
        }
    }

    fn render(&self, request: &RenderRequest<'_>) -> Result<RenderOutput, TemplateError> {
        RenderBudget::new(request.options.limits).source(request.source).map_err(|_| {
            TemplateError::with_args(
                codes::SOURCE_BUDGET,
                "dejavu template source exceeds the shared source budget",
                vec![("limit", request.options.limits.max_source_bytes.to_string())],
            )
        })?;

        let entry_name = request.entry.map(ToString::to_string);
        let entry_path = request.entry.map_or("", TemplateId::path);
        let language = language_for_path(entry_path);
        let doc = parse_with(request.source, entry_name.as_deref().unwrap_or("<main>"), &language)
            .map_err(|e| to_template_error(parse_error_diagnostic(e)))?;

        let mut loader = EngineLoader::new(request.loader, request.options.limits);
        let options = render::RenderOptions {
            strict_undefined: request.options.strict_undefined,
            name: entry_name.clone(),
            limits: request.options.limits,
            ..render::RenderOptions::default()
        };
        let text = render_ir_opts(&doc, request.data, options, Some(&mut loader)).map_err(ir_to_template_error)?;

        let mut dependencies = Vec::new();
        if let Some(entry) = request.entry {
            dependencies.push(entry.clone());
        }
        for id in loader.into_dependencies() {
            if !dependencies.contains(&id) {
                dependencies.push(id);
            }
        }
        Ok(RenderOutput { text, dependencies })
    }
}

/// Map one IR failure onto the shared template error, keeping the engine code.
fn ir_to_template_error(error: IrError) -> TemplateError {
    match error {
        IrError::Diagnostic(diagnostic) => to_template_error(diagnostic),
        IrError::Json(error) => TemplateError::new("template.parse-failed", error.to_string()),
    }
}

/// Map one dejavu diagnostic onto the shared template error, keeping the code.
fn to_template_error(diagnostic: Diagnostic) -> TemplateError {
    let args = diagnostic.args.iter().map(|(name, value)| (name.clone(), arg_text(value))).collect::<Vec<_>>();
    TemplateError::with_owned_args(
        diagnostic.code.clone(),
        diagnostic.message.clone().unwrap_or_else(|| diagnostic.code.clone()),
        args,
    )
}

fn arg_text(value: &ArgValue) -> String {
    match value {
        ArgValue::String(value) => value.clone(),
        other => format!("{other:?}"),
    }
}

/// Loader adapter: shared loader resolves ids and hands back source text, then
/// this engine parses per-source with the language profile of its extension.
#[derive(Debug)]
struct EngineLoader<'a> {
    inner: &'a dyn document_template::TemplateLoader,
    limits: RenderLimits,
    resolved: RefCell<std::collections::HashMap<(String, Option<String>), Result<ResolvedTemplate, Diagnostic>>>,
    dependencies: RefCell<Vec<TemplateId>>,
}

impl<'a> EngineLoader<'a> {
    fn new(inner: &'a dyn document_template::TemplateLoader, limits: RenderLimits) -> Self {
        Self { inner, limits, resolved: RefCell::new(std::collections::HashMap::new()), dependencies: RefCell::new(Vec::new()) }
    }

    fn into_dependencies(self) -> Vec<TemplateId> {
        self.dependencies.into_inner()
    }

    fn resolve_shared(&self, reference: &str, from: Option<&str>) -> Result<ResolvedTemplate, Diagnostic> {
        let key = (reference.to_string(), from.map(ToString::to_string));
        if let Some(cached) = self.resolved.borrow().get(&key) {
            return cached.clone();
        }
        let result = self.load_shared(reference, from);
        self.resolved.borrow_mut().insert(key, result.clone());
        result
    }

    fn load_shared(&self, reference: &str, from: Option<&str>) -> Result<ResolvedTemplate, Diagnostic> {
        let from_id = match from {
            Some(from) if from != "<main>" => Some(TemplateId::parse(from).map_err(template_error_to_diagnostic)?),
            _ => None,
        };
        let id = self.inner.resolve(reference, from_id.as_ref(), EXTENSIONS).map_err(template_error_to_diagnostic)?;
        let source = self.inner.source(&id).map_err(template_error_to_diagnostic)?;
        RenderBudget::new(self.limits).source(source).map_err(|_| {
            Diagnostic::new(Severity::Error, codes::SOURCE_BUDGET, Stage::Load)
                .arg("limit", self.limits.max_source_bytes as i64)
        })?;
        let language = language_for_path(id.path());
        let document = parse_with(source, &id.to_string(), &language).map_err(parse_error_diagnostic)?;
        self.dependencies.borrow_mut().push(id.clone());
        Ok(ResolvedTemplate { id: id.to_string(), document })
    }
}

impl render::TemplateLoader for EngineLoader<'_> {
    fn resolve(&mut self, ref_: &str, from: Option<&str>) -> Result<ResolvedTemplate, IrError> {
        self.resolve_shared(ref_, from).map_err(IrError::Diagnostic)
    }
}

fn template_error_to_diagnostic(error: TemplateError) -> Diagnostic {
    Diagnostic::new(Severity::Error, error.code(), Stage::Load).message(error.to_string())
}
