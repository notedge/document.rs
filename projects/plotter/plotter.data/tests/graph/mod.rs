#![allow(missing_docs)]

use plotter_data::{GraphData, GraphEdge, GraphNode};
use plotter_types::plot::DiagnosticCode;

#[test]
fn rejects_unknown_endpoint() {
    let err = GraphData::undirected(vec![GraphNode::new("a")], vec![GraphEdge::new("a", "b")]).unwrap_err();
    assert_eq!(err.code, DiagnosticCode::UnknownColumn);
}
