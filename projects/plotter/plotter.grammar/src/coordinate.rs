//! Coordinate 合同。与 Scale 独立组合。

use plotter_types::plot::Interval;

use crate::camera_plot::Camera3dSpec;

/// 二维笛卡尔坐标系参数。
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Cartesian2d {
    /// 覆盖 x domain；`None` 用 scale / 数据域。
    pub xlim: Option<Interval>,
    /// 覆盖 y domain；`None` 用 scale / 数据域。
    pub ylim: Option<Interval>,
    /// 交换 x/y 在场景中的轴向。
    pub flip: bool,
}

/// 三维笛卡尔坐标系。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cartesian3d {
    /// 相机（编译进 Scene）。
    pub camera: Camera3dSpec,
}

impl Default for Cartesian3d {
    fn default() -> Self {
        Self { camera: Camera3dSpec::perspective_default() }
    }
}

/// 图布局算法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum GraphLayoutKind {
    /// 圆周。
    #[default]
    Circular,
    /// 网格。
    Grid,
    /// 分层（有向优先）。
    Layered,
    /// 确定性力导向。
    Force {
        /// 迭代次数。
        iterations: u32,
    },
}

/// 树布局算法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TreeLayoutKind {
    /// 整洁树。
    #[default]
    Tidy,
    /// 径向树。
    Radial,
}

/// 图空间坐标。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GraphSpace {
    /// 布局算法。
    pub algorithm: GraphLayoutKind,
}

impl Default for GraphSpace {
    fn default() -> Self {
        Self { algorithm: GraphLayoutKind::Circular }
    }
}

/// 树空间坐标。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TreeSpace {
    /// 布局算法。
    pub algorithm: TreeLayoutKind,
}

impl Default for TreeSpace {
    fn default() -> Self {
        Self { algorithm: TreeLayoutKind::Tidy }
    }
}

/// 坐标系规格。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CoordinateSpec {
    /// 二维笛卡尔。
    Cartesian2d(Cartesian2d),
    /// 三维笛卡尔。
    Cartesian3d(Cartesian3d),
    /// 图布局空间。
    GraphSpace(GraphSpace),
    /// 树布局空间。
    TreeSpace(TreeSpace),
}

impl Default for CoordinateSpec {
    fn default() -> Self {
        Self::Cartesian2d(Cartesian2d::default())
    }
}

impl CoordinateSpec {
    /// 默认笛卡尔。
    pub fn cartesian_2d() -> Self {
        Self::default()
    }

    /// 带显式限域的笛卡尔。
    pub fn cartesian_limits(xlim: Option<Interval>, ylim: Option<Interval>) -> Self {
        Self::Cartesian2d(Cartesian2d { xlim, ylim, flip: false })
    }

    /// 翻转笛卡尔轴。
    pub fn cartesian_flip() -> Self {
        Self::Cartesian2d(Cartesian2d { xlim: None, ylim: None, flip: true })
    }

    /// 默认三维笛卡尔。
    pub fn cartesian_3d() -> Self {
        Self::Cartesian3d(Cartesian3d::default())
    }

    /// 自定义三维相机。
    pub fn cartesian_3d_camera(camera: Camera3dSpec) -> Self {
        Self::Cartesian3d(Cartesian3d { camera })
    }

    /// 默认图空间（圆周）。
    pub fn graph_space() -> Self {
        Self::GraphSpace(GraphSpace::default())
    }

    /// 指定图布局算法。
    pub fn graph_space_with(algorithm: GraphLayoutKind) -> Self {
        Self::GraphSpace(GraphSpace { algorithm })
    }

    /// 默认树空间（tidy）。
    pub fn tree_space() -> Self {
        Self::TreeSpace(TreeSpace::default())
    }

    /// 指定树布局算法。
    pub fn tree_space_with(algorithm: TreeLayoutKind) -> Self {
        Self::TreeSpace(TreeSpace { algorithm })
    }

    /// 取二维笛卡尔载荷。
    pub fn as_cartesian2d(&self) -> Option<&Cartesian2d> {
        match self {
            Self::Cartesian2d(c) => Some(c),
            _ => None,
        }
    }

    /// 取三维笛卡尔载荷。
    pub fn as_cartesian3d(&self) -> Option<&Cartesian3d> {
        match self {
            Self::Cartesian3d(c) => Some(c),
            _ => None,
        }
    }

    /// 取图空间载荷。
    pub fn as_graph_space(&self) -> Option<&GraphSpace> {
        match self {
            Self::GraphSpace(c) => Some(c),
            _ => None,
        }
    }

    /// 取树空间载荷。
    pub fn as_tree_space(&self) -> Option<&TreeSpace> {
        match self {
            Self::TreeSpace(c) => Some(c),
            _ => None,
        }
    }
}
