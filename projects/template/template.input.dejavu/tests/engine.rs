//! `template.input.dejavu` 合同用例：两个 profile、include 解析、依赖记录与 strict 码。

use serde_json::json;
use template::{MapTemplateLoader, RenderOptions, RenderRequest, TemplateEngine, TemplateError, TemplateId, TemplateRoot};
use template_input_dejavu::DejavuEngine;

fn loader() -> MapTemplateLoader {
    MapTemplateLoader::new().with_root(
        TemplateRoot::new("theme", 0)
            .with_file("page.doki", "{% if page.title %}<h1>{% page.title %}</h1>{% end if %}")
            .with_file("legacy.dejavu", "Hello, <% name %>!")
            .with_file("wrap.doki", "<main>{% include \"frag.doki\" %}</main>")
            .with_file("frag.doki", "<p>{% page.title %}</p>"),
    )
}

fn render(entry: Option<&TemplateId>, source: &str, data: &serde_json::Value, strict: bool) -> Result<String, TemplateError> {
    let loader = loader();
    DejavuEngine
        .render(&RenderRequest {
            source,
            entry,
            loader: &loader,
            data,
            options: &RenderOptions { strict_undefined: strict, ..RenderOptions::default() },
        })
        .map(|output| output.text)
}

#[test]
fn doki_profile_renders_statements_with_curly_delimiters() {
    let entry = TemplateId::new("theme", "page.doki");
    let text = render(
        Some(&entry),
        "{% if page.title %}<h1>{% page.title %}</h1>{% end if %}",
        &json!({ "page": { "title": "Hello" } }),
        false,
    )
    .expect("render");
    assert_eq!(text, "<h1>Hello</h1>");
}

#[test]
fn dejavu_profile_renders_with_angle_delimiters() {
    let entry = TemplateId::new("theme", "legacy.dejavu");
    let text = render(Some(&entry), "Hello, <% name %>!", &json!({ "name": "Ada" }), false).expect("render");
    assert_eq!(text, "Hello, Ada!");
}

#[test]
fn includes_resolve_through_the_shared_loader() {
    let output = DejavuEngine
        .render(&RenderRequest {
            source: "<main><% include \"frag.doki\" %></main>",
            entry: None,
            loader: &loader(),
            data: &json!({ "page": { "title": "Hi" } }),
            options: &RenderOptions::default(),
        })
        .expect("render");
    assert_eq!(output.text, "<main><p>Hi</p></main>");
    assert_eq!(output.dependencies, vec![TemplateId::new("theme", "frag.doki")]);
}

#[test]
fn strict_undefined_keeps_the_frozen_dejavu_code() {
    let entry = TemplateId::new("theme", "page.doki");
    let error = render(Some(&entry), "{% page.title %}", &json!({ "page": {} }), true).expect_err("strict should fail");
    assert_eq!(error.code(), "dejavu.template.undefined-member");
}

#[test]
fn capability_declares_the_input_namespace() {
    let capability = DejavuEngine.capability();
    assert_eq!(capability.id, "dejavu");
    assert_eq!(capability.contract, "dejavu/v0");
    assert_eq!(capability.extensions, &["", ".doki", ".dejavu", ".html"]);
}
