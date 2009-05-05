//! 主题：默认颜色与外边距。

use plotter_types::plot::Rgba;

/// 图主题。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ThemeSpec {
    /// 视口背景色（由渲染清屏或全幅矩形消费；编译期写入 Scene metadata 旁路暂不强制）。
    pub background: Rgba,
    /// 默认几何前景色（描边 / 填充 / 文本）。
    pub foreground: Rgba,
    /// 坐标轴颜色。
    pub axis_stroke: Rgba,
    /// 可选面板底色；`None` 不绘制面板底。
    pub panel_fill: Option<Rgba>,
    /// 外边距（左、下、右、上）。
    pub margin: (f64, f64, f64, f64),
    /// 分面面板间距。
    pub facet_gap: f64,
    /// 分面标题字号。
    pub facet_label_size: f32,
}

impl Default for ThemeSpec {
    fn default() -> Self {
        Self::light()
    }
}

impl ThemeSpec {
    /// 浅色主题（与既有 A2 黑线白底一致）。
    pub fn light() -> Self {
        Self {
            background: Rgba::WHITE,
            foreground: Rgba::BLACK,
            axis_stroke: Rgba::BLACK,
            panel_fill: None,
            margin: (48.0, 36.0, 16.0, 16.0),
            facet_gap: 8.0,
            facet_label_size: 10.0,
        }
    }

    /// 深色主题。
    pub fn dark() -> Self {
        Self {
            background: Rgba { r: 0.12, g: 0.12, b: 0.14, a: 1.0 },
            foreground: Rgba { r: 0.92, g: 0.92, b: 0.94, a: 1.0 },
            axis_stroke: Rgba { r: 0.75, g: 0.75, b: 0.78, a: 1.0 },
            panel_fill: Some(Rgba { r: 0.18, g: 0.18, b: 0.2, a: 1.0 }),
            margin: (48.0, 36.0, 16.0, 16.0),
            facet_gap: 8.0,
            facet_label_size: 10.0,
        }
    }
}
