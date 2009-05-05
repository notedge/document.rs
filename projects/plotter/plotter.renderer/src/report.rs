//! 渲染能力与帧报告。

/// 单项能力状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// 可用。
    Available,
    /// 加速路径可用。
    Accelerated,
    /// 不支持。
    Unsupported,
}

/// 渲染器能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RendererCapabilities {
    /// 2D 矢量/栅格。
    pub raster_2d: Capability,
    /// SVG 导出。
    pub svg: Capability,
    /// GPU。
    pub gpu: Capability,
}

/// 单帧渲染报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameReport {
    /// 绘制的图元数量（折线段、轴线、刻度等近似计数）。
    pub primitive_count: u32,
}
