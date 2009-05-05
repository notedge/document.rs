//! 渲染目标。

use plotter_types::plot::Rgba;
use plotter_view::Viewport;

/// RGBA8 位图（行主序，原点在左上）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    /// 宽（像素）。
    pub width: u32,
    /// 高（像素）。
    pub height: u32,
    /// 像素：`width * height * 4`。
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    /// 按视口尺寸构造，背景填白。
    pub fn from_viewport(viewport: Viewport) -> Self {
        let width = viewport.width.max(1.0).round() as u32;
        let height = viewport.height.max(1.0).round() as u32;
        Self::filled(width, height, Rgba::WHITE)
    }

    /// 纯色填充。
    pub fn filled(width: u32, height: u32, color: Rgba) -> Self {
        let mut pixels = vec![0_u8; (width as usize) * (height as usize) * 4];
        let rgba = color_to_bytes(color);
        for chunk in pixels.as_chunks_mut::<4>().0 {
            chunk.copy_from_slice(&rgba);
        }
        Self { width, height, pixels }
    }

    /// 读像素。
    pub fn get(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        Some([self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3]])
    }

    /// 写像素（越界忽略）。
    pub fn set(&mut self, x: i32, y: i32, color: Rgba) {
        if x < 0 || y < 0 {
            return;
        }
        let x = x as u32;
        let y = y as u32;
        if x >= self.width || y >= self.height {
            return;
        }
        let i = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        let rgba = color_to_bytes(color);
        self.pixels[i..i + 4].copy_from_slice(&rgba);
    }

    /// 非白色像素计数（测试用）。
    pub fn non_white_count(&self) -> usize {
        self.pixels.as_chunks::<4>().0.iter().filter(|px| **px != [255, 255, 255, 255]).count()
    }

    /// 确定性指纹（FNV-1a 64），用于 golden 回归。
    pub fn fingerprint(&self) -> u64 {
        const OFFSET: u64 = 0xcbf29ce484222325;
        const PRIME: u64 = 0x100000001b3;
        let mut hash = OFFSET;
        hash = hash.wrapping_mul(PRIME) ^ u64::from(self.width);
        hash = hash.wrapping_mul(PRIME) ^ u64::from(self.height);
        for byte in &self.pixels {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        hash
    }
}

/// 渲染目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderTarget {
    /// CPU / 位图目标。
    Rgba8(RgbaImage),
    /// SVG 文档字符串。
    Svg(String),
}

/// 颜色转 RGBA8 字节。
pub fn color_to_bytes(color: Rgba) -> [u8; 4] {
    [
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.a.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

/// 颜色转 CSS。
pub fn color_to_css(color: Rgba) -> String {
    let [r, g, b, a] = color_to_bytes(color);
    if a == 255 { format!("rgb({r},{g},{b})") } else { format!("rgba({r},{g},{b},{})", f32::from(a) / 255.0) }
}
