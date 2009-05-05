//! Apollo 渲染器合同 — CPU reference 与后端共用合同。
//!
//! 只消费 Scene IR。不得重新推断 stat、scale 或 layout。
//! SVG / WGPU 实现分别在 `apollo-backend-svg` 与 `apollo-backend-wgpu`。

#![deny(missing_docs)]

mod cpu;
mod font;
mod prepare;
mod report;
mod target;
mod walk;

pub use cpu::CpuRasterRenderer;
pub use prepare::PreparedScene;
pub use report::{Capability, FrameReport, RendererCapabilities};
pub use target::{RenderTarget, RgbaImage, color_to_bytes, color_to_css};
pub use walk::{Drawable, walk_drawables};

use plotter_types::plot::Result;
use plotter_view::Scene;

/// 渲染器合同：只消费已验证的 Scene IR。
pub trait Renderer {
    /// 能力探测。
    fn capabilities(&self) -> RendererCapabilities;

    /// 准备场景（A2：克隆持有，后续可做资源上传）。
    fn prepare(&mut self, scene: &Scene) -> Result<PreparedScene>;

    /// 渲染到目标。
    fn render(&mut self, scene: &PreparedScene, target: &mut RenderTarget) -> Result<FrameReport>;
}

/// 便捷：Scene → RGBA8 位图（CPU reference）。
pub fn render_rgba8(scene: &Scene) -> Result<RgbaImage> {
    let mut renderer = CpuRasterRenderer::new();
    let prepared = renderer.prepare(scene)?;
    let mut target = RenderTarget::Rgba8(RgbaImage::from_viewport(scene.viewport));
    renderer.render(&prepared, &mut target)?;
    match target {
        RenderTarget::Rgba8(image) => Ok(image),
        RenderTarget::Svg(_) => unreachable!("cpu renderer only writes rgba8"),
    }
}
