//! PlantUML 前端冒烟：class 家族降到共用 DiagramSpec，越界指令结构化拒绝。

use std::ops::Range;

use plotter_diagram::{
    DiagramError,
    spec::{DiagramNode, NodeShape},
};
use plotter_input_plantuml::plantuml_to_spec;
use plotter_types::codes;

fn span_of<'a>(source: &'a str, error: &DiagramError) -> &'a str {
    let span = error.span().unwrap_or_else(|| panic!("`{}` carries no span", error.message()));
    let Range { start, end } = span.clone();
    assert!(end <= source.len(), "span {start}..{end} leaves a {} byte source", source.len());
    &source[start..end]
}

fn node(id: &str, label: Option<&str>, shape: NodeShape) -> DiagramNode {
    DiagramNode { id: id.to_string(), label: label.map(str::to_string), shape, ports: Vec::new(), cluster: None }
}

const CLASS_SOURCE: &str = r#"@startuml
class Order {
  +id : String
}
@enduml
"#;

#[test]
fn class_declarations_and_members_lower_to_one_spec() {
    let spec = plantuml_to_spec(CLASS_SOURCE).expect("the class subset should lower");
    assert_eq!(spec.nodes, vec![node("Order", Some("Order\n+id : String"), NodeShape::Box)]);
}

#[test]
fn preprocessor_directives_are_refused_with_their_span() {
    let source = "@startuml\n!include common.iuml\n@enduml\n";
    let error = plantuml_to_spec(source).expect_err("preprocessor directives are outside the subset");
    assert_eq!(error.code(), codes::UNSUPPORTED);
    assert_eq!(span_of(source, &error), "!include common.iuml");
}

#[test]
fn component_display_text_survives_the_alias_form() {
    let source = "@startuml\ncomponent [Web] as W\ncomponent [Api] as A\nW --> A\n@enduml\n";
    let spec = plantuml_to_spec(source).expect("the component subset should lower");
    assert_eq!(
        spec.nodes,
        vec![node("W", Some("Web"), NodeShape::Box), node("A", Some("Api"), NodeShape::Box)]
    );
}

#[test]
fn quoted_component_and_package_names_keep_the_display_text() {
    let source = "@startuml\npackage \"Core\" {\n  component \"Web\" as W\n}\nW --> A\n@enduml\n";
    let spec = plantuml_to_spec(source).expect("the component subset should lower");
    assert_eq!(spec.nodes[0].id, "W");
    assert_eq!(spec.nodes[0].label.as_deref(), Some("Web"));
    assert_eq!(spec.clusters[0].label.as_deref(), Some("Core"));
}
