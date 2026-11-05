//! 门面级冒烟：构造、编译并渲染 2D 折线图。

use apollo::{
    ColumnTable, CompileOptions, LayerSpec, Mapping, PlotSpec, SceneNodeKind, compile_plot, render_rgba8, render_svg,
    validate_plot,
};

#[test]
fn facade_validates_line_plot() {
    let table = ColumnTable::new().push_float("x", vec![0.0, 1.0, 2.0]).unwrap().push_float("y", vec![1.0, 2.0, 3.0]).unwrap();
    let plot = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_line());
    validate_plot(&plot).unwrap();
}

#[test]
fn facade_compiles_line_plot_to_scene() {
    let table = ColumnTable::new().push_float("x", vec![0.0, 1.0, 2.0]).unwrap().push_float("y", vec![1.0, 2.0, 3.0]).unwrap();
    let plot = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_line());
    let scene = compile_plot(&plot, CompileOptions::default()).unwrap();
    let root = scene.nodes.get(scene.root).unwrap();
    assert!(matches!(root.kind, SceneNodeKind::Group { .. }));
}

#[test]
fn facade_renders_cpu_and_svg() {
    let table = ColumnTable::new().push_float("x", vec![0.0, 1.0, 2.0]).unwrap().push_float("y", vec![1.0, 2.0, 3.0]).unwrap();
    let plot = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_line());
    let scene = compile_plot(&plot, CompileOptions::default()).unwrap();
    let image = render_rgba8(&scene).unwrap();
    assert!(image.non_white_count() > 0);
    let svg = render_svg(&scene).unwrap();
    assert!(svg.contains("<polyline"));
}

#[cfg(feature = "wgpu")]
#[test]
fn facade_renders_wgpu_when_available() {
    use apollo::{WgpuRenderer, render_rgba8_wgpu};
    if !WgpuRenderer::is_available() {
        return;
    }
    let table = ColumnTable::new().push_float("x", vec![0.0, 1.0, 2.0]).unwrap().push_float("y", vec![1.0, 2.0, 3.0]).unwrap();
    let plot = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_line());
    let scene = compile_plot(&plot, CompileOptions::default()).unwrap();
    let image = render_rgba8_wgpu(&scene).unwrap();
    assert!(image.non_white_count() > 0);
}
