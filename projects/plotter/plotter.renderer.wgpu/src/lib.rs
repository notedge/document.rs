//! 可选的 Apollo WGPU / WebGPU 后端 — shader、pipeline、GPU 资源。
//!
//! 不是默认核心依赖。CPU 仍是正确性参考实现。
//! 本阶段将 Scene 折线/轴线离屏渲染到 RGBA8，便于与 CPU 对照。

#![deny(missing_docs)]

mod geometry;
mod renderer;
mod shader;

pub use renderer::{WgpuRenderer, render_rgba8_wgpu};
