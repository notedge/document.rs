//! 拾取命中结果。

use super::{
    ids::{InteractionId, PrimitiveId, RowId},
    vec3::Vec3,
};

/// 统一拾取结果（CPU / GPU 后端同合同）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HitResult {
    /// 交互 ID。
    pub interaction: InteractionId,
    /// 图元 ID。
    pub primitive: PrimitiveId,
    /// 可选数据行。
    pub data_row: Option<RowId>,
    /// 可选世界坐标交点。
    pub world_position: Option<Vec3>,
}
