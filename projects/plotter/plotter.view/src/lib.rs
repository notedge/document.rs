//! Apollo Scene IR — 唯一的后端无关渲染合同。
//!
//! Arena + 稳定 ID。不含 ggplot 语义，也不含设备 / GPU API。

#![deny(missing_docs)]

mod arena;
mod camera;
mod node;
mod pick;
mod scene;

pub use arena::SceneArena;
pub use camera::{CameraSpec, Ray, ScreenPoint, project_to_screen, screen_to_ray, try_project_to_screen};
pub use node::{
    AxisNode, Mesh3Node, MeshNode, Point2, Points3Node, PointsNode, PolylineNode, Rect2, SceneNode, SceneNodeKind, TextNode,
};
pub use pick::pick_at;
pub use plotter_types::plot::NodeId as SceneNodeId;
pub use scene::{Scene, SceneMetadata, Viewport};
