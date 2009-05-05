//! SVG 渲染器实现。

use plotter_renderer::{
    Capability, Drawable, FrameReport, PreparedScene, RenderTarget, Renderer, RendererCapabilities, color_to_css,
    walk_drawables,
};
use plotter_types::plot::{Diagnostic, DiagnosticCode, Result};
use plotter_view::{
    AxisNode, Mesh3Node, MeshNode, Point2, Points3Node, PointsNode, PolylineNode, Scene, TextNode, try_project_to_screen,
};

/// SVG 渲染器。
#[derive(Debug, Default, Clone, Copy)]
pub struct SvgRenderer;

impl SvgRenderer {
    /// 构造。
    pub const fn new() -> Self {
        Self
    }
}

impl Renderer for SvgRenderer {
    fn capabilities(&self) -> RendererCapabilities {
        RendererCapabilities { raster_2d: Capability::Unsupported, svg: Capability::Available, gpu: Capability::Unsupported }
    }

    fn prepare(&mut self, scene: &Scene) -> Result<PreparedScene> {
        Ok(PreparedScene::from_scene(scene))
    }

    fn render(&mut self, prepared: &PreparedScene, target: &mut RenderTarget) -> Result<FrameReport> {
        let RenderTarget::Svg(document) = target
        else {
            return Err(Diagnostic::error(DiagnosticCode::UnsupportedTarget, "SvgRenderer 需要 Svg 目标"));
        };

        let scene = &prepared.scene;
        let width = scene.viewport.width.max(1.0);
        let height = scene.viewport.height.max(1.0);
        let mut body = String::new();
        let mut primitive_count = 0_u32;

        walk_drawables(scene, |drawable| {
            match drawable {
                Drawable::Polyline(polyline) => {
                    primitive_count += 1;
                    body.push_str(&polyline_svg(polyline, height));
                }
                Drawable::Points(points) => {
                    primitive_count += points.positions.len() as u32;
                    body.push_str(&points_svg(points, height));
                }
                Drawable::Mesh(mesh) => {
                    primitive_count += 1;
                    body.push_str(&mesh_svg(mesh, height));
                }
                Drawable::Mesh3(mesh) => {
                    let svg = mesh3_svg(scene, mesh);
                    if !svg.is_empty() {
                        primitive_count += 1;
                        body.push_str(&svg);
                    }
                }
                Drawable::Points3(points) => {
                    let (svg, count) = points3_svg(scene, points);
                    primitive_count += count;
                    body.push_str(&svg);
                }
                Drawable::Text(text) => {
                    primitive_count += 1;
                    body.push_str(&text_svg(text, height));
                }
                Drawable::Axis(axis) => {
                    let (axis_svg, count) = axis_svg(axis, height);
                    primitive_count += count;
                    body.push_str(&axis_svg);
                }
            }
            Ok(())
        })?;

        *document = format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?>"#,
                "\n",
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#,
                "\n",
                r#"<rect width="100%" height="100%" fill="white"/>"#,
                "\n",
                "{body}",
                "</svg>\n"
            ),
            w = width,
            h = height,
            body = body
        );

        Ok(FrameReport { primitive_count })
    }
}

/// 便捷：Scene → SVG 文档字符串。
pub fn render_svg(scene: &Scene) -> Result<String> {
    let mut renderer = SvgRenderer::new();
    let prepared = renderer.prepare(scene)?;
    let mut target = RenderTarget::Svg(String::new());
    renderer.render(&prepared, &mut target)?;
    match target {
        RenderTarget::Svg(document) => Ok(document),
        RenderTarget::Rgba8(_) => unreachable!("svg renderer only writes svg"),
    }
}

fn flip_y(point: Point2, height: f64) -> (f64, f64) {
    (point.x, height - point.y)
}

fn polyline_svg(polyline: &PolylineNode, height: f64) -> String {
    let points: Vec<String> = polyline
        .points
        .iter()
        .map(|point| {
            let (x, y) = flip_y(*point, height);
            format!("{x:.3},{y:.3}")
        })
        .collect();
    format!(
        r#"<polyline fill="none" stroke="{stroke}" stroke-width="{width}" points="{points}"/>"#,
        stroke = color_to_css(polyline.stroke),
        width = polyline.linewidth,
        points = points.join(" ")
    ) + "\n"
}

fn points_svg(points: &PointsNode, height: f64) -> String {
    let mut out = String::new();
    let fill = color_to_css(points.fill);
    for position in &points.positions {
        let (x, y) = flip_y(*position, height);
        out.push_str(&format!(r#"<circle cx="{x:.3}" cy="{y:.3}" r="{r}" fill="{fill}"/>"#, r = points.size));
        out.push('\n');
    }
    out
}

fn mesh_svg(mesh: &MeshNode, height: f64) -> String {
    let mut out = String::new();
    let fill = color_to_css(mesh.fill);
    for tri in mesh.indices.as_chunks::<3>().0 {
        let a = flip_y(mesh.positions[tri[0] as usize], height);
        let b = flip_y(mesh.positions[tri[1] as usize], height);
        let c = flip_y(mesh.positions[tri[2] as usize], height);
        out.push_str(&format!(
            r#"<polygon fill="{fill}" points="{:.3},{:.3} {:.3},{:.3} {:.3},{:.3}"/>"#,
            a.0, a.1, b.0, b.1, c.0, c.1
        ));
        out.push('\n');
    }
    out
}

fn mesh3_svg(scene: &Scene, mesh: &Mesh3Node) -> String {
    let height = scene.viewport.height;
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
        tris.push((depth, pa, pb, pc));
    }
    tris.sort_by(|a, b| b.0.total_cmp(&a.0));
    let fill = color_to_css(mesh.fill);
    let mut out = String::new();
    for (_, pa, pb, pc) in tris {
        let a = flip_y(Point2::new(pa.x, pa.y), height);
        let b = flip_y(Point2::new(pb.x, pb.y), height);
        let c = flip_y(Point2::new(pc.x, pc.y), height);
        out.push_str(&format!(
            r#"<polygon fill="{fill}" points="{:.3},{:.3} {:.3},{:.3} {:.3},{:.3}"/>"#,
            a.0, a.1, b.0, b.1, c.0, c.1
        ));
        out.push('\n');
    }
    out
}

fn points3_svg(scene: &Scene, points: &Points3Node) -> (String, u32) {
    let height = scene.viewport.height;
    let fill = color_to_css(points.fill);
    let mut out = String::new();
    let mut count = 0_u32;
    for position in &points.positions {
        let Some(p) = try_project_to_screen(&scene.camera, scene.viewport, *position)
        else {
            continue;
        };
        let (x, y) = flip_y(Point2::new(p.x, p.y), height);
        out.push_str(&format!(r#"<circle cx="{x:.3}" cy="{y:.3}" r="{r}" fill="{fill}"/>"#, r = points.size));
        out.push('\n');
        count += 1;
    }
    (out, count)
}

fn text_svg(text: &TextNode, height: f64) -> String {
    let (x, y) = flip_y(text.position, height);
    format!(
        r#"<text x="{x:.3}" y="{y:.3}" font-size="{size}" fill="{fill}" font-family="monospace">{content}</text>"#,
        size = text.size,
        fill = color_to_css(text.color),
        content = xml_escape(&text.content)
    ) + "\n"
}

fn xml_escape(input: &str) -> String {
    input.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn axis_svg(axis: &AxisNode, height: f64) -> (String, u32) {
    let origin = axis.origin;
    let end = if axis.horizontal {
        Point2::new(origin.x + axis.length, origin.y)
    }
    else {
        Point2::new(origin.x, origin.y + axis.length)
    };
    let (x0, y0) = flip_y(origin, height);
    let (x1, y1) = flip_y(end, height);
    let stroke = color_to_css(axis.stroke);
    let mut out = format!(r#"<line x1="{x0:.3}" y1="{y0:.3}" x2="{x1:.3}" y2="{y1:.3}" stroke="{stroke}" stroke-width="1"/>"#);
    out.push('\n');
    let mut count = 1_u32;

    let ticks = axis.tick_count.max(2);
    for i in 0..ticks {
        let t = f64::from(i) / f64::from(ticks - 1);
        let (start, end) = if axis.horizontal {
            let x = origin.x + axis.length * t;
            (Point2::new(x, origin.y), Point2::new(x, origin.y - 6.0))
        }
        else {
            let y = origin.y + axis.length * t;
            (Point2::new(origin.x, y), Point2::new(origin.x - 6.0, y))
        };
        let (sx, sy) = flip_y(start, height);
        let (ex, ey) = flip_y(end, height);
        out.push_str(&format!(
            r#"<line x1="{sx:.3}" y1="{sy:.3}" x2="{ex:.3}" y2="{ey:.3}" stroke="{stroke}" stroke-width="1"/>"#
        ));
        out.push('\n');
        count += 1;
    }
    (out, count)
}
