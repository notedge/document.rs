//! Intrinsic image dimensions.
//!
//! Layout reserves a box for every authored image and lets the renderer resolve
//! the bytes. The box must still keep the image's aspect ratio, otherwise a
//! wide figure is padded like a square. Reading the header is not reading
//! pixels, so this stays independent of any image decoder.

/// Width over height from a PNG, JPEG or GIF header.
pub(crate) fn aspect_ratio(bytes: &[u8]) -> Option<f64> {
    png(bytes).or_else(|| gif(bytes)).or_else(|| jpeg(bytes))
}

fn ratio(width: u32, height: u32) -> Option<f64> {
    (width > 0 && height > 0).then(|| f64::from(width) / f64::from(height))
}

fn png(bytes: &[u8]) -> Option<f64> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || !bytes.starts_with(SIGNATURE) || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    ratio(width, height)
}

fn gif(bytes: &[u8]) -> Option<f64> {
    if bytes.len() < 10 || !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return None;
    }
    let width = u16::from_le_bytes(bytes[6..8].try_into().ok()?);
    let height = u16::from_le_bytes(bytes[8..10].try_into().ok()?);
    ratio(u32::from(width), u32::from(height))
}

fn jpeg(bytes: &[u8]) -> Option<f64> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut offset = 2;
    while offset + 4 <= bytes.len() {
        if bytes[offset] != 0xFF {
            return None;
        }
        let marker = bytes[offset + 1];
        match marker {
            0xD8 | 0x01 | 0xD0..=0xD7 | 0xFF => offset += 2,
            // SOF0..SOF15 carry the frame size. DHT, JPG and DAC reuse three of
            // those codes and are not frame headers.
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                if offset + 9 > bytes.len() {
                    return None;
                }
                let height = u16::from_be_bytes(bytes[offset + 5..offset + 7].try_into().ok()?);
                let width = u16::from_be_bytes(bytes[offset + 7..offset + 9].try_into().ok()?);
                return ratio(u32::from(width), u32::from(height));
            }
            0xD9 => return None,
            _ => {
                let length = u16::from_be_bytes(bytes[offset + 2..offset + 4].try_into().ok()?) as usize;
                if length < 2 {
                    return None;
                }
                offset += 2 + length;
            }
        }
    }
    None
}
