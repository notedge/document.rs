//! Mermaid 前端冒烟：flowchart 子集降到 DiagramSpec 再出 scene。

use plotter_diagram::spec::NodeShape;
use plotter_input_mermaid::{mermaid_to_scene, mermaid_to_spec};
use plotter_scene::codes;

#[test]
fn composite_state_regions_become_labelled_frames_with_an_approximation() {
    let source = "stateDiagram-v2\n  [*] --> Running\n  state Running {\n    Fast --> Slow : settle\n  }\n";
    let lowered = mermaid_to_scene(source).expect("a composite state lowers with a named loss");

    // The outer state keeps its own node as the transition endpoint.
    assert_eq!(lowered.plan.nodes.len(), 4, "{:?}", lowered.plan.nodes.iter().map(|node| &node.id).collect::<Vec<_>>());
    assert!(lowered.plan.node("Running").is_some());

    // Each region is a frame titled with the state label, and its members point at it.
    let frames: Vec<_> = lowered.plan.clusters.iter().filter(|cluster| cluster.label.as_deref() == Some("Running")).collect();
    assert_eq!(frames.len(), 1, "one region becomes one named frame");
    assert_eq!(lowered.plan.node("Fast").expect("Fast").cluster.as_deref(), Some(frames[0].id.as_str()));
    assert_eq!(lowered.plan.node("Slow").expect("Slow").cluster.as_deref(), Some(frames[0].id.as_str()));

    // The flattened containment is recorded instead of dropped silently.
    let records: Vec<_> = lowered.diagnostics.iter().filter(|record| record.code().as_str() == codes::APPROXIMATED).collect();
    assert_eq!(records.len(), 1, "{:?}", lowered.diagnostics);
    assert!(format!("{:?}", records[0]).contains("Running"), "the approximation names the state: {:?}", records[0]);
}

/// The `approximated` record marks a missing capability rather than a property
/// of composite states, so it must disappear once the region frame nests inside
/// the state frame. Enable this test when `DiagramLayoutPlan` gains nested
/// frames and the region cluster becomes the state node's child.
#[test]
#[ignore = "`DiagramLayoutPlan` has no nested frame yet"]
fn composite_state_regions_stop_reporting_the_approximation_once_nested_frames_land() {
    let source = "stateDiagram-v2\n  [*] --> Running\n  state Running {\n    Fast --> Slow : settle\n  }\n";
    let lowered = mermaid_to_scene(source).expect("a composite state still lowers");

    let records: Vec<_> = lowered.diagnostics.iter().filter(|record| record.code().as_str() == codes::APPROXIMATED).collect();
    assert!(records.is_empty(), "containment is now represented: {:?}", records);
}

#[test]
fn an_unsupported_family_reports_one_code_from_both_entries() {
    let source = "gantt\n  title Plan\n  section A\n  Task :a1, 2024-01-01, 3d\n";

    let spec_error = mermaid_to_spec(source).expect_err("gantt has no AST");
    let scene_error = mermaid_to_scene(source).expect_err("gantt has no AST");

    assert_eq!(spec_error.code(), codes::UNSUPPORTED);
    assert_eq!(scene_error.code(), spec_error.code(), "both public entries agree on the code");
    assert_eq!(scene_error.span(), spec_error.span(), "both public entries agree on the span");
    assert_eq!(&source[scene_error.span().expect("a family refusal names its header")], "gantt");
}

#[test]
fn flowchart_source_lowers_end_to_end() {
    let source = "flowchart TD\n  A[Start] --> B[Stop]\n";
    let lowered = mermaid_to_scene(source).expect("flowchart should lower");

    assert_eq!(lowered.plan.nodes.len(), 2);
    assert_eq!(lowered.plan.edges.len(), 1);
    assert_eq!(lowered.plan.node("B").expect("B").shape, NodeShape::Box);

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
