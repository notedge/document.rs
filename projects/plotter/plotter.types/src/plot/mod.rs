//! Grammar of Graphics 原语类型（原 apollo-types）。
//!
//! 放在 `plot` 下，避免与 crate 根上的 diagram/chart 原语
//! （`Diagnostic` / `Color` 等）撞名。

mod color;
mod diagnostic;
mod hit;
mod ids;
mod range;
mod vec3;

pub use color::Rgba;
pub use diagnostic::{Diagnostic, DiagnosticCode, Result, Severity};
pub use hit::HitResult;
pub use ids::{InteractionId, NodeId, PrimitiveId, RowId, SerializationVersion};
pub use range::Interval;
pub use vec3::Vec3;
