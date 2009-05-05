//! 列式表。

use plotter_types::plot::{Diagnostic, DiagnosticCode, Result};

use crate::column::{Column, FloatColumn, StringColumn};

/// 列式数据表。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ColumnTable {
    columns: Vec<Column>,
}

impl ColumnTable {
    /// 空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加浮点列。
    pub fn push_float(mut self, name: impl Into<String>, values: Vec<f64>) -> Result<Self> {
        let column = FloatColumn::new(name, values);
        self.insert(column.into())?;
        Ok(self)
    }

    /// 追加字符串列。
    pub fn push_string(mut self, name: impl Into<String>, values: Vec<String>) -> Result<Self> {
        let column = StringColumn::new(name, values);
        self.insert(column.into())?;
        Ok(self)
    }

    /// 插入列；名称冲突或长度不一致则失败。
    pub fn insert(&mut self, column: Column) -> Result<()> {
        if column.is_empty() {
            return Err(Diagnostic::error(DiagnosticCode::EmptyData, format!("列 `{}` 为空", column.name()))
                .with_param("column", column.name()));
        }
        if self.columns.iter().any(|existing| existing.name() == column.name()) {
            return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, format!("列名重复：`{}`", column.name()))
                .with_param("column", column.name()));
        }
        if let Some(first) = self.columns.first()
            && first.len() != column.len()
        {
            return Err(Diagnostic::error(
                DiagnosticCode::ColumnLengthMismatch,
                format!("列 `{}` 长度 {} 与既有列长度 {} 不一致", column.name(), column.len(), first.len()),
            )
            .with_param("column", column.name())
            .with_param("len", column.len().to_string())
            .with_param("expected_len", first.len().to_string()));
        }
        self.columns.push(column);
        Ok(())
    }

    /// 列切片。
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// 按名查找列。
    pub fn column(&self, name: &str) -> Result<&Column> {
        self.columns.iter().find(|column| column.name() == name).ok_or_else(|| {
            Diagnostic::error(DiagnosticCode::UnknownColumn, format!("未知列 `{name}`")).with_param("column", name)
        })
    }

    /// 按名取浮点列。
    pub fn float_column(&self, name: &str) -> Result<&FloatColumn> {
        self.column(name)?.as_float()
    }

    /// 按名取字符串列。
    pub fn string_column(&self, name: &str) -> Result<&StringColumn> {
        self.column(name)?.as_string()
    }

    /// 行数；空表为 0。
    pub fn row_count(&self) -> usize {
        self.columns.first().map(Column::len).unwrap_or(0)
    }

    /// 是否无列。
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// 按行索引子集（保持列顺序）。索引越界则失败。
    pub fn select_rows(&self, indices: &[usize]) -> Result<Self> {
        if indices.is_empty() {
            return Err(Diagnostic::error(DiagnosticCode::EmptyData, "行子集为空"));
        }
        let rows = self.row_count();
        let mut out = Self::new();
        for column in &self.columns {
            match column {
                Column::Float(col) => {
                    let mut values = Vec::with_capacity(indices.len());
                    for &index in indices {
                        let value = col.values.get(index).copied().ok_or_else(|| {
                            Diagnostic::error(DiagnosticCode::ValidationFailed, format!("行索引 {index} 越界（行数 {rows}）"))
                                .with_param("row", index.to_string())
                        })?;
                        values.push(value);
                    }
                    out.insert(FloatColumn::new(col.name.clone(), values).into())?;
                }
                Column::String(col) => {
                    let mut values = Vec::with_capacity(indices.len());
                    for &index in indices {
                        let value = col.values.get(index).cloned().ok_or_else(|| {
                            Diagnostic::error(DiagnosticCode::ValidationFailed, format!("行索引 {index} 越界（行数 {rows}）"))
                                .with_param("row", index.to_string())
                        })?;
                        values.push(value);
                    }
                    out.insert(StringColumn::new(col.name.clone(), values).into())?;
                }
            }
        }
        Ok(out)
    }

    /// 基础自检。
    pub fn validate(&self) -> Result<()> {
        if self.columns.is_empty() {
            return Err(Diagnostic::error(DiagnosticCode::EmptyData, "表没有任何列"));
        }
        let expected = self.row_count();
        if expected == 0 {
            return Err(Diagnostic::error(DiagnosticCode::EmptyData, "表行数为 0"));
        }
        for column in &self.columns {
            if column.len() != expected {
                return Err(Diagnostic::error(
                    DiagnosticCode::ColumnLengthMismatch,
                    format!("列 `{}` 长度不一致", column.name()),
                )
                .with_param("column", column.name()));
            }
        }
        Ok(())
    }
}
