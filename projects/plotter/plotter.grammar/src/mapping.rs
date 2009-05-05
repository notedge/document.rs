//! Mapping 与 aesthetic 表达式。

/// 视觉通道表达式（A1：列引用）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AestheticExpr {
    /// 引用数据列名。
    Column(String),
}

impl AestheticExpr {
    /// 列引用。
    pub fn column(name: impl Into<String>) -> Self {
        Self::Column(name.into())
    }

    /// 列名。
    pub fn column_name(&self) -> &str {
        match self {
            Self::Column(name) => name,
        }
    }
}

/// 数据到视觉通道的映射。
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Mapping {
    /// x 通道。
    pub x: Option<AestheticExpr>,
    /// y 通道。
    pub y: Option<AestheticExpr>,
    /// z 通道。
    pub z: Option<AestheticExpr>,
    /// 描边色。
    pub color: Option<AestheticExpr>,
    /// 填充色。
    pub fill: Option<AestheticExpr>,
    /// 尺寸。
    pub size: Option<AestheticExpr>,
    /// 形状。
    pub shape: Option<AestheticExpr>,
    /// 透明度。
    pub alpha: Option<AestheticExpr>,
    /// 分组。
    pub group: Option<AestheticExpr>,
    /// 标签。
    pub label: Option<AestheticExpr>,
}

impl Mapping {
    /// 设置 x、y 列映射。
    pub fn xy(x: impl Into<String>, y: impl Into<String>) -> Self {
        Self { x: Some(AestheticExpr::column(x)), y: Some(AestheticExpr::column(y)), ..Self::default() }
    }

    /// 设置 x、y、z 列映射。
    pub fn xyz(x: impl Into<String>, y: impl Into<String>, z: impl Into<String>) -> Self {
        Self {
            x: Some(AestheticExpr::column(x)),
            y: Some(AestheticExpr::column(y)),
            z: Some(AestheticExpr::column(z)),
            ..Self::default()
        }
    }

    /// 合并：右侧覆盖左侧已有通道。
    pub fn merge(&self, overlay: &Self) -> Self {
        Self {
            x: overlay.x.clone().or_else(|| self.x.clone()),
            y: overlay.y.clone().or_else(|| self.y.clone()),
            z: overlay.z.clone().or_else(|| self.z.clone()),
            color: overlay.color.clone().or_else(|| self.color.clone()),
            fill: overlay.fill.clone().or_else(|| self.fill.clone()),
            size: overlay.size.clone().or_else(|| self.size.clone()),
            shape: overlay.shape.clone().or_else(|| self.shape.clone()),
            alpha: overlay.alpha.clone().or_else(|| self.alpha.clone()),
            group: overlay.group.clone().or_else(|| self.group.clone()),
            label: overlay.label.clone().or_else(|| self.label.clone()),
        }
    }
}
