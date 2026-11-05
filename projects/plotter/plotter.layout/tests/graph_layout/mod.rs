#![allow(missing_docs)]

use plotter_data::{GraphData, GraphEdge, GraphNode};
use plotter_layout::{CircularLayout, GraphLayout, LayeredLayout, LayoutOptions};

fn path_graph() -> GraphData {
    GraphData::directed(
        vec![GraphNode::new("a"), GraphNode::new("b"), GraphNode::new("c")],
        vec![GraphEdge::new("a", "b"), GraphEdge::new("b", "c")],
    )
    .unwrap()
}

#[test]
fn circular_is_deterministic() {
    let g = path_graph();
    let opt = LayoutOptions::new(200.0, 200.0, 20.0);
    let a = CircularLayout.layout(&g, &opt).unwrap();
    let b = CircularLayout.layout(&g, &opt).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.positions.len(), 3);
    assert_eq!(a.routes.len(), 2);
}

#[test]
fn layered_orders_sources_on_top() {
    let g = path_graph();
    let result = LayeredLayout.layout(&g, &LayoutOptions::new(200.0, 200.0, 20.0)).unwrap();
    let ya = result.position_of("a").unwrap().y;
    let yc = result.position_of("c").unwrap().y;
    assert!(ya > yc);
}
