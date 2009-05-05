//! SVG 后端冒烟。

use plotter_data::ColumnTable;
use plotter_grammar::{CompileOptions, LayerSpec, Mapping, PlotSpec, compile_plot};
use plotter_renderer_svg::render_svg;

#[test]
fn svg_contains_polyline_and_axes() {
    let table = ColumnTable::new().push_float("x", vec![0.0, 1.0, 2.0]).unwrap().push_float("y", vec![0.0, 2.0, 1.0]).unwrap();
    let plot = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_line());
    let scene = compile_plot(&plot, CompileOptions::default()).unwrap();
    let svg = render_svg(&scene).unwrap();
    assert!(svg.contains("<svg"));
    assert!(svg.contains("<polyline"));
    assert!(svg.matches("<line").count() >= 2);
}
