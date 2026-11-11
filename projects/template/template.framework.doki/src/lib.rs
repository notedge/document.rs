#![doc = include_str!("../readme.md")]

//! `template.framework.doki`：Doki 构建框架的模板入口。
//!
//! 框架只决定入口布局与宿主扩展；解析、转义与渲染全部交给
//! `template.input.dejavu`。

use serde_json::Value;

use template::{
    MapTemplateLoader, RenderOptions, TemplateContentKind, TemplateError, TemplateId, TemplateLoader, TemplateStatus,
};
use template_framework::{FrameworkCapability, FrameworkOutput, FrameworkRequest, TemplateFramework};
use template_input_dejavu::{
    CustomRegistry, ParseError, language_for_path, parse_with, render_ir_opts, template_error_from_diagnostic,
    template_error_from_ir,
};

/// Framework id registered under `template.framework.<id>`.
pub const DOKI_FRAMEWORK_ID: &str = "doki";

/// Framework contract version.
pub const DOKI_FRAMEWORK_CONTRACT: &str = "doki/v0";

/// Input engines this framework drives.
pub const DOKI_FRAMEWORK_ENGINES: &[&str] = &["dejavu"];

/// Doki 的 framework 实现。
///
/// `custom` 是宿主扩展通道（i18n `t()` / `locale()` 等），只在 framework 边界内
/// 可见，不进入共享 template 合同。
pub struct DokiFramework<'a> {
    /// Host-supplied registry, fixed for the duration of one render.
    pub custom: Option<&'a dyn CustomRegistry>,
}

impl DokiFramework<'_> {
    /// 本 framework 的能力声明。
    #[must_use]
    pub fn capability() -> FrameworkCapability {
        FrameworkCapability {
            id: DOKI_FRAMEWORK_ID,
            status: TemplateStatus::Ready,
            engines: DOKI_FRAMEWORK_ENGINES,
            contract: DOKI_FRAMEWORK_CONTRACT,
        }
    }
}

impl TemplateFramework for DokiFramework<'_> {
    fn id(&self) -> &'static str {
        DOKI_FRAMEWORK_ID
    }

    fn capability(&self) -> FrameworkCapability {
        Self::capability()
    }

    fn render(&self, request: &FrameworkRequest<'_>) -> Result<FrameworkOutput, TemplateError> {
        let id = request.loader.resolve(request.entry, None, template_input_dejavu::EXTENSIONS)?;
        let source = request.loader.source(&id)?;
        let language = language_for_path(id.path());
        let document = parse_with(source, &id.to_string(), &language)
            .map_err(|error: ParseError| template_error_from_diagnostic(parse_error_diagnostic(error)))?;
        let options = template_input_dejavu::RenderOptions {
            strict_undefined: request.options.strict_undefined,
            name: Some(id.to_string()),
            custom: self.custom,
            limits: request.options.limits,
            ..template_input_dejavu::RenderOptions::default()
        };
        let text = render_ir_opts(&document, request.data, options, None).map_err(template_error_from_ir)?;
        Ok(FrameworkOutput { text, content: TemplateContentKind::Html, dependencies: vec![id] })
    }
}

fn parse_error_diagnostic(error: ParseError) -> template_input_dejavu::Diagnostic {
    template_input_dejavu::parse_error_diagnostic(error)
}

/// Build the single-root loader Doki uses for one layout source.
///
/// The entry path is the layout's basename, so the in-memory root only needs one
/// stable, canonical path that keeps the source's extension (and therefore its
/// language profile).
#[must_use]
pub fn layout_loader(layout_id: &str, source: &str) -> (MapTemplateLoader, TemplateId) {
    let entry_path = layout_entry_path(layout_id);
    let loader =
        MapTemplateLoader::new().with_root(template::TemplateRoot::new("theme", 0).with_file(entry_path.as_str(), source));
    (loader, TemplateId::new("theme", entry_path))
}

fn layout_entry_path(layout_id: &str) -> String {
    let basename = layout_id.rsplit(['/', '\\']).next().unwrap_or(layout_id).trim();
    if basename.is_empty() { "layout".to_string() } else { basename.to_string() }
}

/// Render one layout source without host extensions (no i18n registry).
pub fn render_layout_source(
    layout_id: &str,
    source: &str,
    data: &Value,
    strict_undefined: bool,
) -> Result<FrameworkOutput, TemplateError> {
    let (loader, entry) = layout_loader(layout_id, source);
    let framework = DokiFramework { custom: None };
    let options = RenderOptions { strict_undefined, ..RenderOptions::default() };
    framework.render(&FrameworkRequest { entry: &entry.to_string(), loader: &loader, data, options: &options })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn capability_declares_the_doki_framework() {
        let capability = DokiFramework::capability();
        assert_eq!(capability.id, "doki");
        assert_eq!(capability.engines, &["dejavu"]);
        assert_eq!(capability.contract, "doki/v0");
    }

    #[test]
    fn layout_render_delegates_to_the_dejavu_engine() {
        let output = render_layout_source("page.doki", "{% page.title %}", &json!({ "page": { "title": "Hi" } }), false)
            .expect("render");
        assert_eq!(output.text, "Hi");
        assert_eq!(output.dependencies, vec![TemplateId::new("theme", "page.doki")]);
    }

    #[test]
    fn layout_render_keeps_the_dejavu_error_code() {
        let error = render_layout_source("page.doki", "{% page.title %}", &json!({ "page": {} }), true)
            .expect_err("strict should fail");
        assert_eq!(error.code(), "dejavu.template.undefined-member");
    }

    #[test]
    fn dejavu_profile_is_chosen_by_extension() {
        let output =
            render_layout_source("legacy.dejavu", "Hello, <% name %>!", &json!({ "name": "Ada" }), false).expect("render");
        assert_eq!(output.text, "Hello, Ada!");
    }

    #[test]
    fn custom_registry_is_forwarded_to_the_engine() {
        use template_input_dejavu::{Capability, CustomEntry, CustomError, Namespace};

        struct Host {
            entries: Vec<CustomEntry>,
        }
        impl CustomRegistry for Host {
            fn entries(&self) -> &[CustomEntry] {
                &self.entries
            }
            fn apply_filter(&self, _name: &str, _value: &Value, _args: &[Value]) -> Result<Value, CustomError> {
                Err(CustomError::new("unused"))
            }
            fn call_function(&self, name: &str, _args: &[Value]) -> Result<Value, CustomError> {
                if name == "t" { Ok(Value::String("translated".into())) } else { Err(CustomError::new("unknown")) }
            }
            fn apply_test(&self, name: &str, _value: &Value, _args: &[Value]) -> Result<bool, CustomError> {
                Err(CustomError::new(format!("unused {name}")))
            }
        }

        let host = Host {
            entries: vec![CustomEntry {
                namespace: Namespace::Function,
                name: "t".into(),
                aliases: Vec::new(),
                min_args: 1,
                max_args: 2,
                capability: Capability::Pure,
            }],
        };
        let (loader, entry) = layout_loader("page.doki", "{% t(\"key\") %}");
        let options = RenderOptions::default();
        let framework = DokiFramework { custom: Some(&host) };
        let output = framework
            .render(&FrameworkRequest { entry: &entry.to_string(), loader: &loader, data: &json!({}), options: &options })
            .expect("render");
        assert_eq!(output.text, "translated");
    }
}
