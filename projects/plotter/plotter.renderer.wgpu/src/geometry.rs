//! Scene 图元 → GPU 顶点（2D 与投影后的 3D）。

use plotter_renderer::{Drawable, walk_drawables};
use plotter_types::plot::{Result, Rgba};
use plotter_view::{
    AxisNode, Mesh3Node, MeshNode, Point2, Points3Node, PointsNode, PolylineNode, Scene, try_project_to_screen,
};

/// 线段列表顶点（场景坐标，y 向上）。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LineVertex {
    /// xy。
    pub position: [f32; 2],
    /// rgba。
    pub color: [f32; 4],
}

/// 三角形列表顶点。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    /// xy。
    pub position: [f32; 2],
    /// rgba。
    pub color: [f32; 4],
}

impl LineVertex {
    fn new(point: Point2, color: Rgba) -> Self {
        Self { position: [point.x as f32, point.y as f32], color: [color.r, color.g, color.b, color.a] }
    }
}

impl MeshVertex {
    fn new(point: Point2, color: Rgba) -> Self {
        Self { position: [point.x as f32, point.y as f32], color: [color.r, color.g, color.b, color.a] }
    }
}

/// 收集线段与三角形顶点。文本由 CPU/SVG 负责，此处忽略。
pub fn collect_geometry(scene: &Scene) -> Result<(Vec<LineVertex>, Vec<MeshVertex>)> {
    let mut lines = Vec::new();
    let mut mesh = Vec::new();
    walk_drawables(scene, |drawable| {
        match drawable {
            Drawable::Polyline(polyline) => push_polyline(&mut lines, polyline),
            Drawable::Points(points) => push_points_as_crosses(&mut lines, points),
            Drawable::Mesh(node) => push_mesh(&mut mesh, node),
            Drawable::Mesh3(node) => push_mesh3(scene, &mut mesh, node),
            Drawable::Points3(node) => push_points3_as_crosses(scene, &mut lines, node),
            Drawable::Text(_) => {}
            Drawable::Axis(axis) => push_axis(&mut lines, axis),
        }
        Ok(())
    })?;
    Ok((lines, mesh))
}

fn push_segment(out: &mut Vec<LineVertex>, a: Point2, b: Point2, color: Rgba) {
    out.push(LineVertex::new(a, color));
    out.push(LineVertex::new(b, color));
}

fn push_polyline(out: &mut Vec<LineVertex>, polyline: &PolylineNode) {
    for window in polyline.points.windows(2) {
        push_segment(out, window[0], window[1], polyline.stroke);
    }
}

fn push_points_as_crosses(out: &mut Vec<LineVertex>, points: &PointsNode) {
    let r = f64::from(points.size);
    for p in &points.positions {
        push_segment(out, Point2::new(p.x - r, p.y), Point2::new(p.x + r, p.y), points.fill);
        push_segment(out, Point2::new(p.x, p.y - r), Point2::new(p.x, p.y + r), points.fill);
    }
}

fn push_mesh(out: &mut Vec<MeshVertex>, mesh: &MeshNode) {
    for tri in mesh.indices.as_chunks::<3>().0 {
        out.push(MeshVertex::new(mesh.positions[tri[0] as usize], mesh.fill));
        out.push(MeshVertex::new(mesh.positions[tri[1] as usize], mesh.fill));
        out.push(MeshVertex::new(mesh.positions[tri[2] as usize], mesh.fill));
    }
}

fn push_mesh3(scene: &Scene, out: &mut Vec<MeshVertex>, mesh: &Mesh3Node) {
    let mut tris = Vec::new();
    for tri in mesh.indices.as_chunks::<3>().0 {
        let Some(pa) = try_project_to_screen(&scene.camera, scene.viewport, mesh.positions[tri[0] as usize])
        else {
            continue;
        };
        let Some(pb) = try_project_to_screen(&scene.camera, scene.viewport, mesh.positions[tri[1] as usize])
        else {
            continue;
        };
        let Some(pc) = try_project_to_screen(&scene.camera, scene.viewport, mesh.positions[tri[2] as usize])
        else {
            continue;
        };
        let depth = (pa.depth + pb.depth + pc.depth) / 3.0;
        tris.push((depth, Point2::new(pa.x, pa.y), Point2::new(pb.x, pb.y), Point2::new(pc.x, pc.y)));
    }
    tris.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, a, b, c) in tris {
        out.push(MeshVertex::new(a, mesh.fill));
        out.push(MeshVertex::new(b, mesh.fill));
        out.push(MeshVertex::new(c, mesh.fill));
    }
}

fn push_points3_as_crosses(scene: &Scene, out: &mut Vec<LineVertex>, points: &Points3Node) {
    let r = f64::from(points.size);
    for position in &points.positions {
        let Some(p) = try_project_to_screen(&scene.camera, scene.viewport, *position)
        else {
            continue;
        };
        let center = Point2::new(p.x, p.y);
        push_segment(out, Point2::new(center.x - r, center.y), Point2::new(center.x + r, center.y), points.fill);
        push_segment(out, Point2::new(center.x, center.y - r), Point2::new(center.x, center.y + r), points.fill);
    }
}

fn push_axis(out: &mut Vec<LineVertex>, axis: &AxisNode) {
    let origin = axis.origin;
    let end = if axis.horizontal {
        Point2::new(origin.x + axis.length, origin.y)
    }
    else {
        Point2::new(origin.x, origin.y + axis.length)
    };
    push_segment(out, origin, end, axis.stroke);

    let ticks = axis.tick_count.max(2);
    for i in 0..ticks {
        let t = f64::from(i) / f64::from(ticks - 1);
        let (start, tip) = if axis.horizontal {
            let x = origin.x + axis.length * t;
            (Point2::new(x, origin.y), Point2::new(x, origin.y - 6.0))
        }
        else {
            let y = origin.y + axis.length * t;
            (Point2::new(origin.x, y), Point2::new(origin.x - 6.0, y))
        };
        push_segment(out, start, tip, axis.stroke);
    }
}
