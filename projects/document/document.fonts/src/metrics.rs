//! Character advance widths of the bundled body faces.
//!
//! Line breaking needs the width a face actually paints, not a shape guessed
//! from the character class: a Latin italic face can be several percent wider
//! than a Times-shaped estimate, and the next run then starts inside the
//! previous one. This module reads the advance widths a face stores in its
//! `hmtx` table, in em units, so layout reserves what the renderer paints.
//!
//! The renderer and layout must agree on which face serves a run, so the
//! fallback order lives here too: both call [`fallback_order`] instead of
//! keeping private copies.

use std::{collections::BTreeMap, fmt, sync::OnceLock};

use document_compose::math::FontStyle;

use crate::{BODY_FACES, BodyFace, BodyFaceStyle, BodyFamily};

/// Family and style one text run asks the body faces for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyFaceKey {
    /// Whether the run wants prose or a fixed-advance face.
    pub family: BodyFamily,
    /// Weight and slant the run asks for.
    pub style: BodyFaceStyle,
}

/// Map a run's font style onto the bundled body style.
///
/// Only upright, bold, and italic have bundled faces; every other style
/// (`Monospace`, `Strike`, math-only faces) either names a family instead or
/// paints without a body face, so it measures as regular.
pub fn body_style(style: FontStyle) -> BodyFaceStyle {
    match style {
        FontStyle::Bold => BodyFaceStyle::Bold,
        FontStyle::Italic => BodyFaceStyle::Italic,
        _ => BodyFaceStyle::Regular,
    }
}

/// Family and style a text run asks the body faces for.
///
/// Only monospaced text is treated as code. Every other declared family paints
/// with the prose face, because the bundled set carries one reading face per
/// script rather than one per declared family name.
pub fn key_for_run(family: &str, style: FontStyle) -> BodyFaceKey {
    BodyFaceKey {
        family: if family.to_ascii_lowercase().contains("mono") { BodyFamily::Mono } else { BodyFamily::Serif },
        style: body_style(style),
    }
}

/// Faces tried for one requested family and style, in order.
///
/// A run of the requested family and style comes first, then the regular faces
/// of the same family, then the prose family, so a bold code run or a Han
/// character inside a code block still finds a face.
pub fn fallback_order(key: BodyFaceKey) -> Vec<BodyFaceKey> {
    let candidates = [
        key,
        BodyFaceKey { family: key.family, style: BodyFaceStyle::Regular },
        BodyFaceKey { family: BodyFamily::Serif, style: key.style },
        BodyFaceKey { family: BodyFamily::Serif, style: BodyFaceStyle::Regular },
    ];
    let mut order = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if !order.contains(&candidate) {
            order.push(candidate);
        }
    }
    order
}

/// Advance widths one face file declares.
///
/// Built once per face and then queried per character, so the `cmap` and
/// `hmtx` tables are read eagerly into compact tables instead of reparsing the
/// file for every glyph.
#[derive(Debug, Clone)]
pub struct FaceMetrics {
    /// Font design units per em, from `head`.
    units_per_em: u16,
    /// Advance width per glyph id, in font units, from `hmtx`.
    advances: Vec<u16>,
    /// Covered code points mapped to glyph ids, sorted by code point.
    glyphs: Vec<(u32, u16)>,
    /// Legacy `kern` format 0 pairs, in font units.
    kern: BTreeMap<(u16, u16), i16>,
}

/// Why a face could not supply advance widths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricsError {
    /// The sfnt container or a required table could not be parsed.
    Unreadable(String),
}

impl fmt::Display for MetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(reason) => write!(f, "unreadable font metrics: {reason}"),
        }
    }
}

impl std::error::Error for MetricsError {}

impl FaceMetrics {
    /// Read the advance widths, character map, and kern pairs of one face.
    ///
    /// Any sfnt flavour that carries `cmap` and `hmtx` qualifies, CFF included:
    /// `hmtx` describes horizontal advances independently of the outline
    /// format. Only a container that cannot be parsed, or a face missing
    /// `head` / `hmtx` / `maxp`, is rejected.
    pub fn parse(bytes: &[u8]) -> Result<Self, MetricsError> {
        let mut face = acorn_ttf::FontFace::load(bytes).map_err(|error| MetricsError::Unreadable(error.to_string()))?;
        let units_per_em =
            face.head().map(|head| head.units_per_em).ok_or_else(|| MetricsError::Unreadable("missing `head`".to_string()))?;
        let num_glyphs =
            face.maxp().map(|maxp| maxp.num_glyphs).ok_or_else(|| MetricsError::Unreadable("missing `maxp`".to_string()))?;
        if face.hmtx().is_none() {
            return Err(MetricsError::Unreadable("missing `hmtx`".to_string()));
        }
        let mut advances = Vec::with_capacity(usize::from(num_glyphs));
        for glyph in 0..num_glyphs {
            advances.push(face.glyph_hor_advance(glyph).unwrap_or(0));
        }
        // Body faces cover the Basic Multilingual Plane only; a supplementary
        // code point is reported as uncovered instead of probed per call.
        let mut glyphs = Vec::new();
        for code in 0..=u32::from(u16::MAX) {
            if let Some(ch) = char::from_u32(code) {
                if let Some(glyph) = face.glyph_index(ch) {
                    glyphs.push((code, glyph));
                }
            }
        }
        let kern = face.table_bytes(b"kern").map(parse_kern).unwrap_or_default();
        Ok(Self { units_per_em, advances, glyphs, kern })
    }

    /// Font design units per em.
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// Advance width of one character in em units, or `None` when uncovered.
    pub fn advance_em(&self, ch: char) -> Option<f64> {
        let glyph = self.glyph(ch)?;
        let advance = self.advances.get(usize::from(glyph))?;
        Some(f64::from(*advance) / f64::from(self.units_per_em))
    }

    /// Kerning adjustment between two characters in em units.
    ///
    /// Only the legacy `kern` table is read; `None` means the face has no
    /// legacy pair for these characters, not that the pair is unkerned in
    /// OpenType `GPOS`.
    pub fn kerning_em(&self, left: char, right: char) -> Option<f64> {
        let pair = (self.glyph(left)?, self.glyph(right)?);
        self.kern.get(&pair).map(|value| f64::from(*value) / f64::from(self.units_per_em))
    }

    /// Glyph id of one character.
    fn glyph(&self, ch: char) -> Option<u16> {
        self.glyphs.binary_search_by_key(&(ch as u32), |(code, _)| *code).ok().map(|index| self.glyphs[index].1)
    }
}

/// Advance widths of a set of body faces, queried by requested family and style.
#[derive(Debug)]
pub struct BodyMetrics {
    /// Parsed faces in declaration order, so Latin precedes CJK within one style.
    faces: Vec<(BodyFaceKey, FaceMetrics)>,
    /// Labels of faces whose file could not supply metrics.
    unreadable: Vec<String>,
}

impl BodyMetrics {
    /// Metrics of [`BODY_FACES`], parsed once per process.
    pub fn bundled() -> &'static Self {
        static BUNDLED: OnceLock<BodyMetrics> = OnceLock::new();
        BUNDLED.get_or_init(|| {
            Self::from_faces(
                BODY_FACES
                    .iter()
                    .map(|face| (BodyFaceKey { family: face.family, style: face.style }, face_label(face), face.bytes)),
            )
        })
    }

    /// Metrics of an arbitrary face set, for hosts that paint their own faces.
    ///
    /// Each item carries the family and style the renderer will request, a
    /// label used in diagnostics, and the face file. Items whose file cannot
    /// be parsed are recorded in [`Self::unreadable`] and skipped.
    pub fn from_faces<'a>(faces: impl IntoIterator<Item = (BodyFaceKey, String, &'a [u8])>) -> Self {
        let mut parsed = Vec::new();
        let mut unreadable = Vec::new();
        for (key, label, bytes) in faces {
            match FaceMetrics::parse(bytes) {
                Ok(metrics) => parsed.push((key, metrics)),
                Err(_) => unreadable.push(label),
            }
        }
        Self { faces: parsed, unreadable }
    }

    /// Advance width of one character in em units, through the fallback order.
    ///
    /// `None` means no face in [`fallback_order`] covers the character, so the
    /// caller must fall back to its heuristic and report the gap.
    pub fn advance_em(&self, key: BodyFaceKey, ch: char) -> Option<f64> {
        for candidate in fallback_order(key) {
            for (face_key, metrics) in &self.faces {
                if *face_key == candidate {
                    if let Some(advance) = metrics.advance_em(ch) {
                        return Some(advance);
                    }
                }
            }
        }
        None
    }

    /// Kerning adjustment between two characters in em units.
    ///
    /// The pair is read from the face that serves both characters; a pair split
    /// across two faces has no shared glyph pair, so it returns `None`.
    pub fn kerning_em(&self, key: BodyFaceKey, left: char, right: char) -> Option<f64> {
        let (index, metrics) = self.covering_face(key, left)?;
        let (right_index, _) = self.covering_face(key, right)?;
        if index != right_index {
            return None;
        }
        metrics.kerning_em(left, right)
    }

    /// Labels of faces skipped because their file supplied no metrics.
    pub fn unreadable(&self) -> &[String] {
        &self.unreadable
    }

    /// Index and metrics of the face that serves one character.
    fn covering_face(&self, key: BodyFaceKey, ch: char) -> Option<(usize, &FaceMetrics)> {
        for candidate in fallback_order(key) {
            for (index, (face_key, metrics)) in self.faces.iter().enumerate() {
                if *face_key == candidate && metrics.advance_em(ch).is_some() {
                    return Some((index, metrics));
                }
            }
        }
        None
    }
}

/// Human readable label of a bundled face for diagnostics.
fn face_label(face: &BodyFace) -> String {
    format!("{:?}/{:?}/{:?}", face.family, face.script, face.style)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled_advance(text: &str, family: &str, style: FontStyle) -> f64 {
        let key = key_for_run(family, style);
        text.chars().map(|ch| BodyMetrics::bundled().advance_em(key, ch).expect("bundled face covers the sample")).sum()
    }

    #[test]
    fn italic_latin_uses_the_face_hmtx() {
        // Tinos-Italic: 2048 units per em, the same string sums to 8.056152344 em.
        let width = bundled_advance("italic English words", "serif", FontStyle::Italic);
        assert!((width - 8.056152344).abs() < 1e-6, "italic width was {width}");
    }

    #[test]
    fn regular_latin_uses_the_face_hmtx() {
        // Tinos-Regular: the same string sums to 7.999023438 em.
        let width = bundled_advance("italic English words", "serif", FontStyle::Normal);
        assert!((width - 7.999023438).abs() < 1e-6, "regular width was {width}");
    }

    #[test]
    fn han_advance_is_one_em() {
        let key = key_for_run("serif", FontStyle::Normal);
        assert_eq!(BodyMetrics::bundled().advance_em(key, '同'), Some(1.0));
    }

    #[test]
    fn mono_advance_is_six_tenths_of_an_em() {
        let key = key_for_run("monospace", FontStyle::Normal);
        assert_eq!(BodyMetrics::bundled().advance_em(key, 'i'), Some(0.6));
    }

    #[test]
    fn fallback_order_prefers_the_requested_face_then_prose() {
        let key = BodyFaceKey { family: BodyFamily::Mono, style: BodyFaceStyle::Italic };
        assert_eq!(
            fallback_order(key),
            vec![
                key,
                BodyFaceKey { family: BodyFamily::Mono, style: BodyFaceStyle::Regular },
                BodyFaceKey { family: BodyFamily::Serif, style: BodyFaceStyle::Italic },
                BodyFaceKey { family: BodyFamily::Serif, style: BodyFaceStyle::Regular },
            ]
        );
    }

    #[test]
    fn unreadable_face_is_reported_not_silent() {
        let metrics = BodyMetrics::from_faces([(
            BodyFaceKey { family: BodyFamily::Serif, style: BodyFaceStyle::Regular },
            "broken".to_string(),
            &b"not a font"[..],
        )]);
        assert_eq!(metrics.unreadable(), ["broken".to_string()]);
        assert_eq!(metrics.advance_em(BodyFaceKey { family: BodyFamily::Serif, style: BodyFaceStyle::Regular }, 'a'), None);
    }
}

/// Read legacy `kern` format 0 pairs from a `kern` table.
///
/// The table is a sequence of subtables; format 0 stores horizontal pairs of
/// glyph ids and signed adjustments. Other formats are ignored rather than
/// guessed at.
fn parse_kern(data: &[u8]) -> BTreeMap<(u16, u16), i16> {
    let mut pairs = BTreeMap::new();
    let read_u16 = |at: usize| data.get(at..at + 2).map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]));
    let Some(version) = read_u16(0)
    else {
        return pairs;
    };
    if version != 0 {
        return pairs;
    }
    let Some(count) = read_u16(2)
    else {
        return pairs;
    };
    let mut at = 4;
    for _ in 0..count {
        let Some(length) = read_u16(at + 2)
        else {
            break;
        };
        let Some(coverage) = read_u16(at + 4)
        else {
            break;
        };
        if coverage >> 8 == 0 {
            let Some(pair_count) = read_u16(at + 6)
            else {
                break;
            };
            let mut pair_at = at + 14;
            for _ in 0..pair_count {
                let (Some(left), Some(right), Some(value)) = (read_u16(pair_at), read_u16(pair_at + 2), read_u16(pair_at + 4))
                else {
                    break;
                };
                let adjustment = i16::from_be_bytes([value.to_be_bytes()[0], value.to_be_bytes()[1]]);
                pairs.insert((left, right), adjustment);
                pair_at += 6;
            }
        }
        if length < 6 {
            break;
        }
        at += usize::from(length);
    }
    pairs
}
