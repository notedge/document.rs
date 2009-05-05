//! 分面规格。

/// 分面布局。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FacetSpec {
    /// 按单列水平包装（`facet_wrap`）。
    Wrap {
        /// 分面列名（字符串列）。
        column: String,
        /// 列数；`None` 时按平方根取整。
        ncol: Option<usize>,
    },
}

impl FacetSpec {
    /// 单列 wrap。
    pub fn wrap(column: impl Into<String>) -> Self {
        Self::Wrap { column: column.into(), ncol: None }
    }

    /// 指定列数的 wrap。
    pub fn wrap_ncol(column: impl Into<String>, ncol: usize) -> Self {
        Self::Wrap { column: column.into(), ncol: Some(ncol.max(1)) }
    }
}
