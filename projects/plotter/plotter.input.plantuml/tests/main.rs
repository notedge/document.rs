//! PlantUML 前端冒烟：五个家族降到共用 DiagramSpec，无法映射的语句拒绝并指向源行。

use std::ops::Range;

use plotter_diagram::{
    DiagramError,
    spec::{DiagramNode, EdgeStyle, NodeShape},
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

const ACTIVITY_SOURCE: &str = r#"@startuml
start
:Read;
if (ready?) then (yes)
:Write;
else (no)
:Skip;
endif
stop
@enduml
"#;

#[test]
fn activity_actions_and_branches_lower_to_a_flow() {
    let spec = plantuml_to_spec(ACTIVITY_SOURCE).expect("the activity family should lower");

    let label = |text: &str| {
        spec.nodes.iter().find(|node| node.label.as_deref() == Some(text)).unwrap_or_else(|| panic!("missing node `{text}`"))
    };
    assert_eq!(label("start").shape, NodeShape::Circle);
    assert_eq!(label("Read").shape, NodeShape::Box);
    assert_eq!(label("ready?").shape, NodeShape::Rhombus);
    assert_eq!(label("stop").shape, NodeShape::Circle);

    let decision = label("ready?").id.as_str();
    let branch_labels: Vec<_> = spec
        .edges
        .iter()
        .filter(|edge| edge.from.node == decision)
        .map(|edge| edge.label.as_deref())
        .collect();
    assert_eq!(branch_labels, vec![Some("yes"), Some("no")], "each branch keeps the authored gate label");

    for edge in &spec.edges {
        assert_eq!(edge.style, EdgeStyle::Arrow, "activity flow uses plain arrows");
    }
}

#[test]
fn activity_swimlane_becomes_a_cluster_and_the_flow_crosses_it() {
    let source = "@startuml\nstart\n|Draft|\n:Write;\n|Review|\n:Check;\nstop\n@enduml\n";
    let spec = plantuml_to_spec(source).expect("swimlanes keep the flow");

    assert_eq!(spec.clusters.len(), 2, "one frame per swimlane");
    assert_eq!(spec.clusters[0].label.as_deref(), Some("Draft"));
    assert!(spec.clusters[1].members.iter().any(|member| member == &spec.nodes[2].id), "Review owns its action");
    let write = spec.nodes.iter().find(|node| node.label.as_deref() == Some("Write")).expect("Write lowers");
    let check = spec.nodes.iter().find(|node| node.label.as_deref() == Some("Check")).expect("Check lowers");
    assert!(
        spec.edges.iter().any(|edge| edge.from.node == write.id && edge.to.node == check.id),
        "the flow continues from the last action of one lane into the next lane"
    );
}

#[test]
fn activity_dropped_statement_is_refused_at_its_line() {
    let source = "@startuml\nstart\n:Read;\nnote right: draft\nstop\n@enduml\n";
    let error = plantuml_to_spec(source).expect_err("the activity builder drops notes");
    assert_eq!(error.code(), codes::UNSUPPORTED);
    assert_eq!(span_of(source, &error), "note right: draft");
}

const STATE_SOURCE: &str = r#"@startuml
state Idle
state Busy
[*] --> Idle
Idle --> Busy : start
Busy --> [*]
@enduml
"#;

#[test]
fn state_nodes_transitions_and_pseudo_states_lower() {
    let spec = plantuml_to_spec(STATE_SOURCE).expect("the state family should lower");

    let idle = spec.nodes.iter().find(|node| node.id == "Idle").expect("Idle lowers");
    assert_eq!(idle.shape, NodeShape::Round);
    let pseudo: Vec<_> = spec.nodes.iter().filter(|node| node.label.as_deref() == Some("[*]")).collect();
    assert_eq!(pseudo.len(), 2, "a start and an end pseudo state are distinct");
    assert!(pseudo.iter().all(|node| node.shape == NodeShape::Circle));
    assert_eq!(spec.edges.len(), 3);
    assert!(spec.edges.iter().any(|edge| edge.label.as_deref() == Some("start")));
}

#[test]
fn state_composite_regions_and_stereotypes_keep_their_outline() {
    let source = "@startuml\nstate Working {\n  [*] --> Run\n  Run --> [*]\n}\nstate Pick <<choice>>\n@enduml\n";
    let spec = plantuml_to_spec(source).expect("composite states lower");

    let frame = spec.clusters.iter().find(|cluster| cluster.id.starts_with("Working:region")).expect("a region frame");
    assert!(frame.members.iter().any(|member| member == "Run"));
    let pick = spec.nodes.iter().find(|node| node.id == "Pick").expect("Pick lowers");
    assert_eq!(pick.shape, NodeShape::Rhombus);
}

#[test]
fn state_note_is_refused_at_its_line() {
    let source = "@startuml\nstate Idle\nnote right of Idle: waiting\n@enduml\n";
    let error = plantuml_to_spec(source).expect_err("the state builder drops notes");
    assert_eq!(error.code(), codes::UNSUPPORTED);
    assert_eq!(span_of(source, &error), "note right of Idle: waiting");
}

const COMPONENT_SOURCE: &str = r#"@startuml
package core {
  component A
  component B
}
interface API
A --> B : uses
A ..> API : calls
@enduml
"#;

#[test]
fn component_packages_interfaces_and_relations_lower() {
    let spec = plantuml_to_spec(COMPONENT_SOURCE).expect("the component family should lower");

    let package = spec.clusters.iter().find(|cluster| cluster.label.as_deref() == Some("core")).expect("the package frame");
    assert!(package.members.iter().any(|member| member == "A"));
    assert!(package.members.iter().any(|member| member == "B"));
    let api = spec.nodes.iter().find(|node| node.id == "API").expect("API lowers");
    assert_eq!(api.label.as_deref(), Some("«interface» API"));
    let labels: Vec<_> = spec.edges.iter().map(|edge| edge.label.as_deref()).collect();
    assert_eq!(labels, vec![Some("uses"), Some("calls")]);
    assert_eq!(spec.edges[1].style, EdgeStyle::DottedArrow, "`..>` keeps its dotted arrow");
}

#[test]
fn component_relation_multiplicities_fold_into_the_edge_label() {
    let source = "@startuml\ncomponent A\ncomponent B\nA \"1\" --> \"many\" B : owns\n@enduml\n";
    let spec = plantuml_to_spec(source).expect("component multiplicities lower");
    assert_eq!(spec.edges[0].label.as_deref(), Some("1 many owns"));
}

#[test]
fn component_dropped_statement_is_refused_at_its_line() {
    let source = "@startuml\ncomponent A\ndatabase DB\n@enduml\n";
    let error = plantuml_to_spec(source).expect_err("the component AST has no database node");
    assert_eq!(error.code(), codes::UNSUPPORTED);
    assert_eq!(span_of(source, &error), "database DB");
}

#[test]
fn lowered_activity_scene_renders_svg() {
    let lowered = plantuml_to_scene(ACTIVITY_SOURCE).expect("the activity scene should resolve");
    let svg = lowered.scene.to_svg().expect("the scene should serialize");
    assert!(svg.contains("Read"), "the action label reaches the scene");
    assert!(svg.contains("Write"), "both branches reach the scene");
}

#[test]
fn class_or_sequence_dropped_statement_keeps_the_whole_authored_line() {
    let source = "@startuml\nskinparam monochrome true\nA -> B\n@enduml\n";
    let error = plantuml_to_spec(source).expect_err("the sequence builder drops skinparam");
    assert_eq!(error.code(), codes::UNSUPPORTED);
    assert_eq!(span_of(source, &error), "skinparam monochrome true");
}

#[test]
fn start_marker_must_be_a_delimited_keyword() {
    let source = "@startumlx\nclass A\n@enduml\n";
    let error = plantuml_to_spec(source).expect_err("`@startumlx` is not the start marker");

    assert_eq!(error.code(), codes::UNSUPPORTED);
    assert_eq!(span_of(source, &error), "@startumlx");
}
