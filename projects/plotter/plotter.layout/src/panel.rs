//! 2D 面板布局（分面网格等）。

use plotter_types::plot::{Diagnostic, DiagnosticCode, Result};

/// 绘图面板矩形（场景坐标，y 向上）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelRect {
    /// 左边界。
    pub left: f64,
    /// 下边界。
    pub bottom: f64,
    /// 右边界。
    pub right: f64,
    /// 上边界。
    pub top: f64,
}

impl PanelRect {
    /// 构造。
    pub const fn new(left: f64, bottom: f64, right: f64, top: f64) -> Self {
        Self { left, bottom, right, top }
    }

    /// 宽度。
    pub fn width(self) -> f64 {
        self.right - self.left
    }

    /// 高度。
    pub fn height(self) -> f64 {
        self.top - self.bottom
    }
}

/// 在视口内布置一个或多个分面面板。
///
/// `outer_margin` 为左、下、右、上。面板按行主序从左上开始编号（ggplot `facet_wrap` 习惯）。
pub fn layout_facet_panels(
    viewport_width: f64,
    viewport_height: f64,
    outer_margin: (f64, f64, f64, f64),
    panel_count: usize,
    ncol: Option<usize>,
    gap: f64,
) -> Result<Vec<PanelRect>> {
    if panel_count == 0 {
        return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "分面面板数不能为 0"));
    }
    if gap < 0.0 {
        return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "分面间距不能为负"));
    }

    let (ml, mb, mr, mt) = outer_margin;
    let inner_left = ml;
    let inner_bottom = mb;
    let inner_right = viewport_width - mr;
    let inner_top = viewport_height - mt;
    let inner_w = inner_right - inner_left;
    let inner_h = inner_top - inner_bottom;
    if inner_w <= 0.0 || inner_h <= 0.0 {
        return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "外边距过大，无可用绘图区"));
    }

    let ncol = ncol.unwrap_or_else(|| ((panel_count as f64).sqrt().ceil() as usize).max(1)).max(1);
    let nrow = panel_count.div_ceil(ncol);
    let gap_x = if ncol > 1 { gap * (ncol as f64 - 1.0) } else { 0.0 };
    let gap_y = if nrow > 1 { gap * (nrow as f64 - 1.0) } else { 0.0 };
    let cell_w = (inner_w - gap_x) / ncol as f64;
    let cell_h = (inner_h - gap_y) / nrow as f64;
    if cell_w <= 0.0 || cell_h <= 0.0 {
        return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "分面网格单元尺寸无效"));
    }

    let mut panels = Vec::with_capacity(panel_count);
    for index in 0..panel_count {
        let col = index % ncol;
        let row_from_top = index / ncol;
        let row_from_bottom = nrow - 1 - row_from_top;
        let left = inner_left + col as f64 * (cell_w + gap);
        let bottom = inner_bottom + row_from_bottom as f64 * (cell_h + gap);
        panels.push(PanelRect::new(left, bottom, left + cell_w, bottom + cell_h));
    }
    Ok(panels)
}

/// 无分面时的单面板（整个内边距框）。
pub fn layout_single_panel(viewport_width: f64, viewport_height: f64, outer_margin: (f64, f64, f64, f64)) -> Result<PanelRect> {
    Ok(layout_facet_panels(viewport_width, viewport_height, outer_margin, 1, Some(1), 0.0)?[0])
}
