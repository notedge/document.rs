//! Deterministic semantic scene rendering for a diagram layout plan.
//!
//! Rendering only translates final plan geometry into [`SemanticScene`] nodes.
//! It never re-runs layout, measures text, chooses scales, or emits backend
//! commands. Resolution into display geometry is a separate step.

use plotter_scene::{
    Color, MarkerShape, PathData, Point, Rect, SemanticKind, SemanticNode, SemanticRole, SemanticScene, ShapeStyle, Size,
    SourceRef, TextAnchor, TextStyle, Transform,
};

use crate::{
    plan::{DiagramLayoutPlan, PlacedCluster, PlacedNode, PlanLayout, RoutedEdge},
    spec::{EdgeStyle, NodeShape},
};

use plotter_types::{DIAGRAM_THEME, EDGE_LABEL_BG_OPACITY};

/// Light-palette tokens; dark hosts get the inlined dark palette from the
/// themed SVG serializer.
const NODE_FILL: Color = DIAGRAM_THEME.light.node_fill;
const NODE_STROKE: Color = DIAGRAM_THEME.light.node_stroke;
const EDGE_COLOR: Color = DIAGRAM_THEME.light.edge_stroke;
const CLUSTER_FILL: Color = DIAGRAM_THEME.light.cluster_fill;
const CLUSTER_STROKE: Color = DIAGRAM_THEME.light.node_stroke;
const NODE_TEXT: Color = DIAGRAM_THEME.light.node_text;
const EDGE_LABEL_TEXT: Color = DIAGRAM_THEME.light.edge_label_text;
const LIFELINE_COLOR: Color = DIAGRAM_THEME.light.edge_stroke;
const ROUND_RADIUS: f64 = 5.0;
const CLUSTER_RADIUS: f64 = 10.0;
/// Dash lengths in scene units, shared by every diagram backend.
const DASH_PATTERN: [f64; 2] = [3.0, 3.0];
/// Dot lengths in scene units, shared by every diagram backend.
const DOT_PATTERN: [f64; 2] = [1.0, 3.0];
/// Stroke width used by thick and bold edges.
const BOLD_EDGE_WIDTH: f64 = 2.5;
const LINE_HEIGHT: f64 = 1.4;
const BASELINE_SHIFT: f64 = 0.35;
const ARROW_SIZE: f64 = 8.0;
const CLUSTER_LABEL_X: f64 = 10.0;
const CLUSTER_LABEL_Y: f64 = 14.0;
const CLUSTER_FONT_SIZE: f64 = 12.0;
const GRAPH_EDGE_PREFIX: &str = "edge";
const SEQUENCE_MESSAGE_PREFIX: &str = "sequence-message";

/// Tuning for deterministic scene rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderOptions {
    /// Font size used for node labels. Edge and cluster labels use a size
    /// derived from this value.
    pub font_size: f64,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { font_size: 13.0 }
    }
}

/// Render a layout plan into a deterministic semantic scene.
///
/// This uses [`RenderOptions::default`]. Equal plans always produce equal
/// semantics and byte-identical SVG after resolution.
pub fn render(plan: &DiagramLayoutPlan) -> SemanticScene {
    render_with_options(plan, &RenderOptions::default())
}

/// Render a layout plan with explicit text options.
///
/// The plan's [`PlanLayout`] chooses the renderer: layered plans paint
/// clusters, edges, nodes, and labels, sequence plans paint lifelines and
/// activations around the same participant and message surfaces. The output
/// keeps one shape node per placed node carrying [`SemanticRole::Node`], a
/// [`SourceRef`] for the spec node id, and a hit region equal to the node
/// rectangle. Edges are groups with [`SemanticRole::Edge`], edge labels use
/// [`SemanticRole::EdgeLabel`], and cluster frames use
/// [`SemanticRole::Cluster`].
pub fn render_with_options(plan: &DiagramLayoutPlan, options: &RenderOptions) -> SemanticScene {
    match plan.layout {
        PlanLayout::Layered => render_layered(plan, options),
        PlanLayout::TimeOrdered => render_sequence_with_options(plan, options),
    }
}

fn render_layered(plan: &DiagramLayoutPlan, options: &RenderOptions) -> SemanticScene {
    let mut scene = SemanticScene::new(plan.canvas);

    for cluster in &plan.clusters {
        push_cluster(&mut scene, cluster, options);
    }
    for (index, edge) in plan.edges.iter().enumerate() {
        push_edge(&mut scene, GRAPH_EDGE_PREFIX, index, edge);
    }
    for node in &plan.nodes {
        push_node(&mut scene, node);
    }
    for node in &plan.nodes {
        push_node_label(&mut scene, node, options);
    }
    for (index, edge) in plan.edges.iter().enumerate() {
        push_edge_label(&mut scene, GRAPH_EDGE_PREFIX, index, edge, options);
    }

    scene
}

/// Render a sequence layout plan into a deterministic semantic scene.
///
/// Participants use the same box and label surface as graph nodes, messages
/// reuse routed edges with [`SemanticRole::Edge`], and every participant gets a
/// [`SemanticRole::Decoration`] lifeline from its header bottom to the canvas
/// bottom. Equal plans always produce equal scenes and byte-identical SVG after
/// resolution.
pub fn render_sequence(plan: &DiagramLayoutPlan) -> SemanticScene {
    render_sequence_with_options(plan, &RenderOptions::default())
}

/// Render a sequence layout plan with explicit text options.
pub fn render_sequence_with_options(plan: &DiagramLayoutPlan, options: &RenderOptions) -> SemanticScene {
    let mut scene = SemanticScene::new(plan.canvas);

    for participant in &plan.nodes {
        let center = participant.rect.center();
        scene.push(
            SemanticNode::new(SemanticKind::Line {
                from: Point::new(center.x, participant.rect.bottom()),
                to: Point::new(center.x, plan.canvas.height),
                style: ShapeStyle::stroked(LIFELINE_COLOR, 1.0),
            })
            .with_id(format!("sequence-lifeline-{}", participant.id))
            .with_role(SemanticRole::Decoration)
            .with_source(SourceRef::element(participant.id.clone())),
        );
    }
    for (index, activation) in plan.activations.iter().enumerate() {
        scene.push(
            SemanticNode::new(SemanticKind::Rect {
                rect: activation.rect,
                style: ShapeStyle {
                    fill: Some(NODE_FILL),
                    stroke: Some(NODE_STROKE),
                    stroke_width: 1.0,
                    ..ShapeStyle::default()
                },
            })
            .with_id(format!("sequence-activation-{index}"))
            .with_role(SemanticRole::Decoration)
            .with_source(SourceRef::element(activation.participant.clone())),
        );
    }
    for (index, message) in plan.edges.iter().enumerate() {
        push_edge(&mut scene, SEQUENCE_MESSAGE_PREFIX, index, message);
    }
    for participant in &plan.nodes {
        push_node(&mut scene, participant);
    }
    for participant in &plan.nodes {
        push_node_label(&mut scene, participant, options);
    }
    for (index, message) in plan.edges.iter().enumerate() {
        push_edge_label(&mut scene, SEQUENCE_MESSAGE_PREFIX, index, message, options);
    }

    scene
}

fn push_cluster(scene: &mut SemanticScene, cluster: &PlacedCluster, options: &RenderOptions) {
    let source = SourceRef::element(cluster.id.clone());
    scene.push(
        SemanticNode::new(SemanticKind::RoundedRect { rect: cluster.rect, radius: CLUSTER_RADIUS, style: cluster_style() })
            .with_id(format!("cluster-{}", cluster.id))
            .with_role(SemanticRole::Cluster)
            .with_source(source.clone()),
    );

    if let Some(label) = &cluster.label {
        let font_size = CLUSTER_FONT_SIZE.min(options.font_size);
        for (index, line) in label.split('\n').enumerate() {
            let position = Point::new(
                cluster.rect.x + CLUSTER_LABEL_X,
                cluster.rect.y + CLUSTER_LABEL_Y + index as f64 * font_size * LINE_HEIGHT,
            );
            scene.push(
                SemanticNode::new(SemanticKind::Text {
                    position,
                    text: line.to_string(),
                    style: label_text_style(font_size, CLUSTER_STROKE),
                })
                .with_id(cluster_label_id(&cluster.id, index, label.split('\n').count()))
                .with_role(SemanticRole::Label)
                .with_source(source.clone()),
            );
        }
    }
}

fn push_edge(scene: &mut SemanticScene, id_prefix: &str, index: usize, edge: &RoutedEdge) {
    let source = SourceRef::element(format!("{}->{}", edge.from.node, edge.to.node));
    let mut path = PathData::new().move_to(edge.points[0]);
    for &point in &edge.points[1..] {
        path = path.line_to(point);
    }

    let mut children = vec![
        SemanticNode::new(SemanticKind::Path { path, style: edge_style(edge.style) })
            .with_id(format!("{id_prefix}-{index}-line")),
    ];

    if has_arrow(edge.style) {
        let end = edge.points[edge.points.len() - 1];
        let previous = edge.points[edge.points.len() - 2];
        let angle = (end.y - previous.y).atan2(end.x - previous.x).to_degrees();
        let transform = Transform::rotate_degrees(angle + 90.0).then(Transform::translate(end.x, end.y));
        children.push(
            SemanticNode::new(SemanticKind::Marker {
                at: Point::ZERO,
                shape: marker_shape(edge.style),
                size: ARROW_SIZE,
                style: marker_style(edge.style),
            })
            .with_id(format!("{id_prefix}-{index}-arrow"))
            .with_transform(transform),
        );
    }

    scene.push(
        SemanticNode::group(children).with_id(format!("{id_prefix}-{index}")).with_role(SemanticRole::Edge).with_source(source),
    );
}

fn push_node(scene: &mut SemanticScene, node: &PlacedNode) {
    let source = SourceRef::element(node.id.clone());
    let id = format!("node-{}", node.id);
    scene.push(
        node_shape(node).with_id(id.clone()).with_role(SemanticRole::Node).with_source(source).with_hit_region(node.rect),
    );
}

fn push_node_label(scene: &mut SemanticScene, node: &PlacedNode, options: &RenderOptions) {
    if matches!(node.shape, NodeShape::Point) {
        return;
    }
    let source = SourceRef::element(node.id.clone());
    push_centered_text(
        scene,
        &format!("node-{}-label", node.id),
        &node.label,
        node.rect.center(),
        options.font_size,
        LabelKind::Node,
        source,
    );
}

fn push_edge_label(scene: &mut SemanticScene, id_prefix: &str, index: usize, edge: &RoutedEdge, options: &RenderOptions) {
    let (Some(label), Some(label_box)) = (edge.label.as_ref(), edge.label_box)
    else {
        return;
    };
    let source = SourceRef::element(format!("{}->{}", edge.from.node, edge.to.node));
    // The backing plate paints before the text so the routed line never
    // crosses readable glyphs.
    scene.push(
        SemanticNode::new(SemanticKind::RoundedRect { rect: label_box, radius: 2.0, style: label_backdrop_style() })
            .with_id(format!("{id_prefix}-{index}-label-bg"))
            .with_role(SemanticRole::EdgeLabelBackdrop)
            .with_source(source.clone()),
    );
    let font_size = crate::compile::edge_label_font_size(options.font_size);
    push_centered_text(
        scene,
        &format!("{id_prefix}-{index}-label"),
        label,
        label_box.center(),
        font_size,
        LabelKind::Edge,
        source,
    );
}

fn label_backdrop_style() -> ShapeStyle {
    ShapeStyle {
        fill: Some(DIAGRAM_THEME.light.edge_label_bg),
        stroke: None,
        stroke_width: 0.0,
        dash: None,
        opacity: EDGE_LABEL_BG_OPACITY,
    }
}

#[derive(Clone, Copy)]
enum LabelKind {
    Node,
    Edge,
}

fn push_centered_text(
    scene: &mut SemanticScene,
    id_prefix: &str,
    text: &str,
    center: Point,
    font_size: f64,
    kind: LabelKind,
    source: SourceRef,
) {
    let (fill, role) = match kind {
        LabelKind::Node => (NODE_TEXT, SemanticRole::Label),
        LabelKind::Edge => (EDGE_LABEL_TEXT, SemanticRole::EdgeLabel),
    };
    let lines: Vec<&str> = text.split('\n').collect();
    for (index, line) in lines.iter().enumerate() {
        let offset = (index as f64 - (lines.len() - 1) as f64 * 0.5) * font_size * LINE_HEIGHT;
        let position = Point::new(center.x, center.y + offset + font_size * BASELINE_SHIFT);
        let id = if lines.len() == 1 { id_prefix.to_string() } else { format!("{id_prefix}-{index}") };
        scene.push(
            SemanticNode::new(SemanticKind::Text {
                position,
                text: (*line).to_string(),
                style: label_text_style(font_size, fill),
            })
            .with_id(id)
            .with_role(role)
            .with_source(source.clone()),
        );
    }
}

fn cluster_label_id(cluster: &str, index: usize, line_count: usize) -> String {
    if line_count == 1 { format!("cluster-{cluster}-label") } else { format!("cluster-{cluster}-label-{index}") }
}

fn node_shape(node: &PlacedNode) -> SemanticNode {
    let style = node_style();
    let kind = match node.shape {
        NodeShape::Box | NodeShape::Subroutine => SemanticKind::Rect { rect: node.rect, style },
        NodeShape::Round => SemanticKind::RoundedRect { rect: node.rect, radius: ROUND_RADIUS, style },
        NodeShape::Stadium => SemanticKind::RoundedRect { rect: node.rect, radius: node.rect.height * 0.5, style },
        NodeShape::Cylinder => SemanticKind::Group { children: cylinder_children(&node.id, node.rect, style), opacity: 1.0 },
        NodeShape::Circle => {
            let radius = node.rect.width.min(node.rect.height) * 0.5;
            SemanticKind::Ellipse { center: node.rect.center(), radii: Size::new(radius, radius), style }
        }
        NodeShape::Rhombus => SemanticKind::Path { path: rhombus_path(node.rect), style },
        NodeShape::Hexagon => SemanticKind::Path { path: hexagon_path(node.rect), style },
        NodeShape::Triangle => SemanticKind::Path { path: triangle_path(node.rect), style },
        NodeShape::Star => SemanticKind::Path { path: star_path(node.rect), style },
        NodeShape::Trapezium => SemanticKind::Path { path: trapezium_path(node.rect), style },
        NodeShape::Parallelogram => SemanticKind::Path { path: parallelogram_path(node.rect), style },
        NodeShape::Note => SemanticKind::Group { children: note_children(&node.id, node.rect, style), opacity: 1.0 },
        NodeShape::Point => {
            let radius = node.rect.width.min(node.rect.height) * 0.5;
            SemanticKind::Ellipse {
                center: node.rect.center(),
                radii: Size::new(radius, radius),
                style: ShapeStyle { fill: Some(NODE_STROKE), stroke: None, ..ShapeStyle::default() },
            }
        }
        NodeShape::DoubleCircle => SemanticKind::Group { children: double_circle_children(&node.id, node.rect, style), opacity: 1.0 },
        NodeShape::Component => SemanticKind::Group { children: component_children(&node.id, node.rect, style), opacity: 1.0 },
        NodeShape::Ellipse => {
            SemanticKind::Ellipse { center: node.rect.center(), radii: Size::new(node.rect.width * 0.5, node.rect.height * 0.5), style }
        }
        NodeShape::Plaintext => {
            SemanticKind::Rect { rect: node.rect, style: ShapeStyle { fill: None, stroke: None, ..ShapeStyle::default() } }
        }
    };
    SemanticNode::new(kind)
}

fn polygon_path(points: &[Point]) -> PathData {
    let mut path = PathData::new();
    if let Some((first, rest)) = points.split_first() {
        path = path.move_to(*first);
        for point in rest {
            path = path.line_to(*point);
        }
        path = path.close();
    }
    path
}

fn hexagon_path(rect: Rect) -> PathData {
    let inset = rect.width * 0.25;
    let center = rect.center();
    polygon_path(&[
        Point::new(rect.x + inset, rect.y),
        Point::new(rect.right() - inset, rect.y),
        Point::new(rect.right(), center.y),
        Point::new(rect.right() - inset, rect.bottom()),
        Point::new(rect.x + inset, rect.bottom()),
        Point::new(rect.x, center.y),
    ])
}

fn triangle_path(rect: Rect) -> PathData {
    let center = rect.center();
    polygon_path(&[Point::new(center.x, rect.y), Point::new(rect.right(), rect.bottom()), Point::new(rect.x, rect.bottom())])
}

fn star_path(rect: Rect) -> PathData {
    let center = rect.center();
    let outer = rect.width.min(rect.height) * 0.5;
    let inner = outer * 0.382;
    let mut points = Vec::with_capacity(10);
    for index in 0..10 {
        let radius = if index % 2 == 0 { outer } else { inner };
        let angle = -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * index as f64 / 5.0;
        points.push(Point::new(center.x + radius * angle.cos(), center.y + radius * angle.sin()));
    }
    polygon_path(&points)
}

fn trapezium_path(rect: Rect) -> PathData {
    let inset = rect.width * 0.2;
    polygon_path(&[
        Point::new(rect.x + inset, rect.y),
        Point::new(rect.right() - inset, rect.y),
        Point::new(rect.right(), rect.bottom()),
        Point::new(rect.x, rect.bottom()),
    ])
}

fn parallelogram_path(rect: Rect) -> PathData {
    let slant = rect.width * 0.2;
    polygon_path(&[
        Point::new(rect.x + slant, rect.y),
        Point::new(rect.right(), rect.y),
        Point::new(rect.right() - slant, rect.bottom()),
        Point::new(rect.x, rect.bottom()),
    ])
}

fn note_children(id: &str, rect: Rect, style: ShapeStyle) -> Vec<SemanticNode> {
    let fold = (rect.width.min(rect.height) * 0.25).clamp(2.0, 10.0);
    let outline = polygon_path(&[
        Point::new(rect.x, rect.y),
        Point::new(rect.right() - fold, rect.y),
        Point::new(rect.right(), rect.y + fold),
        Point::new(rect.right(), rect.bottom()),
        Point::new(rect.x, rect.bottom()),
    ]);
    vec![
        SemanticNode::new(SemanticKind::Path { path: outline, style: style.clone() })
            .with_id(format!("node-{id}-outline")),
        SemanticNode::new(SemanticKind::Line {
            from: Point::new(rect.right() - fold, rect.y),
            to: Point::new(rect.right(), rect.y + fold),
            style,
        })
        .with_id(format!("node-{id}-fold")),
    ]
}

fn double_circle_children(id: &str, rect: Rect, style: ShapeStyle) -> Vec<SemanticNode> {
    let outer = Size::new(rect.width * 0.5, rect.height * 0.5);
    let inner = Size::new((rect.width * 0.5 - 2.0).max(1.0), (rect.height * 0.5 - 2.0).max(1.0));
    vec![
        SemanticNode::new(SemanticKind::Ellipse { center: rect.center(), radii: outer, style: style.clone() })
            .with_id(format!("node-{id}-outer")),
        SemanticNode::new(SemanticKind::Ellipse { center: rect.center(), radii: inner, style })
            .with_id(format!("node-{id}-inner")),
    ]
}

fn component_children(id: &str, rect: Rect, style: ShapeStyle) -> Vec<SemanticNode> {
    let ear_width = (rect.width * 0.12).clamp(2.0, 6.0);
    let ear_height = (rect.height * 0.18).clamp(3.0, 8.0);
    let body = Rect::new(rect.x + ear_width, rect.y, (rect.width - ear_width).max(0.0), rect.height);
    vec![
        SemanticNode::new(SemanticKind::Rect { rect: body, style: style.clone() }).with_id(format!("node-{id}-body")),
        SemanticNode::new(SemanticKind::Rect {
            rect: Rect::new(rect.x, rect.y + rect.height * 0.28 - ear_height * 0.5, ear_width, ear_height),
            style: style.clone(),
        })
        .with_id(format!("node-{id}-ear-top")),
        SemanticNode::new(SemanticKind::Rect {
            rect: Rect::new(rect.x, rect.y + rect.height * 0.72 - ear_height * 0.5, ear_width, ear_height),
            style,
        })
        .with_id(format!("node-{id}-ear-bottom")),
    ]
}

fn cylinder_children(id: &str, rect: Rect, style: ShapeStyle) -> Vec<SemanticNode> {
    let cap = (rect.height * 0.15).max(2.0);
    let center = rect.center();
    let body = Rect::new(rect.x, rect.y + cap, rect.width, (rect.height - cap * 2.0).max(0.0));
    vec![
        SemanticNode::new(SemanticKind::Rect { rect: body, style: style.clone() }).with_id(format!("node-{id}-body")),
        SemanticNode::new(SemanticKind::Ellipse {
            center: Point::new(center.x, rect.bottom() - cap),
            radii: Size::new(rect.width * 0.5, cap),
            style: style.clone(),
        })
        .with_id(format!("node-{id}-bottom")),
        SemanticNode::new(SemanticKind::Ellipse {
            center: Point::new(center.x, rect.y + cap),
            radii: Size::new(rect.width * 0.5, cap),
            style,
        })
        .with_id(format!("node-{id}-top")),
    ]
}

fn rhombus_path(rect: Rect) -> PathData {
    let center = rect.center();
    PathData::new()
        .move_to(Point::new(center.x, rect.y))
        .line_to(Point::new(rect.right(), center.y))
        .line_to(Point::new(center.x, rect.bottom()))
        .line_to(Point::new(rect.x, center.y))
        .close()
}

fn node_style() -> ShapeStyle {
    ShapeStyle { fill: Some(NODE_FILL), stroke: Some(NODE_STROKE), stroke_width: 1.0, dash: None, opacity: 1.0 }
}

fn cluster_style() -> ShapeStyle {
    ShapeStyle { fill: Some(CLUSTER_FILL), stroke: Some(CLUSTER_STROKE), stroke_width: 1.0, dash: None, opacity: 1.0 }
}

fn edge_style(style: EdgeStyle) -> ShapeStyle {
    let (width, dash) = match style {
        EdgeStyle::ThickArrow | EdgeStyle::BoldArrow | EdgeStyle::BoldLine => (BOLD_EDGE_WIDTH, None),
        EdgeStyle::DashedArrow | EdgeStyle::DashedLine => (1.0, Some(DASH_PATTERN)),
        EdgeStyle::DottedArrow | EdgeStyle::DottedLine => (1.0, Some(DOT_PATTERN)),
        _ => (1.0, None),
    };
    let mut paint = ShapeStyle::stroked(EDGE_COLOR, width);
    paint.dash = dash;
    paint
}

fn label_text_style(font_size: f64, fill: Color) -> TextStyle {
    TextStyle {
        fill: Some(fill),
        opacity: 1.0,
        font_family: Some("Trebuchet MS, Verdana, sans-serif".to_string()),
        font_size,
        font_weight: None,
        text_anchor: TextAnchor::Middle,
    }
}

fn has_arrow(style: EdgeStyle) -> bool {
    matches!(
        style,
        EdgeStyle::Arrow
            | EdgeStyle::DottedArrow
            | EdgeStyle::DashedArrow
            | EdgeStyle::ThickArrow
            | EdgeStyle::BoldArrow
            | EdgeStyle::HollowArrow
            | EdgeStyle::FilledDiamond
            | EdgeStyle::HollowDiamond
    )
}

fn marker_shape(style: EdgeStyle) -> MarkerShape {
    match style {
        EdgeStyle::FilledDiamond | EdgeStyle::HollowDiamond => MarkerShape::Diamond,
        _ => MarkerShape::Triangle,
    }
}

fn marker_style(style: EdgeStyle) -> ShapeStyle {
    match style {
        EdgeStyle::HollowArrow | EdgeStyle::HollowDiamond => ShapeStyle::stroked(EDGE_COLOR, 1.5),
        _ => ShapeStyle::filled(EDGE_COLOR),
    }
}
