//! CPU 射线拾取（Mesh3 / Points3）。

use plotter_types::plot::{Diagnostic, DiagnosticCode, HitResult, PrimitiveId, Result, RowId, Vec3};

use crate::{
    camera::{Ray, screen_to_ray},
    node::SceneNodeKind,
    scene::Scene,
};

/// 在屏幕像素（左下原点）处拾取最近命中。
pub fn pick_at(scene: &Scene, screen_x: f64, screen_y: f64) -> Result<Option<HitResult>> {
    if scene.camera.is_2d() {
        return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "2D 场景请使用二维命中（尚未实现）"));
    }
    let ray = screen_to_ray(&scene.camera, scene.viewport, screen_x, screen_y)?;
    let mut best: Option<(f64, HitResult)> = None;

    for node in scene.nodes.nodes() {
        match &node.kind {
            SceneNodeKind::Mesh3(mesh) => {
                let Some(interaction) = mesh.interaction
                else {
                    continue;
                };
                for (tri_index, tri) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
                    let a = mesh.positions[tri[0] as usize];
                    let b = mesh.positions[tri[1] as usize];
                    let c = mesh.positions[tri[2] as usize];
                    if let Some((t, hit_point)) = ray_triangle(ray, a, b, c)
                        && (best.as_ref().is_none_or(|(best_t, _)| t < *best_t))
                    {
                        best = Some((
                            t,
                            HitResult {
                                interaction,
                                primitive: PrimitiveId(tri_index as u64),
                                data_row: None,
                                world_position: Some(hit_point),
                            },
                        ));
                    }
                }
            }
            SceneNodeKind::Points3(points) => {
                let Some(interaction) = points.interaction
                else {
                    continue;
                };
                let radius = f64::from(points.size).max(1.0) * 0.02;
                for (row, &position) in points.positions.iter().enumerate() {
                    if let Some((t, hit_point)) = ray_point(ray, position, radius)
                        && (best.as_ref().is_none_or(|(best_t, _)| t < *best_t))
                    {
                        best = Some((
                            t,
                            HitResult {
                                interaction,
                                primitive: PrimitiveId(row as u64),
                                data_row: Some(RowId(row as u64)),
                                world_position: Some(hit_point),
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    Ok(best.map(|(_, hit)| hit))
}

/// Möller–Trumbore：返回 `(t, 交点)`，`t` 为沿射线距离。
fn ray_triangle(ray: Ray, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<(f64, Vec3)> {
    const EPS: f64 = 1e-8;
    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let pvec = ray.direction.cross(edge2);
    let det = edge1.dot(pvec);
    if det.abs() < EPS {
        return None;
    }
    let inv_det = 1.0 / det;
    let tvec = ray.origin - v0;
    let u = tvec.dot(pvec) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qvec = tvec.cross(edge1);
    let v = ray.direction.dot(qvec) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = edge2.dot(qvec) * inv_det;
    if t < EPS {
        return None;
    }
    Some((t, ray.origin + ray.direction * t))
}

fn ray_point(ray: Ray, point: Vec3, radius: f64) -> Option<(f64, Vec3)> {
    let to_point = point - ray.origin;
    let t = to_point.dot(ray.direction);
    if t < 0.0 {
        return None;
    }
    let closest = ray.origin + ray.direction * t;
    let dist = (point - closest).length();
    if dist <= radius { Some((t, closest)) } else { None }
}
