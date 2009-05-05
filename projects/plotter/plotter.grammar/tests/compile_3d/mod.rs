#![allow(missing_docs)]

use plotter_data::GridData;
use plotter_grammar::{CompileOptions, LayerSpec, PlotSpec, compile_plot};
use plotter_view::SceneNodeKind;

#[test]
fn compiles_surface_mesh3() {
    let grid = GridData::new(vec![-1.0, 0.0, 1.0], vec![-1.0, 0.0, 1.0], {
        let mut z = Vec::new();
        for y in [-1.0, 0.0, 1.0] {
            for x in [-1.0, 0.0, 1.0] {
                z.push(x * y);
            }
        }
        z
    })
    .unwrap();
    let scene = compile_plot(&PlotSpec::from_grid(grid).layer(LayerSpec::geom_surface()), CompileOptions::golden()).unwrap();
    assert!(!scene.camera.is_2d());
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Mesh3(_))));
}
