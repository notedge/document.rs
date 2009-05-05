//! WGPU 离屏冒烟（无 GPU 时自动跳过）。

use plotter_data::ColumnTable;
use plotter_grammar::{CompileOptions, LayerSpec, Mapping, PlotSpec, compile_plot};
use plotter_renderer::render_rgba8;
use plotter_renderer_wgpu::{WgpuRenderer, render_rgba8_wgpu};

fn sample_scene() -> plotter_view::Scene {
    let table = ColumnTable::new().push_float("x", vec![0.0, 1.0, 2.0]).unwrap().push_float("y", vec![0.0, 2.0, 1.0]).unwrap();
    let plot = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_line());
    compile_plot(&plot, CompileOptions::default()).unwrap()
}

#[test]
fn wgpu_offscreen_draws_when_adapter_available() {
    if !WgpuRenderer::is_available() {
        eprintln!("skip: no GPU adapter");
        return;
    }
    let scene = sample_scene();
    let image = render_rgba8_wgpu(&scene).expect("wgpu render");
    assert_eq!(image.width, 640);
    assert_eq!(image.height, 480);
    assert!(image.non_white_count() > 50, "expected GPU primitives");
}

#[test]
fn wgpu_and_cpu_both_draw_ink() {
    if !WgpuRenderer::is_available() {
        eprintln!("skip: no GPU adapter");
        return;
    }
    let scene = sample_scene();
    let cpu = render_rgba8(&scene).unwrap();
    let gpu = render_rgba8_wgpu(&scene).unwrap();
    assert!(cpu.non_white_count() > 0);
    assert!(gpu.non_white_count() > 0);
}
