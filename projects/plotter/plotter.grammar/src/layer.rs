//! 图层、stat、geom、position。

use crate::mapping::Mapping;

/// 统计变换（A2：identity）。
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum StatSpec {
    /// 原样传递。
    #[default]
    Identity,
}

/// 折线几何参数。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeomLine {
    /// 线宽（场景单位）。
    pub linewidth: f32,
}

impl Default for GeomLine {
    fn default() -> Self {
        Self { linewidth: 1.0 }
    }
}

/// 散点几何参数。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeomPoint {
    /// 点半径（场景单位）。
    pub size: f32,
}

impl Default for GeomPoint {
    fn default() -> Self {
        Self { size: 3.0 }
    }
}

/// 柱几何参数。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeomBar {
    /// 柱宽（数据 x 单位）。
    pub width: f64,
}

impl Default for GeomBar {
    fn default() -> Self {
        Self { width: 0.8 }
    }
}

/// 文本几何参数。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeomText {
    /// 字号（场景单位）。
    pub size: f32,
    /// 无 `label` mapping 时使用的常量文本。
    pub text: Option<String>,
}

impl Default for GeomText {
    fn default() -> Self {
        Self { size: 12.0, text: None }
    }
}

/// 三维曲面（消费 `GridData`）。
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct GeomSurface {}

/// 三维点云（消费表的 x/y/z mapping）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeomPoint3d {
    /// 点半径。
    pub size: f32,
}

impl Default for GeomPoint3d {
    fn default() -> Self {
        Self { size: 3.0 }
    }
}

/// 图节点。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeomNode {
    /// 半径。
    pub size: f32,
}

impl Default for GeomNode {
    fn default() -> Self {
        Self { size: 4.0 }
    }
}

/// 图边。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeomEdge {
    /// 线宽。
    pub linewidth: f32,
}

impl Default for GeomEdge {
    fn default() -> Self {
        Self { linewidth: 1.0 }
    }
}

/// 树节点。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeomTreeNode {
    /// 半径。
    pub size: f32,
}

impl Default for GeomTreeNode {
    fn default() -> Self {
        Self { size: 4.0 }
    }
}

/// 树边。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeomTreeEdge {
    /// 线宽。
    pub linewidth: f32,
}

impl Default for GeomTreeEdge {
    fn default() -> Self {
        Self { linewidth: 1.0 }
    }
}

/// 几何标记。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GeomSpec {
    /// 折线。
    Line(GeomLine),
    /// 散点。
    Point(GeomPoint),
    /// 柱。
    Bar(GeomBar),
    /// 文本。
    Text(GeomText),
    /// 三维曲面。
    Surface(GeomSurface),
    /// 三维点云。
    Point3d(GeomPoint3d),
    /// 图节点。
    Node(GeomNode),
    /// 图边。
    Edge(GeomEdge),
    /// 树节点。
    TreeNode(GeomTreeNode),
    /// 树边。
    TreeEdge(GeomTreeEdge),
}

impl GeomSpec {
    /// 默认折线。
    pub fn line() -> Self {
        Self::Line(GeomLine::default())
    }

    /// 默认散点。
    pub fn point() -> Self {
        Self::Point(GeomPoint::default())
    }

    /// 默认柱。
    pub fn bar() -> Self {
        Self::Bar(GeomBar::default())
    }

    /// 默认文本。
    pub fn text() -> Self {
        Self::Text(GeomText::default())
    }

    /// 默认曲面。
    pub fn surface() -> Self {
        Self::Surface(GeomSurface {})
    }

    /// 默认三维点。
    pub fn point3d() -> Self {
        Self::Point3d(GeomPoint3d::default())
    }

    /// 默认图节点。
    pub fn node() -> Self {
        Self::Node(GeomNode::default())
    }

    /// 默认图边。
    pub fn edge() -> Self {
        Self::Edge(GeomEdge::default())
    }

    /// 默认树节点。
    pub fn tree_node() -> Self {
        Self::TreeNode(GeomTreeNode::default())
    }

    /// 默认树边。
    pub fn tree_edge() -> Self {
        Self::TreeEdge(GeomTreeEdge::default())
    }
}

/// 位置调整（A2：identity）。
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum PositionSpec {
    /// 不调整。
    #[default]
    Identity,
}

/// 图层附加参数（占位，避免把常量伪装成 mapping）。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct LayerParameters {}

/// 单层图形规格。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayerSpec {
    /// 可选覆盖数据键。`None` 表示沿用图级数据。
    pub data: Option<String>,
    /// 层内 mapping（与图级合并）。
    pub mapping: Mapping,
    /// 统计变换。
    pub stat: StatSpec,
    /// 几何。
    pub geom: GeomSpec,
    /// 位置调整。
    pub position: PositionSpec,
    /// 常量参数。
    pub parameters: LayerParameters,
}

impl LayerSpec {
    fn base(geom: GeomSpec) -> Self {
        Self {
            data: None,
            mapping: Mapping::default(),
            stat: StatSpec::Identity,
            geom,
            position: PositionSpec::Identity,
            parameters: LayerParameters::default(),
        }
    }

    /// 折线层。
    pub fn geom_line() -> Self {
        Self::base(GeomSpec::line())
    }

    /// 散点层。
    pub fn geom_point() -> Self {
        Self::base(GeomSpec::point())
    }

    /// 柱层。
    pub fn geom_bar() -> Self {
        Self::base(GeomSpec::bar())
    }

    /// 文本层。
    pub fn geom_text() -> Self {
        Self::base(GeomSpec::text())
    }

    /// 曲面层。
    pub fn geom_surface() -> Self {
        Self::base(GeomSpec::surface())
    }

    /// 三维点云层。
    pub fn geom_point3d() -> Self {
        Self::base(GeomSpec::point3d())
    }

    /// 图节点层。
    pub fn geom_node() -> Self {
        Self::base(GeomSpec::node())
    }

    /// 图边层。
    pub fn geom_edge() -> Self {
        Self::base(GeomSpec::edge())
    }

    /// 树节点层。
    pub fn geom_tree_node() -> Self {
        Self::base(GeomSpec::tree_node())
    }

    /// 树边层。
    pub fn geom_tree_edge() -> Self {
        Self::base(GeomSpec::tree_edge())
    }

    /// 覆盖层 mapping。
    pub fn mapping(mut self, mapping: Mapping) -> Self {
        self.mapping = mapping;
        self
    }
}
