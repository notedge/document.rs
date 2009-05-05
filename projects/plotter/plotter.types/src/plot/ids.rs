//! 稳定标识与版本标记。

/// Scene / 资源节点稳定 ID（arena 友好）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NodeId(pub u64);

/// 交互命中 ID（业务侧 lookup，不进任意用户对象）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct InteractionId(pub u64);

/// 图元索引（例如三角形序号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PrimitiveId(pub u64);

/// 数据行 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RowId(pub u64);

/// 序列化合同版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SerializationVersion(pub u32);

impl SerializationVersion {
    /// 当前合同版本。
    pub const CURRENT: Self = Self(1);
}
