//! surface / point cloud / picking。

use apollo::{
    ColumnTable, CompileOptions, CoordinateSpec, GridData, LayerSpec, Mapping, PlotSpec, SceneNodeKind, compile_plot, pick_at,
    render_rgba8, render_svg,
};

fn sample_grid() -> GridData {
    let xs: Vec<f64> = (-2..=2).map(|i| i as f64 * 0.5).collect();
    let ys = xs.clone();
    let mut z = Vec::with_capacity(xs.len() * ys.len());
    for &y in &ys {
        for &x in &xs {
            z.push((x * x + y * y).sin());
        }
    }
    GridData::new(xs, ys, z).unwrap()
}

#[test]
fn surface_compiles_renders_and_picks() {
    let plot = PlotSpec::from_grid(sample_grid()).layer(LayerSpec::geom_surface());
    let scene = compile_plot(&plot, CompileOptions::golden()).unwrap();
    assert!(!scene.camera.is_2d());
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Mesh3(_))));

    let image = render_rgba8(&scene).unwrap();
    assert!(image.non_white_count() > 0);

    let svg = render_svg(&scene).unwrap();
    assert!(svg.contains("<polygon"));

    let hit = pick_at(&scene, 100.0, 75.0).unwrap();
    assert!(hit.is_some(), "expected surface pick near viewport center");
}

#[test]
fn point_cloud_compiles_and_renders() {
    let table = ColumnTable::new()
        .push_float("x", vec![0.0, 0.5, -0.5])
        .unwrap()
        .push_float("y", vec![0.0, 0.4, -0.3])
        .unwrap()
        .push_float("z", vec![0.0, 0.2, 0.1])
        .unwrap();
    let plot = PlotSpec::new(table)
        .mapping(Mapping::xyz("x", "y", "z"))
        .coordinates(CoordinateSpec::cartesian_3d())
        .layer(LayerSpec::geom_point3d());
    let scene = compile_plot(&plot, CompileOptions::golden()).unwrap();
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Points3(_))));
    let image = render_rgba8(&scene).unwrap();
    assert!(image.non_white_count() > 0);
}
