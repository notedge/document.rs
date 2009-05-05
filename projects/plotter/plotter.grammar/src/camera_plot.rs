//! PlotSpec 侧的 3D 相机描述（编译为 Scene `CameraSpec`）。

use plotter_types::plot::Vec3;
use plotter_view::CameraSpec;

/// 语法层三维相机。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Camera3dSpec {
    /// 透视。
    Perspective {
        /// 眼睛。
        eye: Vec3,
        /// 目标。
        target: Vec3,
        /// 上方向。
        up: Vec3,
        /// 垂直视场角（度）。
        fovy_degrees: f64,
        /// 近平面。
        near: f64,
        /// 远平面。
        far: f64,
    },
    /// 正交。
    Orthographic {
        /// 眼睛。
        eye: Vec3,
        /// 目标。
        target: Vec3,
        /// 上方向。
        up: Vec3,
        /// 半高。
        half_extent_y: f64,
        /// 近平面。
        near: f64,
        /// 远平面。
        far: f64,
    },
}

impl Camera3dSpec {
    /// 默认透视。
    pub fn perspective_default() -> Self {
        Self::Perspective {
            eye: Vec3::new(2.5, 2.0, 2.5),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fovy_degrees: 45.0,
            near: 0.1,
            far: 100.0,
        }
    }

    /// 转为 Scene 相机。
    pub fn to_scene_camera(&self) -> CameraSpec {
        match self {
            Self::Perspective { eye, target, up, fovy_degrees, near, far } => CameraSpec::Perspective {
                eye: *eye,
                target: *target,
                up: *up,
                fovy_degrees: *fovy_degrees,
                near: *near,
                far: *far,
            },
            Self::Orthographic { eye, target, up, half_extent_y, near, far } => CameraSpec::Orthographic3d {
                eye: *eye,
                target: *target,
                up: *up,
                half_extent_y: *half_extent_y,
                near: *near,
                far: *far,
            },
        }
    }
}
