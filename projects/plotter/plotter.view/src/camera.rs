//! 相机规格与世界→屏幕投影。

use plotter_types::plot::{Diagnostic, DiagnosticCode, Result, Vec3};

use crate::scene::Viewport;

/// 相机规格。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum CameraSpec {
    /// 正交二维（场景坐标即像素平面，y 向上）。
    #[default]
    Orthographic2d,
    /// 透视三维。
    Perspective {
        /// 眼睛。
        eye: Vec3,
        /// 看向。
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
    /// 正交三维。
    Orthographic3d {
        /// 眼睛。
        eye: Vec3,
        /// 看向。
        target: Vec3,
        /// 上方向。
        up: Vec3,
        /// 半高（世界单位）。
        half_extent_y: f64,
        /// 近平面。
        near: f64,
        /// 远平面。
        far: f64,
    },
}

impl CameraSpec {
    /// 默认透视相机（看向原点）。
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

    /// 是否为纯 2D 正交。
    pub fn is_2d(&self) -> bool {
        matches!(self, Self::Orthographic2d)
    }

    /// 眼点（2D 返回 `None`）。
    pub fn eye(&self) -> Option<Vec3> {
        match self {
            Self::Orthographic2d => None,
            Self::Perspective { eye, .. } | Self::Orthographic3d { eye, .. } => Some(*eye),
        }
    }
}

/// 屏幕空间投影结果（像素，y 向上的场景习惯再由渲染器翻转）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPoint {
    /// 像素 x。
    pub x: f64,
    /// 像素 y（与 2D Scene 一致：原点在视口左下）。
    pub y: f64,
    /// 深度：越大越远（用于画家算法）。
    pub depth: f64,
}

/// 世界坐标 → 视口像素（左下为原点）。
pub fn project_to_screen(camera: &CameraSpec, viewport: Viewport, point: Vec3) -> Result<ScreenPoint> {
    match camera {
        CameraSpec::Orthographic2d => Ok(ScreenPoint { x: point.x, y: point.y, depth: point.z }),
        CameraSpec::Perspective { eye, target, up, fovy_degrees, near, far } => {
            let (nx, ny, nz) = view_project_perspective(point, *eye, *target, *up, *fovy_degrees, viewport, *near, *far)?;
            Ok(ndc_to_screen(nx, ny, nz, viewport))
        }
        CameraSpec::Orthographic3d { eye, target, up, half_extent_y, near, far } => {
            let (nx, ny, nz) = view_project_ortho(point, *eye, *target, *up, *half_extent_y, viewport, *near, *far)?;
            Ok(ndc_to_screen(nx, ny, nz, viewport))
        }
    }
}

/// 投影；近平面后或非法相机时返回 `None`（绘制路径跳过用）。
pub fn try_project_to_screen(camera: &CameraSpec, viewport: Viewport, point: Vec3) -> Option<ScreenPoint> {
    project_to_screen(camera, viewport, point).ok()
}

fn ndc_to_screen(nx: f64, ny: f64, nz: f64, viewport: Viewport) -> ScreenPoint {
    ScreenPoint { x: (nx * 0.5 + 0.5) * viewport.width, y: (ny * 0.5 + 0.5) * viewport.height, depth: nz }
}

fn look_basis(eye: Vec3, target: Vec3, up: Vec3) -> Result<(Vec3, Vec3, Vec3)> {
    let forward = (target - eye)
        .normalized()
        .ok_or_else(|| Diagnostic::error(DiagnosticCode::ValidationFailed, "相机 eye 与 target 重合"))?;
    let right = forward
        .cross(up)
        .normalized()
        .ok_or_else(|| Diagnostic::error(DiagnosticCode::ValidationFailed, "相机 up 与视线平行"))?;
    let cam_up = right
        .cross(forward)
        .normalized()
        .ok_or_else(|| Diagnostic::error(DiagnosticCode::ValidationFailed, "无法构造相机上方向"))?;
    Ok((right, cam_up, forward))
}

fn to_view(point: Vec3, eye: Vec3, right: Vec3, cam_up: Vec3, forward: Vec3) -> Vec3 {
    let d = point - eye;
    Vec3::new(d.dot(right), d.dot(cam_up), d.dot(forward))
}

#[allow(clippy::too_many_arguments)]
fn view_project_perspective(
    point: Vec3,
    eye: Vec3,
    target: Vec3,
    up: Vec3,
    fovy_degrees: f64,
    viewport: Viewport,
    near: f64,
    far: f64,
) -> Result<(f64, f64, f64)> {
    let (right, cam_up, forward) = look_basis(eye, target, up)?;
    let view = to_view(point, eye, right, cam_up, forward);
    if view.z <= near {
        return Err(Diagnostic::error(DiagnosticCode::RenderFailed, "点在近平面之后"));
    }
    let aspect = viewport.width / viewport.height.max(1e-9);
    let f = 1.0 / (fovy_degrees.to_radians() * 0.5).tan();
    let nx = (view.x * f / aspect) / view.z;
    let ny = (view.y * f) / view.z;
    let nz = (view.z - near) / (far - near);
    Ok((nx, ny, nz))
}

#[allow(clippy::too_many_arguments)]
fn view_project_ortho(
    point: Vec3,
    eye: Vec3,
    target: Vec3,
    up: Vec3,
    half_extent_y: f64,
    viewport: Viewport,
    near: f64,
    far: f64,
) -> Result<(f64, f64, f64)> {
    let (right, cam_up, forward) = look_basis(eye, target, up)?;
    let view = to_view(point, eye, right, cam_up, forward);
    let aspect = viewport.width / viewport.height.max(1e-9);
    let half_x = half_extent_y * aspect;
    let nx = view.x / half_x;
    let ny = view.y / half_extent_y;
    let nz = (view.z - near) / (far - near);
    Ok((nx, ny, nz))
}

/// 由屏幕像素（左下原点）生成世界空间拾取射线。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// 起点。
    pub origin: Vec3,
    /// 单位方向。
    pub direction: Vec3,
}

/// 屏幕点 → 世界射线（仅 3D 相机）。
pub fn screen_to_ray(camera: &CameraSpec, viewport: Viewport, screen_x: f64, screen_y: f64) -> Result<Ray> {
    let ndc_x = (screen_x / viewport.width) * 2.0 - 1.0;
    let ndc_y = (screen_y / viewport.height) * 2.0 - 1.0;
    match camera {
        CameraSpec::Orthographic2d => Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "2D 正交相机不支持射线拾取")),
        CameraSpec::Perspective { eye, target, up, fovy_degrees, .. } => {
            let (right, cam_up, forward) = look_basis(*eye, *target, *up)?;
            let aspect = viewport.width / viewport.height.max(1e-9);
            let f = 1.0 / (fovy_degrees.to_radians() * 0.5).tan();
            let dir_cam = Vec3::new(ndc_x * aspect / f, ndc_y / f, 1.0)
                .normalized()
                .ok_or_else(|| Diagnostic::error(DiagnosticCode::ValidationFailed, "拾取方向无效"))?;
            let direction = (right * dir_cam.x + cam_up * dir_cam.y + forward * dir_cam.z)
                .normalized()
                .ok_or_else(|| Diagnostic::error(DiagnosticCode::ValidationFailed, "拾取方向无效"))?;
            Ok(Ray { origin: *eye, direction })
        }
        CameraSpec::Orthographic3d { eye, target, up, half_extent_y, near, .. } => {
            let (right, cam_up, forward) = look_basis(*eye, *target, *up)?;
            let aspect = viewport.width / viewport.height.max(1e-9);
            let half_x = *half_extent_y * aspect;
            let origin = *eye + right * (ndc_x * half_x) + cam_up * (ndc_y * *half_extent_y) + forward * *near;
            Ok(Ray { origin, direction: forward })
        }
    }
}
