//! Apollo 列式数据合同 — 表、视图、缺失值、批次。
//!
//! 不含图形语法或数学求值。

#![deny(missing_docs)]

mod column;
mod graph;
mod grid;
mod table;
mod tree;

pub use column::{Column, FloatColumn, StringColumn};
pub use graph::{GraphData, GraphEdge, GraphNode};
pub use grid::GridData;
pub use table::ColumnTable;
pub use tree::{TreeData, TreeEdge, TreeNode};
