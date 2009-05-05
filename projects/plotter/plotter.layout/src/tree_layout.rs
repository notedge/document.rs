//! 树布局算法。

use plotter_data::TreeData;
use plotter_types::plot::Result;
use std::collections::HashMap;

use crate::result::{LayoutOptions, LayoutPoint, LayoutResult, straight_routes};

/// 树布局合同。
pub trait TreeLayout {
    /// 计算节点位置与边路由。
    fn layout(&self, tree: &TreeData, options: &LayoutOptions) -> Result<LayoutResult>;
}

/// Reingold–Tilford 风格整洁树（简化确定性版：按叶子从左到右分配 x，深度定 y）。
#[derive(Debug, Default, Clone, Copy)]
pub struct TidyTreeLayout;

impl TreeLayout for TidyTreeLayout {
    fn layout(&self, tree: &TreeData, options: &LayoutOptions) -> Result<LayoutResult> {
        tree.validate()?;
        let children = tree.children_map();
        let mut prelim: HashMap<&str, f64> = HashMap::new();
        let mut next_x = 0.0_f64;
        assign_prelim(tree.root.as_str(), &children, &mut prelim, &mut next_x);
        let max_x = next_x.max(1.0);
        let depths = depths_from_root(tree);
        let max_depth = depths.values().copied().max().unwrap_or(0);
        let mut positions = Vec::with_capacity(tree.nodes.len());
        for node in &tree.nodes {
            let px = prelim[node.id.as_str()];
            let depth = depths[&node.id.as_str()];
            let x = options.margin + (px + 0.5) / max_x * options.inner_width();
            let y = options.margin + options.inner_height()
                - (depth as f64 + 0.5) / (max_depth as f64 + 1.0).max(1.0) * options.inner_height();
            positions.push((node.id.clone(), LayoutPoint::new(x, y)));
        }
        let routes = straight_routes(tree.edges().into_iter().map(|e| (e.parent, e.child)), &positions);
        Ok(LayoutResult { positions, routes })
    }
}

fn assign_prelim<'a>(
    id: &'a str,
    children: &HashMap<&str, Vec<&'a str>>,
    prelim: &mut HashMap<&'a str, f64>,
    next_x: &mut f64,
) {
    let kids = children.get(id).map(Vec::as_slice).unwrap_or(&[]);
    if kids.is_empty() {
        prelim.insert(id, *next_x);
        *next_x += 1.0;
        return;
    }
    for &child in kids {
        assign_prelim(child, children, prelim, next_x);
    }
    let first = prelim[kids[0]];
    let last = prelim[kids[kids.len() - 1]];
    prelim.insert(id, 0.5 * (first + last));
}

fn depths_from_root(tree: &TreeData) -> HashMap<&str, usize> {
    let children = tree.children_map();
    let mut depths = HashMap::new();
    let mut stack = vec![(tree.root.as_str(), 0usize)];
    while let Some((id, depth)) = stack.pop() {
        depths.insert(id, depth);
        if let Some(kids) = children.get(id) {
            for &kid in kids {
                stack.push((kid, depth + 1));
            }
        }
    }
    depths
}

/// 径向树：tidy 后按深度映射到极坐标。
#[derive(Debug, Default, Clone, Copy)]
pub struct RadialTreeLayout;

impl TreeLayout for RadialTreeLayout {
    fn layout(&self, tree: &TreeData, options: &LayoutOptions) -> Result<LayoutResult> {
        let tidy = TidyTreeLayout.layout(tree, options)?;
        let depths = depths_from_root(tree);
        let max_depth = depths.values().copied().max().unwrap_or(0).max(1);
        let cx = options.margin + options.inner_width() * 0.5;
        let cy = options.margin + options.inner_height() * 0.5;
        let max_r = options.inner_width().min(options.inner_height()) * 0.45;

        // 用 tidy 的 x 序作为角度权重。
        let xs: Vec<f64> = tidy.positions.iter().map(|(_, p)| p.x).collect();
        let min_x = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let max_x = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let span = (max_x - min_x).max(1e-9);

        let mut positions = Vec::with_capacity(tidy.positions.len());
        for (id, p) in &tidy.positions {
            let depth = depths[id.as_str()];
            let t = (p.x - min_x) / span;
            let angle = t * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
            let r = if depth == 0 { 0.0 } else { max_r * (depth as f64) / max_depth as f64 };
            positions.push((id.clone(), LayoutPoint::new(cx + r * angle.cos(), cy + r * angle.sin())));
        }
        let routes = straight_routes(tree.edges().into_iter().map(|e| (e.parent, e.child)), &positions);
        Ok(LayoutResult { positions, routes })
    }
}
