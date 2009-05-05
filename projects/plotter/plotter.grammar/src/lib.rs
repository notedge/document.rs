//! Apollo Grammar of Graphics — `PlotSpec`、mapping、stat、geom、scale、coord、facet、theme。
//!
//! 向 Scene IR 编译。不绘制像素，也不调用设备 API。

#![deny(missing_docs)]

mod camera_plot;
mod compile;
mod compile_3d;
mod compile_graph;
mod coordinate;
mod facet;
mod layer;
mod mapping;
mod plot;
mod scale;
mod theme;
mod validate;

pub use camera_plot::Camera3dSpec;
pub use compile::{CompileOptions, compile_plot};
pub use coordinate::{Cartesian2d, Cartesian3d, CoordinateSpec, GraphLayoutKind, GraphSpace, TreeLayoutKind, TreeSpace};
pub use facet::FacetSpec;
pub use layer::{
    GeomBar, GeomEdge, GeomLine, GeomNode, GeomPoint, GeomPoint3d, GeomSpec, GeomSurface, GeomText, GeomTreeEdge, GeomTreeNode,
    LayerParameters, LayerSpec, PositionSpec, StatSpec,
};
pub use mapping::{AestheticExpr, Mapping};
pub use plot::{DataRef, PlotSpec};
pub use scale::{ScaleKind, ScaleSpec};
pub use theme::ThemeSpec;
pub use validate::validate_plot;
