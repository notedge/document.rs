//! DOT 前端冒烟：白名单子集可降到 DiagramSpec，其余以结构化诊断拒绝。

use std::ops::Range;

use plotter_diagram::{
    DiagramError,
    spec::{DiagramNode, NodeShape},
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
