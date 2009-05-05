//! 场景节点种类与几何载荷（2D + 3D mesh/point cloud）。

use plotter_types::plot::{InteractionId, Interval, NodeId, Rgba, Vec3};

/// 二维点（场景空间）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point2 {
    /// x。
    pub x: f64,
    /// y。
    pub y: f64,
}

impl Point2 {
    /// 构造点。
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// 折线节点。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PolylineNode {
    /// 折线顶点（已是场景坐标）。
    pub points: Vec<Point2>,
    /// 描边色。
    pub stroke: Rgba,
    /// 线宽。
    pub linewidth: f32,
}

/// 散点节点。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PointsNode {
    /// 点位置。
    pub positions: Vec<Point2>,
    /// 点半径（场景单位）。
    pub size: f32,
    /// 填充色。
    pub fill: Rgba,
}

/// 轴对齐矩形（bar 等）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rect2 {
    /// 左下角。
    pub min: Point2,
    /// 右上角。
    pub max: Point2,
}

/// 网格 / 填充多边形节点（A2：三角形列表，用于 bar）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeshNode {
    /// 顶点。
    pub positions: Vec<Point2>,
    /// 三角形索引（每 3 个一组）。
    pub indices: Vec<u32>,
    /// 填充色。
    pub fill: Rgba,
}

impl MeshNode {
    /// 由轴对齐矩形构造两个三角形。
    pub fn from_rect(rect: Rect2, fill: Rgba) -> Self {
        let positions = vec![
            Point2::new(rect.min.x, rect.min.y),
            Point2::new(rect.max.x, rect.min.y),
            Point2::new(rect.max.x, rect.max.y),
            Point2::new(rect.min.x, rect.max.y),
        ];
        Self { positions, indices: vec![0, 1, 2, 0, 2, 3], fill }
    }
}

/// 三维三角形网格（世界坐标）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Mesh3Node {
    /// 顶点。
    pub positions: Vec<Vec3>,
    /// 三角形索引。
    pub indices: Vec<u32>,
    /// 填充色。
    pub fill: Rgba,
    /// 可选交互 ID。
    pub interaction: Option<InteractionId>,
}

/// 三维点云（世界坐标）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Points3Node {
    /// 点位置。
    pub positions: Vec<Vec3>,
    /// 点半径（投影后像素近似）。
    pub size: f32,
    /// 填充色。
    pub fill: Rgba,
    /// 可选交互 ID。
    pub interaction: Option<InteractionId>,
}

/// 文本节点。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextNode {
    /// 锚点（场景坐标，基线起点近似）。
    pub position: Point2,
    /// 文本内容。
    pub content: String,
    /// 字号（场景单位，约等于像素）。
    pub size: f32,
    /// 颜色。
    pub color: Rgba,
}

/// 坐标轴节点（A2：线性刻度轴）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AxisNode {
    /// 是否为水平轴（x）。否则为竖直轴（y）。
    pub horizontal: bool,
    /// 数据域。
    pub domain: Interval,
    /// 轴在场景中的起点。
    pub origin: Point2,
    /// 轴长度（场景单位）。
    pub length: f64,
    /// 刻度数量（含两端）。
    pub tick_count: u32,
    /// 描边色。
    pub stroke: Rgba,
}

/// 场景节点种类。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SceneNodeKind {
    /// 分组。
    Group {
        /// 子节点。
        children: Vec<NodeId>,
    },
    /// 折线。
    Polyline(PolylineNode),
    /// 散点。
    Points(PointsNode),
    /// 填充网格。
    Mesh(MeshNode),
    /// 三维网格。
    Mesh3(Mesh3Node),
    /// 三维点云。
    Points3(Points3Node),
    /// 文本。
    Text(TextNode),
    /// 坐标轴。
    Axis(AxisNode),
}

/// Arena 中的场景节点。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SceneNode {
    /// 稳定 ID。
    pub id: NodeId,
    /// 种类与载荷。
    pub kind: SceneNodeKind,
}
