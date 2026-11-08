//! The template namespace produces text only. This test pins the boundary: rendered
//! text enters the existing HTML import path, and no template engine appears as a
//! format capability or conversion route.

use document::{
    convert::{READERS, WRITERS},
    formats::capabilities,
    ir::{Block, DocumentGraph, Inline},
    template::{MapTemplateLoader, RenderOptions, RenderRequest, TemplateEngine, hbs::HbsEngine},
};
use serde_json::json;

fn paragraph_text(graph: &DocumentGraph) -> String {
    let mut parts = Vec::new();
    for node in &graph.blocks {
        if let Block::Paragraph { content } = &node.block {
            for inline in content {
                if let Inline::Text { text } = inline {
                    parts.push(text.clone());
                }
            }
        }
    }
    parts.join("")
}

#[test]
fn rendered_text_enters_the_html_import_path() {
    let loader = MapTemplateLoader::new();
    let output = HbsEngine
        .render(&RenderRequest {
            source: "<p>Hello {{ name }}</p>",
            entry: None,
            loader: &loader,
            data: &json!({ "name": "Ada" }),
            options: &RenderOptions::default(),
        })
        .expect("render");

    let graph = document::formats::import::html::import_html_text("rendered.html", &output.text).expect("import html");
    assert!(paragraph_text(&graph).contains("Hello Ada"));
}

#[test]
fn template_is_not_a_format_or_a_conversion_route() {
    assert!(capabilities().iter().all(|capability| capability.id != "template"));
    assert!(capabilities().iter().any(|capability| capability.id == "html"));
    assert!(READERS.iter().all(|reader| *reader != "template"));
    assert!(WRITERS.iter().all(|writer| *writer != "template"));
}
