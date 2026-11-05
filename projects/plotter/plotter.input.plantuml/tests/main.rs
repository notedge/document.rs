//! PlantUML 前端冒烟：class 家族降到共用 DiagramSpec，越界指令结构化拒绝。

use std::ops::Range;

use plotter_diagram::{
    DiagramError,
    spec::{DiagramNode, NodeShape},
};
use plotter_input_plantuml::{plantuml_to_scene, plantuml_to_spec};
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
fn composite_state_regions_become_labelled_frames_with_an_approximation() {
    let source = "@startuml\n[*] --> Running\nstate Running {\n  Fast --> Slow\n}\n@enduml\n";
    let lowered = plantuml_to_scene(source).expect("a composite state lowers with a named loss");

    let frames: Vec<_> = lowered.plan.clusters.iter().filter(|cluster| cluster.label.as_deref() == Some("Running")).collect();
    assert_eq!(frames.len(), 1, "one region becomes one named frame");
    assert_eq!(lowered.plan.node("Fast").expect("Fast").cluster.as_deref(), Some(frames[0].id.as_str()));

    let records: Vec<_> = lowered.diagnostics.iter().filter(|record| record.code().as_str() == codes::APPROXIMATED).collect();
    assert_eq!(records.len(), 1, "{:?}", lowered.diagnostics);
    assert!(format!("{:?}", records[0]).contains("Running"), "the approximation names the state: {:?}", records[0]);
}

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
    assert_eq!(spec.nodes, vec![node("W", Some("Web"), NodeShape::Box), node("A", Some("Api"), NodeShape::Box)]);
}

#[test]
fn quoted_component_and_package_names_keep_the_display_text() {
    let source = "@startuml\npackage \"Core\" {\n  component \"Web\" as W\n}\nW --> A\n@enduml\n";
    let spec = plantuml_to_spec(source).expect("the component subset should lower");
    assert_eq!(spec.nodes[0].id, "W");
    assert_eq!(spec.nodes[0].label.as_deref(), Some("Web"));
    assert_eq!(spec.clusters[0].label.as_deref(), Some("Core"));
}
