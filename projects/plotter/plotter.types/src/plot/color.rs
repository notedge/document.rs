//! 颜色合同。

/// 线性 sRGB，通道范围 `[0, 1]`。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rgba {
    /// 红。
    pub r: f32,
    /// 绿。
    pub g: f32,
    /// 蓝。
    pub b: f32,
    /// 不透明度。
    pub a: f32,
}

impl Rgba {
    /// 不透明黑。
    pub const BLACK: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    /// 不透明白。
    pub const WHITE: Self = Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
}
