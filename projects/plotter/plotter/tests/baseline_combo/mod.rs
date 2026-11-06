//! scale / coord / facet / theme 基线组合。

use plotter::{
    data::ColumnTable,
    grammar::{CompileOptions, CoordinateSpec, FacetSpec, LayerSpec, Mapping, PlotSpec, ScaleSpec, ThemeSpec, compile_plot},
    types::plot::Interval,
};

#[test]
fn ggplot_baseline_combo() {
    let table = ColumnTable::new()
        .push_float("x", vec![1.0, 10.0, 1.0, 10.0])
        .unwrap()
        .push_float("y", vec![1.0, 100.0, 10.0, 1000.0])
        .unwrap()
        .push_string("g", vec!["a".into(), "a".into(), "b".into(), "b".into()])
        .unwrap();

    let plot = PlotSpec::new(table)
        .mapping(Mapping::xy("x", "y"))
        .scale(ScaleSpec::log10_y())
        .coordinates(CoordinateSpec::cartesian_limits(Some(Interval::new(1.0, 10.0)), None))
        .facets(FacetSpec::wrap_ncol("g", 2))
        .theme(ThemeSpec::light())
        .layer(LayerSpec::geom_point());

    let scene = compile_plot(&plot, CompileOptions::golden()).unwrap();
    assert!(scene.nodes.nodes().len() > 4);
}
