#![allow(missing_docs)]

use plotter_data::ColumnTable;
use plotter_grammar::{
    AestheticExpr, CompileOptions, CoordinateSpec, FacetSpec, LayerSpec, Mapping, PlotSpec, ScaleSpec, ThemeSpec, compile_plot,
};
use plotter_types::plot::Interval;
use plotter_view::{Scene, SceneNodeKind};

#[test]
fn compiles_point_bar_text() {
    let table = ColumnTable::new()
        .push_float("x", vec![0.0, 1.0, 2.0])
        .unwrap()
        .push_float("y", vec![1.0, 2.0, 1.5])
        .unwrap()
        .push_string("lab", vec!["a".into(), "b".into(), "c".into()])
        .unwrap();

    let point = compile_plot(
        &PlotSpec::new(table.clone()).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_point()),
        CompileOptions::golden(),
    )
    .unwrap();
    assert!(point.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Points(_))));

    let bar = compile_plot(
        &PlotSpec::new(table.clone()).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_bar()),
        CompileOptions::golden(),
    )
    .unwrap();
    assert!(bar.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Mesh(_))));

    let mut mapping = Mapping::xy("x", "y");
    mapping.label = Some(AestheticExpr::column("lab"));
    let text =
        compile_plot(&PlotSpec::new(table).mapping(mapping).layer(LayerSpec::geom_text()), CompileOptions::golden()).unwrap();
    assert!(text.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Text(_))));
}

#[test]
fn log_y_midpoint_differs_from_linear() {
    let table =
        ColumnTable::new().push_float("x", vec![0.0, 0.5, 1.0]).unwrap().push_float("y", vec![1.0, 10.0, 100.0]).unwrap();
    let linear = compile_plot(
        &PlotSpec::new(table.clone()).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_point()),
        CompileOptions::golden(),
    )
    .unwrap();
    let log = compile_plot(
        &PlotSpec::new(table).mapping(Mapping::xy("x", "y")).scale(ScaleSpec::log10_y()).layer(LayerSpec::geom_point()),
        CompileOptions::golden(),
    )
    .unwrap();
    let mid_linear = nth_point_y(&linear, 1);
    let mid_log = nth_point_y(&log, 1);
    assert!((mid_linear - mid_log).abs() > 1.0, "log mid should not match linear mid");
}

#[test]
fn coord_xlim_clips_domain() {
    let table = ColumnTable::new().push_float("x", vec![0.0, 10.0]).unwrap().push_float("y", vec![0.0, 1.0]).unwrap();
    let scene = compile_plot(
        &PlotSpec::new(table)
            .mapping(Mapping::xy("x", "y"))
            .coordinates(CoordinateSpec::cartesian_limits(Some(Interval::new(0.0, 5.0)), None))
            .layer(LayerSpec::geom_line()),
        CompileOptions::golden(),
    )
    .unwrap();
    let axis = scene.nodes.nodes().iter().find_map(|n| match &n.kind {
        SceneNodeKind::Axis(axis) if axis.horizontal => Some(axis),
        _ => None,
    });
    assert_eq!(axis.unwrap().domain, Interval::new(0.0, 5.0));
}

#[test]
fn facet_wrap_builds_two_panels() {
    let table = ColumnTable::new()
        .push_float("x", vec![0.0, 1.0, 0.0, 1.0])
        .unwrap()
        .push_float("y", vec![1.0, 2.0, 3.0, 4.0])
        .unwrap()
        .push_string("g", vec!["a".into(), "a".into(), "b".into(), "b".into()])
        .unwrap();
    let scene = compile_plot(
        &PlotSpec::new(table)
            .mapping(Mapping::xy("x", "y"))
            .facets(FacetSpec::wrap_ncol("g", 2))
            .layer(LayerSpec::geom_point()),
        CompileOptions::golden(),
    )
    .unwrap();
    let texts: Vec<_> = scene
        .nodes
        .nodes()
        .iter()
        .filter_map(|n| match &n.kind {
            SceneNodeKind::Text(t) => Some(t.content.as_str()),
            _ => None,
        })
        .collect();
    assert!(texts.contains(&"a"));
    assert!(texts.contains(&"b"));
    let point_nodes = scene.nodes.nodes().iter().filter(|n| matches!(n.kind, SceneNodeKind::Points(_))).count();
    assert_eq!(point_nodes, 2);
}

#[test]
fn dark_theme_uses_foreground() {
    let table = ColumnTable::new().push_float("x", vec![0.0, 1.0]).unwrap().push_float("y", vec![1.0, 2.0]).unwrap();
    let scene = compile_plot(
        &PlotSpec::new(table).mapping(Mapping::xy("x", "y")).theme(ThemeSpec::dark()).layer(LayerSpec::geom_line()),
        CompileOptions::golden(),
    )
    .unwrap();
    let stroke = scene.nodes.nodes().iter().find_map(|n| match &n.kind {
        SceneNodeKind::Polyline(p) => Some(p.stroke),
        _ => None,
    });
    assert_eq!(stroke.unwrap(), ThemeSpec::dark().foreground);
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Mesh(_))));
}

fn nth_point_y(scene: &Scene, index: usize) -> f64 {
    scene
        .nodes
        .nodes()
        .iter()
        .find_map(|n| match &n.kind {
            SceneNodeKind::Points(p) => Some(p.positions[index].y),
            _ => None,
        })
        .unwrap()
}
