//! 已准备场景。

use plotter_view::Scene;

/// A2：持有 Scene 克隆。后续可挂 GPU 资源句柄。
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedScene {
    /// 源场景。
    pub scene: Scene,
}

impl PreparedScene {
    /// 由 Scene 构造。
    pub fn from_scene(scene: &Scene) -> Self {
        Self { scene: scene.clone() }
    }
}
