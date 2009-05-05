//! Apollo SVG 矢量后端。
//!
//! 只消费 Scene IR。不是默认 GPU 路径；用于静态导出与文档。

#![deny(missing_docs)]

mod renderer;

pub use renderer::{SvgRenderer, render_svg};
