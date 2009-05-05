#![deny(missing_docs)]
//! Plotter 共享原语：几何、颜色、变换、诊断，以及 GoG 侧的 `plot` 子模块。
//!
//! 本 crate 不依赖 `notedown-ir`、Oak、VuTeX 或具体渲染后端。
//! `plotter-scene` 在此之上建矢量场景；`plotter-diagram` / `plotter-chart` 降到该场景。
//!
//! **没有**独立的 `plotter-core`：原 apollo-types 已并入 [`plot`]。
//! **没有** `plotter-ir`：`PlotSpec` / `DiagramSpec` / Chart Spec 平行，不合并成单一 IR
//! （理由见仓库内 `projects/plotter/readme.md` §2）。

pub mod plot;

mod color;
mod diagnostics;
mod geometry;
mod rejections;
mod text_metrics;
mod theme;
mod transform;

pub use color::Color;
pub use diagnostics::{
    ByteRange, Diagnostic, DiagnosticLabel, DiagnosticLocation, DiagnosticOrigin, DiagnosticSeverity, LabelRole, Message,
    NAMESPACE, SourceRef, codes, diagnostic, origin,
};
pub use geometry::{Bounds, Point, Rect, Size};
pub use rejections::{CATALOG_JSON, CodeDoc, MatrixRow, RejectionCase, cases, cases_for, code_docs, matrix};
pub use text_metrics::{measure_line, wrap_label};
pub use theme::{DIAGRAM_THEME, DiagramTheme, EDGE_LABEL_BG_OPACITY, ThemeTokens, contrast_ratio, relative_luminance};
pub use transform::Transform;
