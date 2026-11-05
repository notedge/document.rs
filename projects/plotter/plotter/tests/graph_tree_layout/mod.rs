//! 图 / 树布局 → Scene → CPU/SVG。

use apollo::{
    CircularLayout, CompileOptions, CoordinateSpec, ForceLayout, GraphData, GraphEdge, GraphLayout, GraphLayoutKind, GraphNode,
    GridLayout, LayerSpec, LayeredLayout, LayoutOptions, PlotSpec, RadialTreeLayout, SceneNodeKind, TidyTreeLayout, TreeData,
    TreeLayout, TreeLayoutKind, TreeNode, compile_plot, render_rgba8, render_svg,
};

fn sample_graph() -> GraphData {
    GraphData::undirected(
        vec![GraphNode::new("a"), GraphNode::new("b"), GraphNode::new("c"), GraphNode::new("d")],
        vec![GraphEdge::new("a", "b"), GraphEdge::new("b", "c"), GraphEdge::new("c", "d"), GraphEdge::new("a", "d")],
    )
    .unwrap()
}

fn sample_tree() -> TreeData {
    TreeData::new(
        "r",
        vec![
            TreeNode::root("r"),
            TreeNode::child("a", "r"),
            TreeNode::child("b", "r"),
            TreeNode::child("c", "a"),
            TreeNode::child("d", "a"),
        ],
    )
    .unwrap()
}

#[test]
fn graph_layouts_are_deterministic() {
    let g = sample_graph();
    let opt = LayoutOptions::new(200.0, 150.0, 20.0);
    assert_eq!(CircularLayout.layout(&g, &opt).unwrap(), CircularLayout.layout(&g, &opt).unwrap());
    assert_eq!(GridLayout.layout(&g, &opt).unwrap(), GridLayout.layout(&g, &opt).unwrap());
    assert_eq!(LayeredLayout.layout(&g, &opt).unwrap(), LayeredLayout.layout(&g, &opt).unwrap());
    let force = ForceLayout { iterations: 40 };
    assert_eq!(force.layout(&g, &opt).unwrap(), force.layout(&g, &opt).unwrap());
}

#[test]
fn tree_layouts_are_deterministic() {
    let t = sample_tree();
    let opt = LayoutOptions::new(200.0, 150.0, 20.0);
    let tidy_a = TidyTreeLayout.layout(&t, &opt).unwrap();
    let tidy_b = TidyTreeLayout.layout(&t, &opt).unwrap();
    assert_eq!(tidy_a, tidy_b);
    let radial_a = RadialTreeLayout.layout(&t, &opt).unwrap();
    let radial_b = RadialTreeLayout.layout(&t, &opt).unwrap();
    assert_eq!(radial_a, radial_b);
    assert!(tidy_a.position_of("r").unwrap().y > tidy_a.position_of("c").unwrap().y);
}

#[test]
fn graph_plot_renders() {
    let plot = PlotSpec::from_graph(sample_graph())
        .coordinates(CoordinateSpec::graph_space_with(GraphLayoutKind::Circular))
        .layer(LayerSpec::geom_edge())
        .layer(LayerSpec::geom_node());
    let scene = compile_plot(&plot, CompileOptions::golden()).unwrap();
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Points(_))));
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Polyline(_))));
    let image = render_rgba8(&scene).unwrap();
    assert!(image.non_white_count() > 0);
    let svg = render_svg(&scene).unwrap();
    assert!(svg.contains("<polyline") || svg.contains("<circle"));
}

#[test]
fn tree_plot_renders_radial() {
    let plot = PlotSpec::from_tree(sample_tree())
        .coordinates(CoordinateSpec::tree_space_with(TreeLayoutKind::Radial))
        .layer(LayerSpec::geom_tree_edge())
        .layer(LayerSpec::geom_tree_node());
    let scene = compile_plot(&plot, CompileOptions::golden()).unwrap();
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Points(_))));
    let image = render_rgba8(&scene).unwrap();
    assert!(image.non_white_count() > 0);
}
