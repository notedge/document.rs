//! 列类型。

use plotter_types::plot::{Diagnostic, DiagnosticCode, Result};

/// 浮点列。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FloatColumn {
    /// 列名。
    pub name: String,
    /// 数值。可用 `NaN` 表示缺失。
    pub values: Vec<f64>,
}

impl FloatColumn {
    /// 由名称与数值构造。
    pub fn new(name: impl Into<String>, values: Vec<f64>) -> Self {
        Self { name: name.into(), values }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// 字符串列（标签 / 分类文本）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StringColumn {
    /// 列名。
    pub name: String,
    /// 文本值。
    pub values: Vec<String>,
}

impl StringColumn {
    /// 由名称与文本构造。
    pub fn new(name: impl Into<String>, values: Vec<String>) -> Self {
        Self { name: name.into(), values }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// 列枚举。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Column {
    /// 浮点列。
    Float(FloatColumn),
    /// 字符串列。
    String(StringColumn),
}

impl Column {
    /// 列名。
    pub fn name(&self) -> &str {
        match self {
            Self::Float(column) => &column.name,
            Self::String(column) => &column.name,
        }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        match self {
            Self::Float(column) => column.len(),
            Self::String(column) => column.len(),
        }
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 取浮点视图。
    pub fn as_float(&self) -> Result<&FloatColumn> {
        match self {
            Self::Float(column) => Ok(column),
            Self::String(_) => {
                Err(Diagnostic::error(DiagnosticCode::ValidationFailed, format!("列 `{}` 不是浮点列", self.name()))
                    .with_param("column", self.name()))
            }
        }
    }

    /// 取字符串视图。
    pub fn as_string(&self) -> Result<&StringColumn> {
        match self {
            Self::String(column) => Ok(column),
            Self::Float(_) => {
                Err(Diagnostic::error(DiagnosticCode::ValidationFailed, format!("列 `{}` 不是字符串列", self.name()))
                    .with_param("column", self.name()))
            }
        }
    }
}

impl From<FloatColumn> for Column {
    fn from(value: FloatColumn) -> Self {
        Self::Float(value)
    }
}

impl From<StringColumn> for Column {
    fn from(value: StringColumn) -> Self {
        Self::String(value)
    }
}
