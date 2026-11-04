//! Mermaid 前端冒烟：flowchart 子集降到 DiagramSpec 再出 scene。

use plotter_diagram::spec::NodeShape;
use plotter_input_mermaid::mermaid_to_scene;
use plotter_scene::codes;

#[test]
fn flowchart_source_lowers_end_to_end() {
    let source = "flowchart TD\n  A[Start] --> B[Stop]\n";
    let lowered = mermaid_to_scene(source).expect("flowchart should lower");

    assert_eq!(lowered.plan.nodes.len(), 2);
    assert_eq!(lowered.plan.edges.len(), 1);
    assert_eq!(lowered.plan.node("A").expect("A").label, "Start");
    assert_eq!(lowered.plan.node("B").expect("B").shape, NodeShape::Box);
    assert!(lowered.diagnostics.is_empty(), "a clean flowchart must not report loss");

    let svg = lowered.scene.to_svg().expect("scene should serialize");
    assert!(svg.contains(">Start</text>"));
}

#[test]
fn unsupported_sequence_statements_fall_back_to_source() {
    let source = "sequenceDiagram\n    A->>B: ping\n    Note over A,B: shared\n";
    let error = mermaid_to_scene(source).expect_err("notes are not lowered");

    assert_eq!(error.code(), codes::UNSUPPORTED);
    let span = error.span().expect("a sequence refusal names the statement");
    assert_eq!(&source[span], "Note over A,B: shared");
}

#[test]
fn skipped_sequence_statement_with_arrow_text_is_refused() {
    let source = "sequenceDiagram\n    Note over A,B: click -> here\n";
    let error = mermaid_to_scene(source).expect_err("a skipped note must not pass on the arrow in its text");

    assert_eq!(error.code(), codes::UNSUPPORTED);
    let span = error.span().expect("the refusal names the note");
    assert_eq!(&source[span], "Note over A,B: click -> here");
}

#[test]
fn diagram_family_without_an_ast_is_refused_at_its_header() {
    let source = "gantt\n  title Plan\n  section Work\n  Task :a1, 2024-01-01, 30d\n";
    let error = mermaid_to_scene(source).expect_err("gantt diagrams have no Oak AST");

    assert_eq!(error.code(), codes::UNSUPPORTED);
    let span = error.span().expect("the refusal names the header");
    assert_eq!(&source[span], "gantt");
}

#[test]
fn state_diagram_lowers_states_and_transitions() {
    let source = "stateDiagram-v2\n    [*] --> Idle\n    Idle --> Busy: start\n    Busy --> [*]\n";
    let lowered = mermaid_to_scene(source).expect("a state diagram should lower");

    assert_eq!(lowered.plan.nodes.len(), 4);
    assert_eq!(lowered.plan.edges.len(), 3);
    assert_eq!(lowered.plan.node("Idle").expect("Idle").shape, NodeShape::Round);
    assert_eq!(lowered.plan.node("[*]:start").expect("initial state").label, "[*]");
    assert_eq!(lowered.plan.edges[1].label.as_deref(), Some("start"));
    assert!(lowered.diagnostics.is_empty(), "a clean state diagram must not report loss");
}

#[test]
fn composite_state_region_keeps_its_own_initial_state() {
    let source = "stateDiagram-v2\n    state Active {\n        [*] --> Running\n        Running --> Paused: pause\n    }\n";
    let lowered = mermaid_to_scene(source).expect("a composite state should lower");

    assert_eq!(lowered.plan.clusters.len(), 1, "one region becomes one frame");
    assert_eq!(lowered.plan.node("[*]:Active:region0:start").expect("region initial state").label, "[*]");
    assert_eq!(lowered.plan.node("Running").expect("Running").cluster.as_deref(), Some("Active:region0"));
}

#[test]
fn state_note_is_refused_with_its_authored_line() {
    // Byte-for-byte the `diagram-mermaid-state-note` corpus case, so the
    // asserted slice is the same string the harness compares against.
    let source = "stateDiagram-v2\n  [*] --> Idle\n  note right of Idle: waiting\n";
    let error = mermaid_to_scene(source).expect_err("a note has no Oak AST carrier");

    assert_eq!(error.code(), codes::UNSUPPORTED);
    let span = error.span().expect("the refusal names the note");
    assert_eq!(&source[span], "note right of Idle: waiting");
}

#[test]
fn entity_relationship_diagram_lowers_entities_and_cardinality() {
    let source = "erDiagram\n    CUSTOMER {\n        string id PK\n        string name\n    }\n    ORDER {\n        int id PK\n    }\n    CUSTOMER ||--o{ ORDER : places\n";
    let lowered = mermaid_to_scene(source).expect("an entity relationship diagram should lower");

    let customer = lowered.plan.node("CUSTOMER").expect("CUSTOMER");
    assert!(customer.label.contains("string id PK"), "got {}", customer.label);
    assert!(customer.label.contains("string name"), "got {}", customer.label);
    assert_eq!(lowered.plan.edges.len(), 1);
    assert_eq!(lowered.plan.edges[0].label.as_deref(), Some("|| places o{"));
    assert_eq!(lowered.plan.edges[0].style, plotter_diagram::spec::EdgeStyle::Line);
}

#[test]
fn class_method_members_keep_their_parentheses() {
    let source = "classDiagram\n  class Hub {\n    +run()\n  }\n";
    let lowered = mermaid_to_scene(source).expect("a class body should lower");

    let hub = lowered.plan.node("Hub").expect("Hub");
    assert!(hub.label.contains("+run()"), "got {}", hub.label);
}

#[test]
fn activation_shorthand_is_refused_with_the_message_span() {
    let source = "sequenceDiagram\n    A->>+B: ping\n";
    let error = mermaid_to_scene(source).expect_err("the activation suffix has no AST carrier");

    assert_eq!(error.code(), codes::UNSUPPORTED);
    let span = error.span().expect("the refusal names the message");
    assert_eq!(&source[span], "A->>+B: ping");
}
