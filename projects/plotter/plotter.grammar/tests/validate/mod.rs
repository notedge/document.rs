#![allow(missing_docs)]

use plotter_data::ColumnTable;
use plotter_grammar::{AestheticExpr, FacetSpec, LayerSpec, Mapping, PlotSpec, ScaleSpec, validate_plot};
use plotter_types::plot::{DiagnosticCode, Interval};

fn sample_table() -> ColumnTable {
    ColumnTable::new().push_float("x", vec![0.0, 1.0, 2.0]).unwrap().push_float("y", vec![0.0, 1.0, 0.0]).unwrap()
}

#[test]
fn accepts_point_and_bar() {
    let table = sample_table();
    let point = PlotSpec::new(table.clone()).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_point());
    let bar = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_bar());
    assert!(validate_plot(&point).is_ok());
    assert!(validate_plot(&bar).is_ok());
}

#[test]
fn accepts_text_with_label_column() {
    let table = ColumnTable::new()
        .push_float("x", vec![0.0, 1.0])
        .unwrap()
        .push_float("y", vec![1.0, 2.0])
        .unwrap()
        .push_string("lab", vec!["a".into(), "b".into()])
        .unwrap();
    let mut mapping = Mapping::xy("x", "y");
    mapping.label = Some(AestheticExpr::column("lab"));
    let plot = PlotSpec::new(table).mapping(mapping).layer(LayerSpec::geom_text());
    assert!(validate_plot(&plot).is_ok());
}

#[test]
fn rejects_missing_y() {
    let mut mapping = Mapping::default();
    mapping.x = Some(AestheticExpr::column("x"));
    let plot = PlotSpec::new(sample_table()).mapping(mapping).layer(LayerSpec::geom_line());
    let err = validate_plot(&plot).unwrap_err();
    assert_eq!(err.code, DiagnosticCode::MissingMapping);
}

#[test]
fn rejects_bar_with_log_y() {
    let plot = PlotSpec::new(sample_table())
        .mapping(Mapping::xy("x", "y"))
        .scale(ScaleSpec::log10_y().with_domain(Interval::new(1.0, 10.0)))
        .layer(LayerSpec::geom_bar());
    let err = validate_plot(&plot).unwrap_err();
    assert_eq!(err.code, DiagnosticCode::UnsupportedSpec);
}

#[test]
fn accepts_facet_wrap() {
    let table = ColumnTable::new()
        .push_float("x", vec![0.0, 1.0])
        .unwrap()
        .push_float("y", vec![1.0, 2.0])
        .unwrap()
        .push_string("g", vec!["a".into(), "b".into()])
        .unwrap();
    let plot = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).facets(FacetSpec::wrap("g")).layer(LayerSpec::geom_point());
    assert!(validate_plot(&plot).is_ok());
}

#[test]
fn accepts_surface_grid() {
    use plotter_data::GridData;

    let grid = GridData::new(vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0, 0.0, 0.0, 1.0]).unwrap();
    let plot = PlotSpec::from_grid(grid).layer(LayerSpec::geom_surface());
    assert!(validate_plot(&plot).is_ok());
}

#[test]
fn accepts_graph_and_tree() {
    use plotter_data::{GraphData, GraphEdge, GraphNode, TreeData, TreeNode};

    let graph = GraphData::undirected(vec![GraphNode::new("a"), GraphNode::new("b")], vec![GraphEdge::new("a", "b")]).unwrap();
    let gplot = PlotSpec::from_graph(graph).layer(LayerSpec::geom_edge()).layer(LayerSpec::geom_node());
    assert!(validate_plot(&gplot).is_ok());

    let tree = TreeData::new("r", vec![TreeNode::root("r"), TreeNode::child("a", "r")]).unwrap();
    let tplot = PlotSpec::from_tree(tree).layer(LayerSpec::geom_tree_edge()).layer(LayerSpec::geom_tree_node());
    assert!(validate_plot(&tplot).is_ok());
}
