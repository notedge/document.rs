//! 图数据合同。

use plotter_types::plot::{Diagnostic, DiagnosticCode, Result};
use std::collections::{HashMap, HashSet};

/// 图节点。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GraphNode {
    /// 稳定键。
    pub id: String,
    /// 可选标签。
    pub label: Option<String>,
}

impl GraphNode {
    /// 仅 ID。
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), label: None }
    }

    /// 带标签。
    pub fn with_label(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: Some(label.into()) }
    }
}

/// 图边。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GraphEdge {
    /// 起点。
    pub source: String,
    /// 终点。
    pub target: String,
}

impl GraphEdge {
    /// 构造边。
    pub fn new(source: impl Into<String>, target: impl Into<String>) -> Self {
        Self { source: source.into(), target: target.into() }
    }
}

/// 图数据。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GraphData {
    /// 节点。
    pub nodes: Vec<GraphNode>,
    /// 边。
    pub edges: Vec<GraphEdge>,
    /// 是否有向。
    pub directed: bool,
}

impl GraphData {
    /// 无向图。
    pub fn undirected(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> Result<Self> {
        let graph = Self { nodes, edges, directed: false };
        graph.validate()?;
        Ok(graph)
    }

    /// 有向图。
    pub fn directed(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> Result<Self> {
        let graph = Self { nodes, edges, directed: true };
        graph.validate()?;
        Ok(graph)
    }

    /// 自检：非空、ID 唯一、边端点存在。
    pub fn validate(&self) -> Result<()> {
        if self.nodes.is_empty() {
            return Err(Diagnostic::error(DiagnosticCode::EmptyData, "图没有节点"));
        }
        let mut seen = HashSet::new();
        for node in &self.nodes {
            if node.id.is_empty() {
                return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "图节点 ID 为空"));
            }
            if !seen.insert(node.id.clone()) {
                return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, format!("图节点 ID 重复：`{}`", node.id))
                    .with_param("node", node.id.clone()));
            }
        }
        for edge in &self.edges {
            if !seen.contains(&edge.source) {
                return Err(Diagnostic::error(DiagnosticCode::UnknownColumn, format!("边起点 `{}` 不存在", edge.source))
                    .with_param("node", edge.source.clone()));
            }
            if !seen.contains(&edge.target) {
                return Err(Diagnostic::error(DiagnosticCode::UnknownColumn, format!("边终点 `{}` 不存在", edge.target))
                    .with_param("node", edge.target.clone()));
            }
        }
        Ok(())
    }

    /// 节点索引。
    pub fn index_of(&self) -> HashMap<&str, usize> {
        self.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect()
    }

    /// 邻接表（有向按出边；无向双向）。
    pub fn adjacency(&self) -> HashMap<&str, Vec<&str>> {
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for node in &self.nodes {
            adj.entry(node.id.as_str()).or_default();
        }
        for edge in &self.edges {
            adj.entry(edge.source.as_str()).or_default().push(edge.target.as_str());
            if !self.directed {
                adj.entry(edge.target.as_str()).or_default().push(edge.source.as_str());
            }
        }
        adj
    }
}
