//! PlantUML 前端冒烟：class 子集降到共用 DiagramSpec，其余拒绝并指向源行。

use std::ops::Range;

use plotter_diagram::{DiagramError, spec::DiagramNode, spec::NodeShape};
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
fn block_comments_are_skipped_before_the_refusal_scan() {
    let source = "@startuml\n/'\nnote over Order : draft\n'/\nclass Order {\n  +id : String\n}\n@enduml\n";
    let spec = plantuml_to_spec(source).expect("block comments carry no diagram meaning");

    assert_eq!(spec.nodes, vec![node("Order", Some("Order\n+id : String"), NodeShape::Box)]);
}

#[test]
fn class_keywords_survive_as_label_prefixes() {
    let source = "@startuml\ninterface Repo {\n  +find() : void\n}\n@enduml\n";
    let spec = plantuml_to_spec(source).expect("interfaces are part of the class subset");

    let label = spec.nodes[0].label.as_deref().expect("an interface keeps its name");
    assert!(label.starts_with("«interface» Repo"), "got {label}");
}

#[test]
fn start_marker_must_be_a_delimited_keyword() {
    let source = "@startumlx\nclass A\n@enduml\n";
    let error = plantuml_to_spec(source).expect_err("`@startumlx` is not the start marker");

    assert_eq!(error.code(), codes::UNSUPPORTED);
    assert_eq!(span_of(source, &error), "@startumlx");
}
