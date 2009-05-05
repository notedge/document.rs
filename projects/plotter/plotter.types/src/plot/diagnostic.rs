//! 结构化诊断 — 语言无关。

use core::fmt;

/// 诊断严重级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Severity {
    /// 硬错误。
    Error,
    /// 警告。
    Warning,
}

/// 稳定诊断码（`apollo_*`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DiagnosticCode {
    /// 空数据或不完整列。
    EmptyData,
    /// 列名未知。
    UnknownColumn,
    /// 列长度不一致。
    ColumnLengthMismatch,
    /// 缺少必要 mapping。
    MissingMapping,
    /// 图层非法或为空。
    InvalidLayer,
    /// 当前阶段不支持的 geom / stat / coord。
    UnsupportedSpec,
    /// 校验失败（通用）。
    ValidationFailed,
    /// 渲染失败。
    RenderFailed,
    /// 渲染目标与后端不匹配。
    UnsupportedTarget,
}

impl DiagnosticCode {
    /// 线协议字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyData => "apollo_empty_data",
            Self::UnknownColumn => "apollo_unknown_column",
            Self::ColumnLengthMismatch => "apollo_column_length_mismatch",
            Self::MissingMapping => "apollo_missing_mapping",
            Self::InvalidLayer => "apollo_invalid_layer",
            Self::UnsupportedSpec => "apollo_unsupported_spec",
            Self::ValidationFailed => "apollo_validation_failed",
            Self::RenderFailed => "plotter_renderer_failed",
            Self::UnsupportedTarget => "apollo_unsupported_target",
        }
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一条结构化诊断。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    /// 严重级别。
    pub severity: Severity,
    /// 稳定码。
    pub code: DiagnosticCode,
    /// 结构化参数（语言无关）。
    pub params: Vec<(String, String)>,
    /// 可选人类可读说明（调试用，不是前端文案真相源）。
    pub message: Option<String>,
}

impl Diagnostic {
    /// 构造错误诊断。
    pub fn error(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, code, params: Vec::new(), message: Some(message.into()) }
    }

    /// 附加参数。
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.push((key.into(), value.into()));
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)?;
        if let Some(message) = &self.message {
            write!(f, ": {message}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostic {}

/// Apollo 合同层统一结果类型。
pub type Result<T> = core::result::Result<T, Diagnostic>;
