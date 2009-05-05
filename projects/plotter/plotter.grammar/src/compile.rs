//! `PlotSpec` → Scene IR 编译（validate → scale → layout → scene）。

use plotter_data::ColumnTable;
use plotter_layout::{PanelRect, layout_facet_panels, layout_single_panel};
use plotter_types::plot::{Diagnostic, DiagnosticCode, Interval, Result};
use plotter_view::{
    AxisNode, CameraSpec, MeshNode, Point2, PointsNode, PolylineNode, Rect2, Scene, SceneArena, SceneMetadata, SceneNodeKind,
    TextNode, Viewport,
};

use crate::{
    compile_3d::compile_plot_3d,
    coordinate::CoordinateSpec,
    facet::FacetSpec,
    layer::{GeomSpec, LayerSpec},
    mapping::Mapping,
    plot::{DataRef, PlotSpec},
    scale::{ScaleKind, ScaleSpec},
    theme::ThemeSpec,
    validate::validate_plot,
};

/// 编译选项。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompileOptions {
    /// 视口。
    pub viewport: Viewport,
    /// 覆盖主题外边距（左、下、右、上）；`None` 使用 `ThemeSpec.margin`。
    pub margin: Option<(f64, f64, f64, f64)>,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self { viewport: Viewport::new(640.0, 480.0), margin: None }
    }
}

impl CompileOptions {
    /// 黄金测试用的固定小视口与外边距。
    pub fn golden() -> Self {
        Self { viewport: Viewport::new(200.0, 150.0), margin: Some((24.0, 20.0, 12.0, 12.0)) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ResolvedScale {
    kind: ScaleKind,
    domain: Interval,
}

/// 校验并编译图规格为 Scene IR。
pub fn compile_plot(spec: &PlotSpec, options: CompileOptions) -> Result<Scene> {
    validate_plot(spec)?;
    match &spec.coordinates {
        CoordinateSpec::Cartesian3d(cartesian) => compile_plot_3d(spec, options, cartesian),
        CoordinateSpec::Cartesian2d(cartesian) => compile_plot_2d(spec, options, *cartesian),
        CoordinateSpec::GraphSpace(space) => crate::compile_graph::compile_plot_graph(spec, options, space),
        CoordinateSpec::TreeSpace(space) => crate::compile_graph::compile_plot_tree(spec, options, space),
    }
}

fn compile_plot_2d(spec: &PlotSpec, options: CompileOptions, cartesian: crate::coordinate::Cartesian2d) -> Result<Scene> {
    let DataRef::Table(table) = &spec.data
    else {
        return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "二维坐标系需要 ColumnTable"));
    };
    let theme = &spec.theme;
    let margin = options.margin.unwrap_or(theme.margin);

    let mut arena = SceneArena::new();
    let mut root_children = Vec::new();

    let x_scale = resolve_scale(table, &spec.mapping, &spec.layers, &spec.scales, "x", cartesian.xlim, false)?;
    let y_scale = resolve_scale(
        table,
        &spec.mapping,
        &spec.layers,
        &spec.scales,
        "y",
        cartesian.ylim,
        spec.layers.iter().any(|layer| matches!(layer.geom, GeomSpec::Bar(_))),
    )?;

    let panels = resolve_panels(table, &spec.facets, options.viewport, margin, theme)?;
    for panel in panels {
        let panel_children = compile_panel(
            &mut arena,
            PanelCompile {
                table: &panel.table,
                plot_mapping: &spec.mapping,
                layers: &spec.layers,
                frame: panel.frame,
                x_scale,
                y_scale,
                flip: cartesian.flip,
                theme,
                facet_label: panel.label.as_deref(),
            },
        )?;
        root_children.push(arena.insert(SceneNodeKind::Group { children: panel_children }));
    }

    let root = arena.insert(SceneNodeKind::Group { children: root_children });
    Ok(Scene {
        root,
        nodes: arena,
        camera: CameraSpec::Orthographic2d,
        viewport: options.viewport,
        metadata: SceneMetadata::default(),
    })
}

struct PanelJob {
    frame: PanelRect,
    table: ColumnTable,
    label: Option<String>,
}

fn resolve_panels(
    table: &ColumnTable,
    facets: &Option<FacetSpec>,
    viewport: Viewport,
    margin: (f64, f64, f64, f64),
    theme: &ThemeSpec,
) -> Result<Vec<PanelJob>> {
    match facets {
        None => {
            let frame = layout_single_panel(viewport.width, viewport.height, margin)?;
            Ok(vec![PanelJob { frame, table: table.clone(), label: None }])
        }
        Some(FacetSpec::Wrap { column, ncol }) => {
            let levels = facet_levels(table, column)?;
            let frames = layout_facet_panels(viewport.width, viewport.height, margin, levels.len(), *ncol, theme.facet_gap)?;
            let mut jobs = Vec::with_capacity(levels.len());
            for (level, frame) in levels.into_iter().zip(frames) {
                let indices = facet_row_indices(table, column, &level)?;
                let subset = table.select_rows(&indices)?;
                jobs.push(PanelJob { frame, table: subset, label: Some(level) });
            }
            Ok(jobs)
        }
    }
}

fn facet_levels(table: &ColumnTable, column: &str) -> Result<Vec<String>> {
    let values = &table.string_column(column)?.values;
    let mut levels = Vec::new();
    for value in values {
        if !levels.iter().any(|existing| existing == value) {
            levels.push(value.clone());
        }
    }
    if levels.is_empty() {
        return Err(Diagnostic::error(DiagnosticCode::EmptyData, "分面列没有水平"));
    }
    Ok(levels)
}

fn facet_row_indices(table: &ColumnTable, column: &str, level: &str) -> Result<Vec<usize>> {
    let values = &table.string_column(column)?.values;
    let indices: Vec<usize> = values.iter().enumerate().filter(|(_, v)| v.as_str() == level).map(|(i, _)| i).collect();
    if indices.is_empty() {
        return Err(Diagnostic::error(DiagnosticCode::EmptyData, format!("分面水平 `{level}` 无行")).with_param("level", level));
    }
    Ok(indices)
}

struct PanelCompile<'a> {
    table: &'a ColumnTable,
    plot_mapping: &'a Mapping,
    layers: &'a [LayerSpec],
    frame: PanelRect,
    x_scale: ResolvedScale,
    y_scale: ResolvedScale,
    flip: bool,
    theme: &'a ThemeSpec,
    facet_label: Option<&'a str>,
}

fn compile_panel(arena: &mut SceneArena, input: PanelCompile<'_>) -> Result<Vec<plotter_types::plot::NodeId>> {
    let PanelCompile { table, plot_mapping, layers, frame, x_scale, y_scale, flip, theme, facet_label } = input;
    let mut children = Vec::new();

    if let Some(fill) = theme.panel_fill {
        let rect = Rect2 { min: Point2::new(frame.left, frame.bottom), max: Point2::new(frame.right, frame.top) };
        children.push(arena.insert(SceneNodeKind::Mesh(MeshNode::from_rect(rect, fill))));
    }

    if let Some(label) = facet_label {
        children.push(arena.insert(SceneNodeKind::Text(TextNode {
            position: Point2::new(frame.left + 2.0, frame.top - f64::from(theme.facet_label_size) - 2.0),
            content: label.to_string(),
            size: theme.facet_label_size,
            color: theme.foreground,
        })));
    }

    let (h_domain, v_domain, h_origin, h_len, v_origin, v_len) = if flip {
        (
            y_scale.domain,
            x_scale.domain,
            Point2::new(frame.left, frame.bottom),
            frame.width(),
            Point2::new(frame.left, frame.bottom),
            frame.height(),
        )
    }
    else {
        (
            x_scale.domain,
            y_scale.domain,
            Point2::new(frame.left, frame.bottom),
            frame.width(),
            Point2::new(frame.left, frame.bottom),
            frame.height(),
        )
    };

    children.push(arena.insert(SceneNodeKind::Axis(AxisNode {
        horizontal: true,
        domain: h_domain,
        origin: h_origin,
        length: h_len,
        tick_count: 5,
        stroke: theme.axis_stroke,
    })));
    children.push(arena.insert(SceneNodeKind::Axis(AxisNode {
        horizontal: false,
        domain: v_domain,
        origin: v_origin,
        length: v_len,
        tick_count: 5,
        stroke: theme.axis_stroke,
    })));

    for layer in layers {
        let mapping = plot_mapping.merge(&layer.mapping);
        match &layer.geom {
            GeomSpec::Line(line) => {
                let points = project_xy_points(table, &mapping, x_scale, y_scale, frame, flip, 2)?;
                children.push(arena.insert(SceneNodeKind::Polyline(PolylineNode {
                    points,
                    stroke: theme.foreground,
                    linewidth: line.linewidth,
                })));
            }
            GeomSpec::Point(point) => {
                let positions = project_xy_points(table, &mapping, x_scale, y_scale, frame, flip, 1)?;
                children.push(arena.insert(SceneNodeKind::Points(PointsNode {
                    positions,
                    size: point.size,
                    fill: theme.foreground,
                })));
            }
            GeomSpec::Bar(bar) => {
                for rect in project_bars(table, &mapping, x_scale, y_scale, frame, flip, bar.width)? {
                    children.push(arena.insert(SceneNodeKind::Mesh(MeshNode::from_rect(rect, theme.foreground))));
                }
            }
            GeomSpec::Text(text) => {
                let labels = resolve_labels(table, &mapping, text.text.as_deref())?;
                let positions = project_xy_points(table, &mapping, x_scale, y_scale, frame, flip, 1)?;
                if positions.len() != labels.len() {
                    return Err(Diagnostic::error(DiagnosticCode::ColumnLengthMismatch, "文本标签与坐标行数不一致"));
                }
                for (position, content) in positions.into_iter().zip(labels) {
                    children.push(arena.insert(SceneNodeKind::Text(TextNode {
                        position,
                        content,
                        size: text.size,
                        color: theme.foreground,
                    })));
                }
            }
            GeomSpec::Surface(_) | GeomSpec::Point3d(_) => {
                return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "三维 geom 需要 cartesian3d"));
            }
            GeomSpec::Node(_) | GeomSpec::Edge(_) | GeomSpec::TreeNode(_) | GeomSpec::TreeEdge(_) => {
                return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "图/树 geom 需要 graph-space 或 tree-space"));
            }
        }
    }

    Ok(children)
}

fn resolve_scale(
    table: &ColumnTable,
    plot_mapping: &Mapping,
    layers: &[LayerSpec],
    scales: &[ScaleSpec],
    aesthetic: &str,
    coord_limit: Option<Interval>,
    include_zero_for_bar: bool,
) -> Result<ResolvedScale> {
    let scale = scales.iter().rev().find(|scale| scale.aesthetic == aesthetic);
    let kind = scale.map(|s| s.kind).unwrap_or(ScaleKind::Continuous);
    let mut domain = if let Some(Some(domain)) = scale.map(|s| s.domain) {
        domain
    }
    else {
        infer_domain(table, plot_mapping, layers, aesthetic)?
    };

    if include_zero_for_bar && aesthetic == "y" {
        domain = Interval::new(domain.min.min(0.0), domain.max.max(0.0));
        if (domain.max - domain.min).abs() < f64::EPSILON {
            domain = Interval::new(0.0, 1.0);
        }
    }

    if let Some(limit) = coord_limit {
        domain = limit;
    }

    if matches!(kind, ScaleKind::Log10) && (domain.min <= 0.0 || domain.max <= 0.0) {
        return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, format!("{aesthetic} log10 domain 必须为正"))
            .with_param("aesthetic", aesthetic));
    }

    if (domain.max - domain.min).abs() < f64::EPSILON {
        domain = Interval::new(domain.min, domain.min + 1.0);
    }

    Ok(ResolvedScale { kind, domain })
}

fn infer_domain(table: &ColumnTable, plot_mapping: &Mapping, layers: &[LayerSpec], aesthetic: &str) -> Result<Interval> {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for layer in layers {
        let mapping = plot_mapping.merge(&layer.mapping);
        let name = match aesthetic {
            "x" => mapping.x.as_ref().map(|expr| expr.column_name()),
            "y" => mapping.y.as_ref().map(|expr| expr.column_name()),
            _ => None,
        }
        .ok_or_else(|| {
            Diagnostic::error(DiagnosticCode::MissingMapping, format!("缺少 {aesthetic} mapping"))
                .with_param("aesthetic", aesthetic)
        })?;
        let column = table.float_column(name)?;
        for value in &column.values {
            if value.is_finite() {
                min = min.min(*value);
                max = max.max(*value);
            }
        }
        if matches!(layer.geom, GeomSpec::Bar(_)) && aesthetic == "x" {
            let half = match &layer.geom {
                GeomSpec::Bar(bar) => bar.width * 0.5,
                _ => 0.0,
            };
            min -= half;
            max += half;
        }
    }
    if !min.is_finite() || !max.is_finite() {
        return Err(
            Diagnostic::error(DiagnosticCode::EmptyData, format!("{aesthetic} 域为空")).with_param("aesthetic", aesthetic)
        );
    }
    if (max - min).abs() < f64::EPSILON {
        max = min + 1.0;
    }
    Ok(Interval::new(min, max))
}

fn to_aesthetic(value: f64, scale: ResolvedScale) -> Result<f64> {
    match scale.kind {
        ScaleKind::Continuous => Ok((value - scale.domain.min) / scale.domain.span()),
        ScaleKind::Log10 => {
            if value <= 0.0 {
                return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "log10 scale 不能映射非正值"));
            }
            let lo = scale.domain.min.log10();
            let hi = scale.domain.max.log10();
            Ok((value.log10() - lo) / (hi - lo))
        }
    }
}

fn aesthetic_to_scene(ax: f64, ay: f64, frame: PanelRect, flip: bool) -> Point2 {
    let (u, v) = if flip { (ay, ax) } else { (ax, ay) };
    Point2::new(frame.left + u * frame.width(), frame.bottom + v * frame.height())
}

fn project_xy_points(
    table: &ColumnTable,
    mapping: &Mapping,
    x_scale: ResolvedScale,
    y_scale: ResolvedScale,
    frame: PanelRect,
    flip: bool,
    min_points: usize,
) -> Result<Vec<Point2>> {
    let x_name =
        mapping.x.as_ref().ok_or_else(|| Diagnostic::error(DiagnosticCode::MissingMapping, "缺少 x mapping"))?.column_name();
    let y_name =
        mapping.y.as_ref().ok_or_else(|| Diagnostic::error(DiagnosticCode::MissingMapping, "缺少 y mapping"))?.column_name();
    let xs = &table.float_column(x_name)?.values;
    let ys = &table.float_column(y_name)?.values;
    if xs.len() != ys.len() {
        return Err(Diagnostic::error(DiagnosticCode::ColumnLengthMismatch, "x/y 列长度不一致"));
    }
    let mut points = Vec::with_capacity(xs.len());
    for (x, y) in xs.iter().zip(ys.iter()) {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        let ax = to_aesthetic(*x, x_scale)?;
        let ay = to_aesthetic(*y, y_scale)?;
        points.push(aesthetic_to_scene(ax, ay, frame, flip));
    }
    if points.len() < min_points {
        return Err(Diagnostic::error(DiagnosticCode::EmptyData, format!("至少需要 {min_points} 个有限点")));
    }
    Ok(points)
}

fn project_bars(
    table: &ColumnTable,
    mapping: &Mapping,
    x_scale: ResolvedScale,
    y_scale: ResolvedScale,
    frame: PanelRect,
    flip: bool,
    width: f64,
) -> Result<Vec<Rect2>> {
    let x_name =
        mapping.x.as_ref().ok_or_else(|| Diagnostic::error(DiagnosticCode::MissingMapping, "缺少 x mapping"))?.column_name();
    let y_name =
        mapping.y.as_ref().ok_or_else(|| Diagnostic::error(DiagnosticCode::MissingMapping, "缺少 y mapping"))?.column_name();
    let xs = &table.float_column(x_name)?.values;
    let ys = &table.float_column(y_name)?.values;
    if xs.len() != ys.len() {
        return Err(Diagnostic::error(DiagnosticCode::ColumnLengthMismatch, "x/y 列长度不一致"));
    }
    let half = width * 0.5;
    let mut rects = Vec::new();
    for (x, y) in xs.iter().zip(ys.iter()) {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        let ax0 = to_aesthetic(*x - half, x_scale)?;
        let ax1 = to_aesthetic(*x + half, x_scale)?;
        let ay0 = to_aesthetic(0.0, y_scale)?;
        let ay1 = to_aesthetic(*y, y_scale)?;
        let p00 = aesthetic_to_scene(ax0, ay0, frame, flip);
        let p11 = aesthetic_to_scene(ax1, ay1, frame, flip);
        let min = Point2::new(p00.x.min(p11.x), p00.y.min(p11.y));
        let max = Point2::new(p00.x.max(p11.x), p00.y.max(p11.y));
        rects.push(Rect2 { min, max });
    }
    if rects.is_empty() {
        return Err(Diagnostic::error(DiagnosticCode::EmptyData, "没有可绘制的柱"));
    }
    Ok(rects)
}

fn resolve_labels(table: &ColumnTable, mapping: &Mapping, constant: Option<&str>) -> Result<Vec<String>> {
    if let Some(label) = &mapping.label {
        return Ok(table.string_column(label.column_name())?.values.clone());
    }
    let text = constant.ok_or_else(|| Diagnostic::error(DiagnosticCode::MissingMapping, "缺少文本"))?;
    let rows = table.row_count();
    Ok(vec![text.to_string(); rows])
}
