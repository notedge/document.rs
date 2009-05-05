//! 图布局算法。

use plotter_data::GraphData;
use plotter_types::plot::{Diagnostic, DiagnosticCode, Result};
use std::collections::{HashMap, VecDeque};

use crate::result::{LayoutOptions, LayoutPoint, LayoutResult, straight_routes};

/// 图布局合同。
pub trait GraphLayout {
    /// 计算节点位置与边路由。
    fn layout(&self, graph: &GraphData, options: &LayoutOptions) -> Result<LayoutResult>;
}

/// 圆周布局（确定性）。
#[derive(Debug, Default, Clone, Copy)]
pub struct CircularLayout;

impl GraphLayout for CircularLayout {
    fn layout(&self, graph: &GraphData, options: &LayoutOptions) -> Result<LayoutResult> {
        graph.validate()?;
        let n = graph.nodes.len();
        let cx = options.margin + options.inner_width() * 0.5;
        let cy = options.margin + options.inner_height() * 0.5;
        let radius = options.inner_width().min(options.inner_height()) * 0.4;
        let mut positions = Vec::with_capacity(n);
        for (i, node) in graph.nodes.iter().enumerate() {
            let angle = std::f64::consts::TAU * (i as f64) / (n as f64).max(1.0) - std::f64::consts::FRAC_PI_2;
            positions.push((node.id.clone(), LayoutPoint::new(cx + radius * angle.cos(), cy + radius * angle.sin())));
        }
        let routes = straight_routes(graph.edges.iter().map(|e| (e.source.clone(), e.target.clone())), &positions);
        Ok(LayoutResult { positions, routes })
    }
}

/// 网格布局（按节点声明顺序行主序）。
#[derive(Debug, Default, Clone, Copy)]
pub struct GridLayout;

impl GraphLayout for GridLayout {
    fn layout(&self, graph: &GraphData, options: &LayoutOptions) -> Result<LayoutResult> {
        graph.validate()?;
        let n = graph.nodes.len();
        let cols = ((n as f64).sqrt().ceil() as usize).max(1);
        let rows = n.div_ceil(cols);
        let cell_w = options.inner_width() / cols as f64;
        let cell_h = options.inner_height() / rows as f64;
        let mut positions = Vec::with_capacity(n);
        for (i, node) in graph.nodes.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let x = options.margin + (col as f64 + 0.5) * cell_w;
            let y = options.margin + ((rows - 1 - row) as f64 + 0.5) * cell_h;
            positions.push((node.id.clone(), LayoutPoint::new(x, y)));
        }
        let routes = straight_routes(graph.edges.iter().map(|e| (e.source.clone(), e.target.clone())), &positions);
        Ok(LayoutResult { positions, routes })
    }
}

/// 分层布局（有向图：按最长路径分层；无向退化为 BFS 层）。
#[derive(Debug, Default, Clone, Copy)]
pub struct LayeredLayout;

impl GraphLayout for LayeredLayout {
    fn layout(&self, graph: &GraphData, options: &LayoutOptions) -> Result<LayoutResult> {
        graph.validate()?;
        let levels = assign_levels(graph);
        let max_level = levels.values().copied().max().unwrap_or(0);
        let mut by_level: Vec<Vec<&str>> = vec![Vec::new(); max_level + 1];
        for node in &graph.nodes {
            let level = levels[&node.id.as_str()];
            by_level[level].push(node.id.as_str());
        }
        let mut positions = Vec::with_capacity(graph.nodes.len());
        let layer_h = options.inner_height() / (max_level as f64 + 1.0).max(1.0);
        for (level, ids) in by_level.iter().enumerate() {
            let count = ids.len().max(1);
            let gap = options.inner_width() / count as f64;
            let y = options.margin + options.inner_height() - (level as f64 + 0.5) * layer_h;
            for (i, id) in ids.iter().enumerate() {
                let x = options.margin + (i as f64 + 0.5) * gap;
                positions.push(((*id).to_string(), LayoutPoint::new(x, y)));
            }
        }
        // 保持与 graph.nodes 声明顺序一致的 positions 更利于测试；重建为 nodes 顺序。
        let map: HashMap<&str, LayoutPoint> = positions.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let positions: Vec<(String, LayoutPoint)> = graph.nodes.iter().map(|n| (n.id.clone(), map[n.id.as_str()])).collect();
        let routes = straight_routes(graph.edges.iter().map(|e| (e.source.clone(), e.target.clone())), &positions);
        Ok(LayoutResult { positions, routes })
    }
}

fn assign_levels(graph: &GraphData) -> HashMap<&str, usize> {
    let adj = graph.adjacency();
    let mut level: HashMap<&str, usize> = HashMap::new();
    let mut queue = VecDeque::new();

    if graph.directed {
        let mut indeg: HashMap<&str, usize> = graph.nodes.iter().map(|n| (n.id.as_str(), 0usize)).collect();
        for edge in &graph.edges {
            *indeg.get_mut(edge.target.as_str()).unwrap() += 1;
        }
        let mut sources: Vec<&str> = indeg.iter().filter(|(_, d)| **d == 0).map(|(k, _)| *k).collect();
        sources.sort_unstable();
        if sources.is_empty() {
            sources.push(graph.nodes[0].id.as_str());
        }
        for s in sources {
            level.insert(s, 0);
            queue.push_back(s);
        }
        while let Some(u) = queue.pop_front() {
            let lu = level[&u];
            for &v in adj.get(u).into_iter().flatten() {
                let next = lu + 1;
                match level.get(&v).copied() {
                    Some(cur) if next <= cur => {}
                    _ => {
                        level.insert(v, next);
                        queue.push_back(v);
                    }
                }
            }
        }
    }
    else {
        let start = graph.nodes[0].id.as_str();
        level.insert(start, 0);
        queue.push_back(start);
        while let Some(u) = queue.pop_front() {
            let lu = level[&u];
            for &v in adj.get(u).into_iter().flatten() {
                if let std::collections::hash_map::Entry::Vacant(e) = level.entry(v) {
                    e.insert(lu + 1);
                    queue.push_back(v);
                }
            }
        }
    }

    for node in &graph.nodes {
        level.entry(node.id.as_str()).or_insert(0);
    }
    level
}

/// 确定性力导向（固定种子与迭代，非随机）。
#[derive(Debug, Clone, Copy)]
pub struct ForceLayout {
    /// 迭代次数。
    pub iterations: u32,
}

impl Default for ForceLayout {
    fn default() -> Self {
        Self { iterations: 64 }
    }
}

impl GraphLayout for ForceLayout {
    fn layout(&self, graph: &GraphData, options: &LayoutOptions) -> Result<LayoutResult> {
        graph.validate()?;
        if self.iterations == 0 {
            return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "force 迭代次数不能为 0"));
        }
        // 初始圆周。
        let mut result = CircularLayout.layout(graph, options)?;
        let n = result.positions.len();
        if n == 0 {
            return Ok(result);
        }
        let index: HashMap<String, usize> = result.positions.iter().enumerate().map(|(i, (id, _))| (id.clone(), i)).collect();
        let mut pos: Vec<LayoutPoint> = result.positions.iter().map(|(_, p)| *p).collect();
        let area = options.inner_width() * options.inner_height();
        let k = (area / n as f64).sqrt().max(1.0);
        let adj = graph.adjacency();
        let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (u, neis) in &adj {
            let ui = index[*u];
            for v in neis {
                let vi = index[*v];
                neighbors[ui].push(vi);
            }
        }
        for list in &mut neighbors {
            list.sort_unstable();
            list.dedup();
        }
        for iter in 0..self.iterations {
            let temp = 0.1 * (1.0 - f64::from(iter) / f64::from(self.iterations));
            let mut disp = vec![LayoutPoint::new(0.0, 0.0); n];
            for i in 0..n {
                for j in (i + 1)..n {
                    let dx = pos[i].x - pos[j].x;
                    let dy = pos[i].y - pos[j].y;
                    let dist = (dx * dx + dy * dy).sqrt().max(0.01);
                    let force = (k * k) / dist;
                    let fx = dx / dist * force;
                    let fy = dy / dist * force;
                    disp[i].x += fx;
                    disp[i].y += fy;
                    disp[j].x -= fx;
                    disp[j].y -= fy;
                }
            }
            for i in 0..n {
                for &j in &neighbors[i] {
                    if j <= i {
                        continue;
                    }
                    let dx = pos[i].x - pos[j].x;
                    let dy = pos[i].y - pos[j].y;
                    let dist = (dx * dx + dy * dy).sqrt().max(0.01);
                    let force = (dist * dist) / k;
                    let fx = dx / dist * force;
                    let fy = dy / dist * force;
                    disp[i].x -= fx;
                    disp[i].y -= fy;
                    disp[j].x += fx;
                    disp[j].y += fy;
                }
            }
            for i in 0..n {
                let d = (disp[i].x * disp[i].x + disp[i].y * disp[i].y).sqrt().max(0.01);
                pos[i].x += disp[i].x / d * temp.min(d) * options.inner_width();
                pos[i].y += disp[i].y / d * temp.min(d) * options.inner_height();
                pos[i].x = pos[i].x.clamp(options.margin, options.width - options.margin);
                pos[i].y = pos[i].y.clamp(options.margin, options.height - options.margin);
            }
        }
        for (i, (_, p)) in result.positions.iter_mut().enumerate() {
            *p = pos[i];
        }
        result.routes = straight_routes(graph.edges.iter().map(|e| (e.source.clone(), e.target.clone())), &result.positions);
        Ok(result)
    }
}
