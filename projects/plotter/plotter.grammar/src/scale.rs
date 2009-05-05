//! Scale 合同。Scale 与 Coordinate 分离。

use plotter_types::plot::Interval;

/// Scale 种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ScaleKind {
    /// 连续线性。
    Continuous,
    /// 以 10 为底的对数（domain 须为正）。
    Log10,
}

/// 单个 scale 规格。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScaleSpec {
    /// 绑定的 aesthetic 名，例如 `x` / `y`。
    pub aesthetic: String,
    /// 种类。
    pub kind: ScaleKind,
    /// 可选显式 domain；`None` 表示由数据推断。
    pub domain: Option<Interval>,
}

impl ScaleSpec {
    /// 连续 x scale。
    pub fn continuous_x() -> Self {
        Self { aesthetic: "x".into(), kind: ScaleKind::Continuous, domain: None }
    }

    /// 连续 y scale。
    pub fn continuous_y() -> Self {
        Self { aesthetic: "y".into(), kind: ScaleKind::Continuous, domain: None }
    }

    /// log10 y scale。
    pub fn log10_y() -> Self {
        Self { aesthetic: "y".into(), kind: ScaleKind::Log10, domain: None }
    }

    /// log10 x scale。
    pub fn log10_x() -> Self {
        Self { aesthetic: "x".into(), kind: ScaleKind::Log10, domain: None }
    }

    /// 设置显式 domain。
    pub fn with_domain(mut self, domain: Interval) -> Self {
        self.domain = Some(domain);
        self
    }
}
