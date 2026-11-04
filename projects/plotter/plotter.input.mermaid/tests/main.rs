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
    let source = "stateDiagram-v2\n  [*] --> Still\n";
    let error = mermaid_to_scene(source).expect_err("state diagrams have no Oak AST");

    assert_eq!(error.code(), codes::UNSUPPORTED);
    let span = error.span().expect("the refusal names the header");
    assert_eq!(&source[span], "stateDiagram-v2");
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
