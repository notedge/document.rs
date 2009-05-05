//! CPU 渲染冒烟。

use plotter_data::ColumnTable;
use plotter_grammar::{CompileOptions, LayerSpec, Mapping, PlotSpec, compile_plot};
use plotter_renderer::render_rgba8;

#[test]
fn cpu_raster_draws_non_white_pixels() {
    let table = ColumnTable::new().push_float("x", vec![0.0, 1.0, 2.0]).unwrap().push_float("y", vec![0.0, 2.0, 1.0]).unwrap();
    let plot = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_line());
    let scene = compile_plot(&plot, CompileOptions::default()).unwrap();
    let image = render_rgba8(&scene).unwrap();
    assert_eq!(image.width, 640);
    assert_eq!(image.height, 480);
    assert!(image.non_white_count() > 100, "expected drawn primitives");
}
