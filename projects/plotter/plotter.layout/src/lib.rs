//! Apollo 布局 — 面板、图、树位置与边路由。
//!
//! 输出供 Scene 编译使用的几何。不拥有 GPU 生命周期。

#![deny(missing_docs)]

mod graph_layout;
mod panel;
mod result;
mod tree_layout;

pub use graph_layout::{CircularLayout, ForceLayout, GraphLayout, GridLayout, LayeredLayout};
pub use panel::{PanelRect, layout_facet_panels, layout_single_panel};
pub use result::{EdgeRoute, LayoutOptions, LayoutPoint, LayoutResult, straight_routes};
pub use tree_layout::{RadialTreeLayout, TidyTreeLayout, TreeLayout};
