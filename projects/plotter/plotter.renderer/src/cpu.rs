//! CPU 栅格后端 — 确定性 reference（2D + 投影后的 3D）。

use plotter_types::plot::{Diagnostic, DiagnosticCode, Result, Rgba};
use plotter_view::{
    AxisNode, Mesh3Node, MeshNode, Point2, Points3Node, PointsNode, PolylineNode, Scene, TextNode, try_project_to_screen,
};

use crate::{
    Renderer,
    font::glyph5x7,
    prepare::PreparedScene,
    report::{Capability, FrameReport, RendererCapabilities},
    target::RenderTarget,
    walk::{Drawable, walk_drawables},
};

/// CPU 栅格渲染器。
#[derive(Debug, Default, Clone, Copy)]
pub struct CpuRasterRenderer;

impl CpuRasterRenderer {
    /// 构造。
    pub const fn new() -> Self {
        Self
    }
}

impl Renderer for CpuRasterRenderer {
    fn capabilities(&self) -> RendererCapabilities {
        RendererCapabilities { raster_2d: Capability::Available, svg: Capability::Unsupported, gpu: Capability::Unsupported }
    }

    fn prepare(&mut self, scene: &Scene) -> Result<PreparedScene> {
        Ok(PreparedScene::from_scene(scene))
    }

    fn render(&mut self, prepared: &PreparedScene, target: &mut RenderTarget) -> Result<FrameReport> {
        let RenderTarget::Rgba8(image) = target
        else {
            return Err(Diagnostic::error(DiagnosticCode::UnsupportedTarget, "CpuRasterRenderer 需要 Rgba8 目标"));
        };

        let scene = &prepared.scene;
        *image = crate::target::RgbaImage::from_viewport(scene.viewport);
        let height = image.height as f64;
        let primitive_count =
            if scene.camera.is_2d() { render_2d(image, height, scene)? } else { render_3d_projected(image, height, scene)? };

        Ok(FrameReport { primitive_count })
    }
}

fn render_2d(image: &mut crate::target::RgbaImage, height: f64, scene: &Scene) -> Result<u32> {
    let mut primitive_count = 0_u32;
    walk_drawables(scene, |drawable| {
        match drawable {
            Drawable::Polyline(polyline) => primitive_count += draw_polyline(image, height, polyline),
            Drawable::Points(points) => primitive_count += draw_points(image, height, points),
            Drawable::Mesh(mesh) => primitive_count += draw_mesh(image, height, mesh),
            Drawable::Mesh3(_) | Drawable::Points3(_) => {}
            Drawable::Text(text) => primitive_count += draw_text(image, height, text),
            Drawable::Axis(axis) => primitive_count += draw_axis(image, height, axis),
        }
        Ok(())
    })?;
    Ok(primitive_count)
}

fn render_3d_projected(image: &mut crate::target::RgbaImage, height: f64, scene: &Scene) -> Result<u32> {
    let mut tris = Vec::new();
    let mut points = Vec::new();
    walk_drawables(scene, |drawable| {
        match drawable {
            Drawable::Mesh3(mesh) => collect_mesh3_tris(scene, mesh, &mut tris),
            Drawable::Points3(cloud) => collect_points3(scene, cloud, &mut points),
            Drawable::Polyline(_) | Drawable::Points(_) | Drawable::Mesh(_) | Drawable::Text(_) | Drawable::Axis(_) => {}
        }
        Ok(())
    })?;

    tris.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    points.sort_by(|a, b| b.depth.total_cmp(&a.depth));

    let mut primitive_count = 0_u32;
    for tri in &tris {
        primitive_count += fill_triangle(image, tri.a, tri.b, tri.c, tri.fill);
    }
    for point in &points {
        let (cx, cy) = scene_to_pixel(Point2::new(point.x, point.y), height);
        let radius = point.radius.max(1);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= radius * radius {
                    image.set(cx + dx, cy + dy, point.fill);
                }
            }
        }
        primitive_count += 1;
    }
    Ok(primitive_count)
}

struct ProjectedTri {
    a: (i32, i32),
    b: (i32, i32),
    c: (i32, i32),
    depth: f64,
    fill: Rgba,
}

struct ProjectedPoint {
    x: f64,
    y: f64,
    depth: f64,
    radius: i32,
    fill: Rgba,
}

fn collect_mesh3_tris(scene: &Scene, mesh: &Mesh3Node, out: &mut Vec<ProjectedTri>) {
    let height = scene.viewport.height;
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
        out.push(ProjectedTri {
            a: scene_to_pixel(Point2::new(pa.x, pa.y), height),
            b: scene_to_pixel(Point2::new(pb.x, pb.y), height),
            c: scene_to_pixel(Point2::new(pc.x, pc.y), height),
            depth: (pa.depth + pb.depth + pc.depth) / 3.0,
            fill: mesh.fill,
        });
    }
}

fn collect_points3(scene: &Scene, cloud: &Points3Node, out: &mut Vec<ProjectedPoint>) {
    for position in &cloud.positions {
        let Some(p) = try_project_to_screen(&scene.camera, scene.viewport, *position)
        else {
            continue;
        };
        out.push(ProjectedPoint {
            x: p.x,
            y: p.y,
            depth: p.depth,
            radius: cloud.size.max(1.0).round() as i32,
            fill: cloud.fill,
        });
    }
}

fn scene_to_pixel(point: Point2, height: f64) -> (i32, i32) {
    ((point.x).round() as i32, (height - point.y).round() as i32)
}

fn draw_polyline(image: &mut crate::target::RgbaImage, height: f64, polyline: &PolylineNode) -> u32 {
    if polyline.points.len() < 2 {
        return 0;
    }
    let radius = ((polyline.linewidth.max(1.0) - 1.0) * 0.5).ceil() as i32;
    let mut count = 0_u32;
    for window in polyline.points.windows(2) {
        let (x0, y0) = scene_to_pixel(window[0], height);
        let (x1, y1) = scene_to_pixel(window[1], height);
        count += draw_thick_line(image, x0, y0, x1, y1, radius, polyline.stroke);
    }
    count
}

fn draw_points(image: &mut crate::target::RgbaImage, height: f64, points: &PointsNode) -> u32 {
    let radius = points.size.max(1.0).round() as i32;
    let mut count = 0_u32;
    for position in &points.positions {
        let (cx, cy) = scene_to_pixel(*position, height);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= radius * radius {
                    image.set(cx + dx, cy + dy, points.fill);
                }
            }
        }
        count += 1;
    }
    count
}

fn draw_mesh(image: &mut crate::target::RgbaImage, height: f64, mesh: &MeshNode) -> u32 {
    let mut count = 0_u32;
    for tri in mesh.indices.as_chunks::<3>().0 {
        let a = scene_to_pixel(mesh.positions[tri[0] as usize], height);
        let b = scene_to_pixel(mesh.positions[tri[1] as usize], height);
        let c = scene_to_pixel(mesh.positions[tri[2] as usize], height);
        count += fill_triangle(image, a, b, c, mesh.fill);
    }
    count
}

fn draw_text(image: &mut crate::target::RgbaImage, height: f64, text: &TextNode) -> u32 {
    let (mut x, y) = scene_to_pixel(text.position, height);
    let scale = ((text.size / 7.0).round() as i32).max(1);
    let mut count = 0_u32;
    for ch in text.content.chars() {
        let glyph = glyph5x7(ch);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            image.set(x + col * scale + sx, y + row as i32 * scale + sy, text.color);
                        }
                    }
                }
            }
        }
        x += 6 * scale;
        count += 1;
    }
    count
}

fn draw_axis(image: &mut crate::target::RgbaImage, height: f64, axis: &AxisNode) -> u32 {
    let mut count = 0_u32;
    let origin = axis.origin;
    let end = if axis.horizontal {
        Point2::new(origin.x + axis.length, origin.y)
    }
    else {
        Point2::new(origin.x, origin.y + axis.length)
    };
    let (x0, y0) = scene_to_pixel(origin, height);
    let (x1, y1) = scene_to_pixel(end, height);
    count += draw_thick_line(image, x0, y0, x1, y1, 0, axis.stroke);

    let ticks = axis.tick_count.max(2);
    for i in 0..ticks {
        let t = f64::from(i) / f64::from(ticks - 1);
        let (tick_start, tick_end) = if axis.horizontal {
            let x = origin.x + axis.length * t;
            (Point2::new(x, origin.y), Point2::new(x, origin.y - 6.0))
        }
        else {
            let y = origin.y + axis.length * t;
            (Point2::new(origin.x, y), Point2::new(origin.x - 6.0, y))
        };
        let (sx, sy) = scene_to_pixel(tick_start, height);
        let (ex, ey) = scene_to_pixel(tick_end, height);
        count += draw_thick_line(image, sx, sy, ex, ey, 0, axis.stroke);
    }
    count
}

fn fill_triangle(image: &mut crate::target::RgbaImage, a: (i32, i32), b: (i32, i32), c: (i32, i32), color: Rgba) -> u32 {
    let min_x = a.0.min(b.0).min(c.0);
    let max_x = a.0.max(b.0).max(c.0);
    let min_y = a.1.min(b.1).min(c.1);
    let max_y = a.1.max(b.1).max(c.1);
    let mut count = 0_u32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if point_in_triangle((x, y), a, b, c) {
                image.set(x, y, color);
                count += 1;
            }
        }
    }
    count
}

fn edge(a: (i32, i32), b: (i32, i32), p: (i32, i32)) -> i32 {
    (p.0 - a.0) * (b.1 - a.1) - (p.1 - a.1) * (b.0 - a.0)
}

fn point_in_triangle(p: (i32, i32), a: (i32, i32), b: (i32, i32), c: (i32, i32)) -> bool {
    let ab = edge(a, b, p);
    let bc = edge(b, c, p);
    let ca = edge(c, a, p);
    let has_neg = ab < 0 || bc < 0 || ca < 0;
    let has_pos = ab > 0 || bc > 0 || ca > 0;
    !(has_neg && has_pos)
}

fn draw_thick_line(image: &mut crate::target::RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, radius: i32, color: Rgba) -> u32 {
    let mut count = 0_u32;
    for point in bresenham(x0, y0, x1, y1) {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                image.set(point.0 + dx, point.1 + dy, color);
            }
        }
        count += 1;
    }
    count
}

fn bresenham(mut x0: i32, mut y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let mut points = Vec::new();
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        points.push((x0, y0));
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    points
}
