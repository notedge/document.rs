//! 三维 PlotSpec → Scene（surface / point cloud）。

use plotter_data::{ColumnTable, GridData};
use plotter_types::plot::{Diagnostic, DiagnosticCode, InteractionId, Result, Vec3};
use plotter_view::{Mesh3Node, Points3Node, Scene, SceneArena, SceneMetadata, SceneNodeKind};

use crate::{
    compile::CompileOptions,
    coordinate::Cartesian3d,
    layer::GeomSpec,
    mapping::Mapping,
    plot::{DataRef, PlotSpec},
    theme::ThemeSpec,
};

pub(crate) fn compile_plot_3d(spec: &PlotSpec, options: CompileOptions, cartesian: &Cartesian3d) -> Result<Scene> {
    let theme = &spec.theme;
    let camera = cartesian.camera.to_scene_camera();
    let mut arena = SceneArena::new();
    let mut children = Vec::new();

    for (layer_index, layer) in spec.layers.iter().enumerate() {
        let mapping = spec.mapping.merge(&layer.mapping);
        let interaction = Some(InteractionId(layer_index as u64));
        match &layer.geom {
            GeomSpec::Surface(_) => {
                let DataRef::Grid(grid) = &spec.data
                else {
                    return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "geom_surface 需要 GridData"));
                };
                children.push(arena.insert(SceneNodeKind::Mesh3(mesh3_from_grid(grid, theme, interaction)?)));
            }
            GeomSpec::Point3d(point) => {
                let DataRef::Table(table) = &spec.data
                else {
                    return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "geom_point3d 需要 ColumnTable"));
                };
                children.push(arena.insert(SceneNodeKind::Points3(points3_from_table(
                    table,
                    &mapping,
                    point.size,
                    theme,
                    interaction,
                )?)));
            }
            _ => {
                return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "三维坐标系仅支持 surface / point3d")
                    .with_param("layer", layer_index.to_string()));
            }
        }
    }

    let root = arena.insert(SceneNodeKind::Group { children });
    Ok(Scene { root, nodes: arena, camera, viewport: options.viewport, metadata: SceneMetadata::default() })
}

fn mesh3_from_grid(grid: &GridData, theme: &ThemeSpec, interaction: Option<InteractionId>) -> Result<Mesh3Node> {
    grid.validate()?;
    let (positions, indices) = grid.triangulate();
    Ok(Mesh3Node { positions, indices, fill: theme.foreground, interaction })
}

fn points3_from_table(
    table: &ColumnTable,
    mapping: &Mapping,
    size: f32,
    theme: &ThemeSpec,
    interaction: Option<InteractionId>,
) -> Result<Points3Node> {
    let x_name =
        mapping.x.as_ref().ok_or_else(|| Diagnostic::error(DiagnosticCode::MissingMapping, "缺少 x mapping"))?.column_name();
    let y_name =
        mapping.y.as_ref().ok_or_else(|| Diagnostic::error(DiagnosticCode::MissingMapping, "缺少 y mapping"))?.column_name();
    let z_name =
        mapping.z.as_ref().ok_or_else(|| Diagnostic::error(DiagnosticCode::MissingMapping, "缺少 z mapping"))?.column_name();
    let xs = &table.float_column(x_name)?.values;
    let ys = &table.float_column(y_name)?.values;
    let zs = &table.float_column(z_name)?.values;
    if xs.len() != ys.len() || ys.len() != zs.len() {
        return Err(Diagnostic::error(DiagnosticCode::ColumnLengthMismatch, "x/y/z 列长度不一致"));
    }
    let mut positions = Vec::new();
    for ((&x, &y), &z) in xs.iter().zip(ys.iter()).zip(zs.iter()) {
        if x.is_finite() && y.is_finite() && z.is_finite() {
            positions.push(Vec3::new(x, y, z));
        }
    }
    if positions.is_empty() {
        return Err(Diagnostic::error(DiagnosticCode::EmptyData, "没有可绘制的三维点"));
    }
    Ok(Points3Node { positions, size, fill: theme.foreground, interaction })
}
