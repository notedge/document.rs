//! 场景节点 arena。

use plotter_types::plot::NodeId;

use crate::node::{SceneNode, SceneNodeKind};

/// 带稳定 ID 分配的场景节点表。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct SceneArena {
    nodes: Vec<SceneNode>,
    next_id: u64,
}

impl SceneArena {
    /// 空 arena。
    pub fn new() -> Self {
        Self::default()
    }

    /// 分配 ID 并插入节点。
    pub fn insert(&mut self, kind: SceneNodeKind) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.push(SceneNode { id, kind });
        id
    }

    /// 按 ID 查找。
    pub fn get(&self, id: NodeId) -> Option<&SceneNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// 全部节点。
    pub fn nodes(&self) -> &[SceneNode] {
        &self.nodes
    }

    /// 节点数量。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
