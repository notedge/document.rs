//! 三维向量。

use core::ops::{Add, Mul, Sub};

/// 三维向量 / 点（世界坐标）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Vec3 {
    /// x。
    pub x: f64,
    /// y。
    pub y: f64,
    /// z。
    pub z: f64,
}

impl Vec3 {
    /// 构造。
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// 原点。
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    /// 单位 y。
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);

    /// 点积。
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// 叉积。
    pub fn cross(self, other: Self) -> Self {
        Self::new(self.y * other.z - self.z * other.y, self.z * other.x - self.x * other.z, self.x * other.y - self.y * other.x)
    }

    /// 长度。
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// 单位化；零向量返回 `None`。
    pub fn normalized(self) -> Option<Self> {
        let len = self.length();
        if len < f64::EPSILON { None } else { Some(self * (1.0 / len)) }
    }
}

impl Add for Vec3 {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;

    fn mul(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}
