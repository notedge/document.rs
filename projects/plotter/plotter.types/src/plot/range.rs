//! 数值区间合同。

/// 闭区间 `[min, max]`。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Interval {
    /// 下界。
    pub min: f64,
    /// 上界。
    pub max: f64,
}

impl Interval {
    /// 构造区间。若 `min > max` 则交换。
    pub fn new(min: f64, max: f64) -> Self {
        if min <= max { Self { min, max } } else { Self { min: max, max: min } }
    }

    /// 宽度。
    pub fn span(self) -> f64 {
        self.max - self.min
    }

    /// 是否包含值。
    pub fn contains(self, value: f64) -> bool {
        value >= self.min && value <= self.max
    }
}
