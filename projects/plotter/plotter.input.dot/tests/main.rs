//! DOT 前端冒烟：白名单子集可降到 DiagramSpec，其余以结构化诊断拒绝。

use std::ops::Range;

use plotter_diagram::{
    DiagramError,
    spec::{DiagramNode, EdgeStyle, NodeShape},
};
use plotter_input_dot::dot_to_spec;
use plotter_types::codes;

fn span_of<'a>(source: &'a str, error: &DiagramError) -> &'a str {
    let span = error.span().unwrap_or_else(|| panic!("`{}` carries no span", error.message()));
    let Range { start, end } = span.clone();
    assert!(end <= source.len(), "span {start}..{end} leaves a {} byte source", source.len());
    &source[start..end]
}

fn node(id: &str) -> DiagramNode {
    DiagramNode { id: id.to_string(), label: None, shape: NodeShape::Box, ports: Vec::new(), cluster: None }
}

#[test]
fn digraph_rankdir_and_nodes_lower_to_one_spec() {
    let source = r#"digraph pipeline {
  rankdir=LR;
  A [label="Start", shape=circle];
}"#;
    let spec = dot_to_spec(source).expect("the subset should lower");

    assert_eq!(spec.nodes, vec![DiagramNode { label: Some("Start".to_string()), shape: NodeShape::Circle, ..node("A") }]);
}

#[test]
fn html_labels_are_rejected_with_the_attribute_span() {
    let source = "digraph {\n  A [label=<b>x</b>];\n}";
    let error = dot_to_spec(source).expect_err("HTML labels are outside the subset");
    assert_eq!(error.code(), codes::UNSUPPORTED);
    assert!(span_of(source, &error).contains("<b>x</b>"));
}

#[test]
fn graphviz_shapes_lower_to_native_variants() {
    let source = r#"digraph G {
  a [shape=box]; b [shape=circle]; c [shape=diamond]; d [shape=cylinder];
  e [shape=note]; f [shape=plaintext]; g [shape=point]; h [shape=triangle];
  i [shape=hexagon]; j [shape=parallelogram]; k [shape=trapezium]; l [shape=star];
  m [shape=ellipse]; n [shape=doublecircle]; o [shape=component];
}"#;
    let spec = dot_to_spec(source).expect("every listed shape is native");

    let shapes: Vec<NodeShape> = spec.nodes.iter().map(|node| node.shape).collect();
    assert_eq!(
        shapes,
        vec![
            NodeShape::Box,
            NodeShape::Circle,
            NodeShape::Rhombus,
            NodeShape::Cylinder,
            NodeShape::Note,
            NodeShape::Plaintext,
            NodeShape::Point,
            NodeShape::Triangle,
            NodeShape::Hexagon,
            NodeShape::Parallelogram,
            NodeShape::Trapezium,
            NodeShape::Star,
            NodeShape::Ellipse,
            NodeShape::DoubleCircle,
            NodeShape::Component,
        ]
    );
}

#[test]
fn dashed_dotted_and_bold_edges_lower_without_approximation() {
    let source = r#"digraph G {
  a -> b [style=dashed];
  b -> c [style=dotted];
  c -> d [style=bold];
}"#;
    let lowered = plotter_input_dot::dot_to_scene(source).expect("the styles are part of the contract");

    let styles: Vec<EdgeStyle> = lowered.plan.edges.iter().map(|edge| edge.style).collect();
    assert_eq!(styles, vec![EdgeStyle::DashedArrow, EdgeStyle::DottedArrow, EdgeStyle::BoldArrow]);
    assert!(
        lowered.diagnostics.iter().all(|record| record.code().as_str() != codes::APPROXIMATED),
        "dash and bold styles must not be approximated: {:?}",
        lowered.diagnostics
    );
}
