#![allow(missing_docs)]

use plotter_data::{GraphData, GraphEdge, GraphNode, TreeData, TreeNode};
use plotter_grammar::{CompileOptions, LayerSpec, PlotSpec, compile_plot};
use plotter_view::SceneNodeKind;

#[test]
fn compiles_graph_circular() {
    let graph = GraphData::undirected(
        vec![GraphNode::new("a"), GraphNode::new("b"), GraphNode::new("c")],
        vec![GraphEdge::new("a", "b"), GraphEdge::new("b", "c")],
    )
    .unwrap();
    let scene = compile_plot(
        &PlotSpec::from_graph(graph).layer(LayerSpec::geom_edge()).layer(LayerSpec::geom_node()),
        CompileOptions::golden(),
    )
    .unwrap();
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Points(_))));
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Polyline(_))));
}

#[test]
fn compiles_tidy_tree() {
    let tree = TreeData::new("r", vec![TreeNode::root("r"), TreeNode::child("a", "r"), TreeNode::child("b", "r")]).unwrap();
    let scene = compile_plot(
        &PlotSpec::from_tree(tree).layer(LayerSpec::geom_tree_edge()).layer(LayerSpec::geom_tree_node()),
        CompileOptions::golden(),
    )
    .unwrap();
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Points(_))));
}
