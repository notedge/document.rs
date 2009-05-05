//! 图 / 树 PlotSpec → layout → Scene。

use plotter_layout::{
    CircularLayout, ForceLayout, GraphLayout, GridLayout, LayeredLayout, LayoutOptions, LayoutResult, RadialTreeLayout,
    TidyTreeLayout, TreeLayout,
};
use plotter_types::plot::{Diagnostic, DiagnosticCode, Result};
use plotter_view::{CameraSpec, Point2, PointsNode, PolylineNode, Scene, SceneArena, SceneMetadata, SceneNodeKind};

use crate::{
    compile::CompileOptions,
    coordinate::{GraphLayoutKind, GraphSpace, TreeLayoutKind, TreeSpace},
    layer::GeomSpec,
    plot::{DataRef, PlotSpec},
};

pub(crate) fn compile_plot_graph(spec: &PlotSpec, options: CompileOptions, space: &GraphSpace) -> Result<Scene> {
    let DataRef::Graph(graph) = &spec.data
    else {
        return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "graph-space 需要 GraphData"));
    };
    let margin = options.margin.unwrap_or(spec.theme.margin).0.min(options.margin.unwrap_or(spec.theme.margin).1);
    let layout_opts = LayoutOptions::new(options.viewport.width, options.viewport.height, margin.max(8.0));
    let laid = run_graph_layout(graph, space.algorithm, &layout_opts)?;
    scene_from_layout(spec, options, &laid)
}

pub(crate) fn compile_plot_tree(spec: &PlotSpec, options: CompileOptions, space: &TreeSpace) -> Result<Scene> {
    let DataRef::Tree(tree) = &spec.data
    else {
        return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "tree-space 需要 TreeData"));
    };
    let margin = options.margin.unwrap_or(spec.theme.margin).0.min(options.margin.unwrap_or(spec.theme.margin).1);
    let layout_opts = LayoutOptions::new(options.viewport.width, options.viewport.height, margin.max(8.0));
    let laid = run_tree_layout(tree, space.algorithm, &layout_opts)?;
    scene_from_layout(spec, options, &laid)
}

fn run_graph_layout(
    graph: &plotter_data::GraphData,
    algorithm: GraphLayoutKind,
    options: &LayoutOptions,
) -> Result<LayoutResult> {
    match algorithm {
        GraphLayoutKind::Circular => CircularLayout.layout(graph, options),
        GraphLayoutKind::Grid => GridLayout.layout(graph, options),
        GraphLayoutKind::Layered => LayeredLayout.layout(graph, options),
        GraphLayoutKind::Force { iterations } => ForceLayout { iterations }.layout(graph, options),
    }
}

fn run_tree_layout(tree: &plotter_data::TreeData, algorithm: TreeLayoutKind, options: &LayoutOptions) -> Result<LayoutResult> {
    match algorithm {
        TreeLayoutKind::Tidy => TidyTreeLayout.layout(tree, options),
        TreeLayoutKind::Radial => RadialTreeLayout.layout(tree, options),
    }
}

fn scene_from_layout(spec: &PlotSpec, options: CompileOptions, laid: &LayoutResult) -> Result<Scene> {
    let theme = &spec.theme;
    let mut arena = SceneArena::new();
    let mut children = Vec::new();

    for (layer_index, layer) in spec.layers.iter().enumerate() {
        match &layer.geom {
            GeomSpec::Edge(edge) => {
                for route in &laid.routes {
                    let points: Vec<Point2> = route.points.iter().map(|p| Point2::new(p.x, p.y)).collect();
                    children.push(arena.insert(SceneNodeKind::Polyline(PolylineNode {
                        points,
                        stroke: theme.foreground,
                        linewidth: edge.linewidth,
                    })));
                }
            }
            GeomSpec::TreeEdge(edge) => {
                for route in &laid.routes {
                    let points: Vec<Point2> = route.points.iter().map(|p| Point2::new(p.x, p.y)).collect();
                    children.push(arena.insert(SceneNodeKind::Polyline(PolylineNode {
                        points,
                        stroke: theme.foreground,
                        linewidth: edge.linewidth,
                    })));
                }
            }
            GeomSpec::Node(node) => {
                let positions: Vec<Point2> = laid.positions.iter().map(|(_, p)| Point2::new(p.x, p.y)).collect();
                children.push(arena.insert(SceneNodeKind::Points(PointsNode {
                    positions,
                    size: node.size,
                    fill: theme.foreground,
                })));
            }
            GeomSpec::TreeNode(node) => {
                let positions: Vec<Point2> = laid.positions.iter().map(|(_, p)| Point2::new(p.x, p.y)).collect();
                children.push(arena.insert(SceneNodeKind::Points(PointsNode {
                    positions,
                    size: node.size,
                    fill: theme.foreground,
                })));
            }
            _ => {
                return Err(Diagnostic::error(
                    DiagnosticCode::UnsupportedSpec,
                    "图/树坐标系仅支持 node/edge 或 tree_node/tree_edge",
                )
                .with_param("layer", layer_index.to_string()));
            }
        }
    }

    let root = arena.insert(SceneNodeKind::Group { children });
    Ok(Scene {
        root,
        nodes: arena,
        camera: CameraSpec::Orthographic2d,
        viewport: options.viewport,
        metadata: SceneMetadata::default(),
    })
}
