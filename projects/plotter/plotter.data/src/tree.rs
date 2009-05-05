//! 树数据合同。

use plotter_types::plot::{Diagnostic, DiagnosticCode, Result};
use std::collections::{HashMap, HashSet};

/// 树节点。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TreeNode {
    /// 稳定键。
    pub id: String,
    /// 父节点；根为 `None`。
    pub parent: Option<String>,
    /// 可选标签。
    pub label: Option<String>,
}

impl TreeNode {
    /// 根节点。
    pub fn root(id: impl Into<String>) -> Self {
        Self { id: id.into(), parent: None, label: None }
    }

    /// 子节点。
    pub fn child(id: impl Into<String>, parent: impl Into<String>) -> Self {
        Self { id: id.into(), parent: Some(parent.into()), label: None }
    }
}

/// 树边（parent → child）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TreeEdge {
    /// 父。
    pub parent: String,
    /// 子。
    pub child: String,
}

/// 树数据（单根）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TreeData {
    /// 根 ID。
    pub root: String,
    /// 节点。
    pub nodes: Vec<TreeNode>,
}

impl TreeData {
    /// 构造并校验。
    pub fn new(root: impl Into<String>, nodes: Vec<TreeNode>) -> Result<Self> {
        let tree = Self { root: root.into(), nodes };
        tree.validate()?;
        Ok(tree)
    }

    /// 由 parent 指针导出边。
    pub fn edges(&self) -> Vec<TreeEdge> {
        self.nodes
            .iter()
            .filter_map(|node| node.parent.as_ref().map(|parent| TreeEdge { parent: parent.clone(), child: node.id.clone() }))
            .collect()
    }

    /// 子节点表（按节点声明顺序）。
    pub fn children_map(&self) -> HashMap<&str, Vec<&str>> {
        let mut map: HashMap<&str, Vec<&str>> = HashMap::new();
        for node in &self.nodes {
            map.entry(node.id.as_str()).or_default();
        }
        for node in &self.nodes {
            if let Some(parent) = &node.parent {
                map.entry(parent.as_str()).or_default().push(node.id.as_str());
            }
        }
        map
    }

    /// 自检：单根、parent 唯一、无环、根存在且无父。
    pub fn validate(&self) -> Result<()> {
        if self.nodes.is_empty() {
            return Err(Diagnostic::error(DiagnosticCode::EmptyData, "树没有节点"));
        }
        let mut seen = HashSet::new();
        let mut roots = 0_usize;
        let mut has_declared_root = false;
        for node in &self.nodes {
            if node.id.is_empty() {
                return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "树节点 ID 为空"));
            }
            if !seen.insert(node.id.clone()) {
                return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, format!("树节点 ID 重复：`{}`", node.id))
                    .with_param("node", node.id.clone()));
            }
            if node.parent.is_none() {
                roots += 1;
            }
            if node.id == self.root {
                has_declared_root = true;
                if node.parent.is_some() {
                    return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "声明的根不能有 parent")
                        .with_param("root", self.root.clone()));
                }
            }
        }
        if !has_declared_root {
            return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, format!("根 `{}` 不在节点列表中", self.root))
                .with_param("root", self.root.clone()));
        }
        if roots != 1 {
            return Err(Diagnostic::error(
                DiagnosticCode::ValidationFailed,
                format!("树必须恰好一个根，当前 {roots} 个（森林请用显式 ForestData，禁止静默选根）"),
            ));
        }
        for node in &self.nodes {
            if let Some(parent) = &node.parent
                && !seen.contains(parent)
            {
                return Err(Diagnostic::error(DiagnosticCode::UnknownColumn, format!("父节点 `{}` 不存在", parent))
                    .with_param("node", parent.clone()));
            }
        }
        // 环检测：沿 parent 上行。
        for node in &self.nodes {
            let mut cursor = node.parent.as_deref();
            let mut guard = HashSet::new();
            guard.insert(node.id.as_str());
            while let Some(pid) = cursor {
                if !guard.insert(pid) {
                    return Err(
                        Diagnostic::error(DiagnosticCode::ValidationFailed, "树存在环").with_param("node", node.id.clone())
                    );
                }
                cursor = self.nodes.iter().find(|n| n.id == pid).and_then(|n| n.parent.as_deref());
            }
        }
        // 连通：所有节点可达根。
        let children = self.children_map();
        let mut reachable = HashSet::new();
        let mut stack = vec![self.root.as_str()];
        while let Some(id) = stack.pop() {
            if reachable.insert(id)
                && let Some(kids) = children.get(id)
            {
                stack.extend(kids.iter().copied());
            }
        }
        if reachable.len() != self.nodes.len() {
            return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "存在无法从根到达的节点"));
        }
        Ok(())
    }
}
