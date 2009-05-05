//! 布局结果合同。

/// 布局坐标点。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutPoint {
    /// x。
    pub x: f64,
    /// y。
    pub y: f64,
}

impl LayoutPoint {
    /// 构造。
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// 布局画布选项。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutOptions {
    /// 宽。
    pub width: f64,
    /// 高。
    pub height: f64,
    /// 外边距。
    pub margin: f64,
}

impl LayoutOptions {
    /// 构造。
    pub const fn new(width: f64, height: f64, margin: f64) -> Self {
        Self { width, height, margin }
    }

    /// 内区左下。
    pub fn origin(self) -> LayoutPoint {
        LayoutPoint::new(self.margin, self.margin)
    }

    /// 内区宽。
    pub fn inner_width(self) -> f64 {
        (self.width - 2.0 * self.margin).max(1.0)
    }

    /// 内区高。
    pub fn inner_height(self) -> f64 {
        (self.height - 2.0 * self.margin).max(1.0)
    }
}

/// 边折线路由。
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeRoute {
    /// 起点 ID。
    pub source: String,
    /// 终点 ID。
    pub target: String,
    /// 折线点（至少两端）。
    pub points: Vec<LayoutPoint>,
}

/// 布局输出：节点位置 + 边路由（不生成 Scene 节点）。
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutResult {
    /// `(id, position)`，顺序稳定。
    pub positions: Vec<(String, LayoutPoint)>,
    /// 边路由。
    pub routes: Vec<EdgeRoute>,
}

impl LayoutResult {
    /// 查位置。
    pub fn position_of(&self, id: &str) -> Option<LayoutPoint> {
        self.positions.iter().find(|(k, _)| k == id).map(|(_, p)| *p)
    }
}

/// 由节点位置生成直线边。
pub fn straight_routes(
    edges: impl IntoIterator<Item = (String, String)>,
    positions: &[(String, LayoutPoint)],
) -> Vec<EdgeRoute> {
    let lookup = |id: &str| positions.iter().find(|(k, _)| k == id).map(|(_, p)| *p);
    edges
        .into_iter()
        .filter_map(|(source, target)| {
            let a = lookup(&source)?;
            let b = lookup(&target)?;
            Some(EdgeRoute { source, target, points: vec![a, b] })
        })
        .collect()
}
