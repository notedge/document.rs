//! 场景根对象。

use plotter_types::plot::NodeId;

use crate::{arena::SceneArena, camera::CameraSpec};

/// 视口（像素或逻辑像素）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Viewport {
    /// 宽。
    pub width: f64,
    /// 高。
    pub height: f64,
}

impl Viewport {
    /// 构造视口。
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new(640.0, 480.0)
    }
}

/// 场景元数据。
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct SceneMetadata {
    /// 可选标题。
    pub title: Option<String>,
}

/// 后端无关场景。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Scene {
    /// 根节点。
    pub root: NodeId,
    /// 节点 arena。
    pub nodes: SceneArena,
    /// 相机。
    pub camera: CameraSpec,
    /// 视口。
    pub viewport: Viewport,
    /// 元数据。
    pub metadata: SceneMetadata,
}

impl Scene {
    /// 由已填充的 arena 与根节点构造（默认 2D 正交相机）。
    pub fn from_arena(root: NodeId, nodes: SceneArena, viewport: Viewport) -> Self {
        Self { root, nodes, camera: CameraSpec::Orthographic2d, viewport, metadata: SceneMetadata::default() }
    }
}
