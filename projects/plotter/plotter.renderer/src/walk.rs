//! 遍历 Scene 树并回调可绘制节点。

use plotter_types::plot::{Diagnostic, DiagnosticCode, NodeId, Result};
use plotter_view::{AxisNode, Mesh3Node, MeshNode, Points3Node, PointsNode, PolylineNode, Scene, SceneNodeKind, TextNode};

/// 可绘制图元。
#[derive(Debug)]
pub enum Drawable<'a> {
    /// 折线。
    Polyline(&'a PolylineNode),
    /// 散点。
    Points(&'a PointsNode),
    /// 填充网格。
    Mesh(&'a MeshNode),
    /// 三维网格。
    Mesh3(&'a Mesh3Node),
    /// 三维点云。
    Points3(&'a Points3Node),
    /// 文本。
    Text(&'a TextNode),
    /// 坐标轴。
    Axis(&'a AxisNode),
}

/// 前序遍历，对每个可绘制节点调用 `visitor`。
pub fn walk_drawables(scene: &Scene, mut visitor: impl FnMut(Drawable<'_>) -> Result<()>) -> Result<()> {
    walk_node(scene, scene.root, &mut visitor)
}

fn walk_node(scene: &Scene, id: NodeId, visitor: &mut impl FnMut(Drawable<'_>) -> Result<()>) -> Result<()> {
    let node = scene.nodes.get(id).ok_or_else(|| {
        Diagnostic::error(DiagnosticCode::RenderFailed, format!("缺失场景节点 {}", id.0)).with_param("node", id.0.to_string())
    })?;
    match &node.kind {
        SceneNodeKind::Group { children } => {
            for child in children {
                walk_node(scene, *child, visitor)?;
            }
        }
        SceneNodeKind::Polyline(polyline) => visitor(Drawable::Polyline(polyline))?,
        SceneNodeKind::Points(points) => visitor(Drawable::Points(points))?,
        SceneNodeKind::Mesh(mesh) => visitor(Drawable::Mesh(mesh))?,
        SceneNodeKind::Mesh3(mesh) => visitor(Drawable::Mesh3(mesh))?,
        SceneNodeKind::Points3(points) => visitor(Drawable::Points3(points))?,
        SceneNodeKind::Text(text) => visitor(Drawable::Text(text))?,
        SceneNodeKind::Axis(axis) => visitor(Drawable::Axis(axis))?,
    }
    Ok(())
}
