//! 冒烟：PNG 图像经 `acorn-codec` 的 zlib 解码成功，超限数据被 `CodecLimits` 拒绝。

use document_presentation::display::{DisplayImage, DisplayItem, DisplayPage, DisplayPresentation};
use document_renderer_pdf::render_pdf;

/// PNG 文件签名，解码器据此识别格式。
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// 把字节串编成 base64，测试用它拼 `data:` URI。
fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for group in data.chunks(3) {
        let bytes = [group[0], *group.get(1).unwrap_or(&0), *group.get(2).unwrap_or(&0)];
        let packed = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        for index in 0..4 {
            if index <= group.len() {
                out.push(char::from(TABLE[(packed >> (18 - index * 6)) as usize & 0x3F]));
            }
            else {
                out.push('=');
            }
        }
    }
    out
}

/// Adler-32 校验和，zlib 结尾的四个字节。
fn adler32(data: &[u8]) -> u32 {
    let (mut low, mut high) = (1u32, 0u32);
    for &byte in data {
        low = (low + u32::from(byte)) % 65_521;
        high = (high + low) % 65_521;
    }
    (high << 16) | low
}

/// 一个 PNG 块：长度、类型、正文、CRC（解码器不校验 CRC，填零即可）。
fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 12);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

/// 8 位灰度 IHDR。
fn ihdr(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&[8, 0, 0, 0, 0]);
    out
}

/// 用一段存储块 zlib 流构造单 IDAT 的 PNG。
fn png(width: u32, height: u32, samples: &[u8]) -> Vec<u8> {
    let mut zlib = vec![0x78, 0x01, 0x01];
    zlib.extend_from_slice(&(samples.len() as u16).to_le_bytes());
    zlib.extend_from_slice(&(!(samples.len() as u16)).to_le_bytes());
    zlib.extend_from_slice(samples);
    zlib.extend_from_slice(&adler32(samples).to_be_bytes());

    let mut out = PNG_SIGNATURE.to_vec();
    out.extend_from_slice(&chunk(b"IHDR", &ihdr(width, height)));
    out.extend_from_slice(&chunk(b"IDAT", &zlib));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

fn render(source: String) -> Vec<u8> {
    render_pdf(&DisplayPresentation {
        pages: vec![DisplayPage {
            width: 200.0,
            height: 200.0,
            items: vec![DisplayItem::Image(DisplayImage {
                x: 10.0,
                y: 10.0,
                width: 100.0,
                height: 100.0,
                alt: "smoke".to_string(),
                source: Some(source),
            })],
        }],
    })
    .expect("pdf render")
}

#[test]
fn valid_zlib_samples_reach_the_pdf_as_an_image_xobject() {
    let raw = png(1, 1, &[0x00, 0x80]);
    let text = String::from_utf8_lossy(&render(format!("data:image/png;base64,{}", base64(&raw)))).into_owned();
    assert!(text.contains("/Subtype /Image"), "raster becomes an image XObject: {text}");
    assert!(text.contains("/Width 1") && text.contains("/Height 1"), "frame size survives: {text}");
    assert!(text.contains("/ColorSpace /DeviceGray"), "grayscale stays grayscale: {text}");
}

#[test]
fn samples_over_the_header_budget_are_rejected() {
    // 头部声明 1x1 灰度（期望两个字节），IDAT 却解出三个字节。
    let raw = png(1, 1, &[0x00, 0x80, 0x80]);
    let text = String::from_utf8_lossy(&render(format!("data:image/png;base64,{}", base64(&raw)))).into_owned();
    assert!(!text.contains("/Subtype /Image"), "CodecLimits 拒绝超限输出，槽位保留虚线占位: {text}");
}
