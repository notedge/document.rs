//! 规则网格数据（曲面采样结果）。

use plotter_types::plot::{Diagnostic, DiagnosticCode, Result, Vec3};

/// 规则矩形网格：`z` 按行主序 `iy * nx + ix`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GridData {
    /// x 采样（长度 `nx`）。
    pub x: Vec<f64>,
    /// y 采样（长度 `ny`）。
    pub y: Vec<f64>,
    /// 高度值（长度 `nx * ny`）。
    pub z: Vec<f64>,
}

impl GridData {
    /// 构造并校验尺寸。
    pub fn new(x: Vec<f64>, y: Vec<f64>, z: Vec<f64>) -> Result<Self> {
        let grid = Self { x, y, z };
        grid.validate()?;
        Ok(grid)
    }

    /// `nx`。
    pub fn nx(&self) -> usize {
        self.x.len()
    }

    /// `ny`。
    pub fn ny(&self) -> usize {
        self.y.len()
    }

    /// 自检。
    pub fn validate(&self) -> Result<()> {
        let nx = self.nx();
        let ny = self.ny();
        if nx < 2 || ny < 2 {
            return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "GridData 至少需要 2×2 采样"));
        }
        if self.z.len() != nx * ny {
            return Err(Diagnostic::error(
                DiagnosticCode::ColumnLengthMismatch,
                format!("z 长度 {} 与 nx*ny={} 不一致", self.z.len(), nx * ny),
            ));
        }
        for (name, values) in [("x", &self.x), ("y", &self.y), ("z", &self.z)] {
            if values.iter().any(|v| !v.is_finite()) {
                return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, format!("GridData.{name} 含非有限值"))
                    .with_param("field", name));
            }
        }
        Ok(())
    }

    /// 取 `z[iy, ix]`。
    pub fn z_at(&self, ix: usize, iy: usize) -> f64 {
        self.z[iy * self.nx() + ix]
    }

    /// 三角化为世界坐标顶点与索引（两三角形/格元）。
    pub fn triangulate(&self) -> (Vec<Vec3>, Vec<u32>) {
        let nx = self.nx();
        let ny = self.ny();
        let mut positions = Vec::with_capacity(nx * ny);
        for iy in 0..ny {
            for ix in 0..nx {
                positions.push(Vec3::new(self.x[ix], self.y[iy], self.z_at(ix, iy)));
            }
        }
        let mut indices = Vec::with_capacity((nx - 1) * (ny - 1) * 6);
        for iy in 0..(ny - 1) {
            for ix in 0..(nx - 1) {
                let i00 = (iy * nx + ix) as u32;
                let i10 = i00 + 1;
                let i01 = ((iy + 1) * nx + ix) as u32;
                let i11 = i01 + 1;
                indices.extend_from_slice(&[i00, i10, i11, i00, i11, i01]);
            }
        }
        (positions, indices)
    }
}
