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
