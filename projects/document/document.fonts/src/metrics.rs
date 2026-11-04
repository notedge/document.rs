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

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::OnceLock,
};

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
    /// Horizontal kern pairs, in font units, from `GPOS` or the legacy table.
    kern: BTreeMap<(u16, u16), i16>,
    /// Whether the face declares a `kern` feature this reader cannot apply.
    kerning_unavailable: bool,
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
        let legacy_kern = face.table_bytes(b"kern").map(parse_kern).unwrap_or_default();
        // A `kern` feature in `GPOS` supersedes the legacy table. When the
        // feature exists but cannot be read as horizontal `PairPos` format 1,
        // the face runs advance-only instead of mixing sources.
        let (kern, kerning_unavailable) = match face.table_bytes(b"GPOS") {
            Some(data) => match read_gpos_kern(data) {
                Ok(Some(pairs)) => (pairs, false),
                Ok(None) => (legacy_kern, false),
                Err(_) => (BTreeMap::new(), true),
            },
            None => (legacy_kern, false),
        };
        Ok(Self { units_per_em, advances, glyphs, kern, kerning_unavailable })
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
    /// The adjustment comes from the `GPOS` `kern` feature when the face
    /// declares one, otherwise from the legacy `kern` table. `None` means the
    /// face has no pair for these characters.
    pub fn kerning_em(&self, left: char, right: char) -> Option<f64> {
        let pair = (self.glyph(left)?, self.glyph(right)?);
        self.kern.get(&pair).map(|value| f64::from(*value) / f64::from(self.units_per_em))
    }

    /// Whether the face declares a `kern` feature this reader cannot apply.
    pub(crate) fn kerning_unavailable(&self) -> bool {
        self.kerning_unavailable
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
    /// Labels of faces whose `kern` feature exists but could not be applied.
    kerning_unavailable: Vec<String>,
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
        let mut kerning_unavailable = Vec::new();
        for (key, label, bytes) in faces {
            match FaceMetrics::parse(bytes) {
                Ok(metrics) => {
                    if metrics.kerning_unavailable() {
                        kerning_unavailable.push(label.clone());
                    }
                    parsed.push((key, metrics));
                }
                Err(_) => unreadable.push(label),
            }
        }
        Self { faces: parsed, unreadable, kerning_unavailable }
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

    /// Labels of faces whose `kern` feature could not be applied.
    ///
    /// These faces run advance-only: their text measures and paints without
    /// kerning, and the caller reports the gap once per layout.
    pub fn kerning_unavailable(&self) -> &[String] {
        &self.kerning_unavailable
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

    fn serif_face(style: BodyFaceStyle) -> &'static FaceMetrics {
        let metrics = BodyMetrics::bundled();
        &metrics
            .faces
            .iter()
            .find(|(key, _)| key.family == BodyFamily::Serif && key.style == style)
            .expect("bundled serif face")
            .1
    }

    #[test]
    fn bundled_latin_faces_carry_gpos_kern_pairs() {
        // Counts and sums come from fontTools reading the shipped `GPOS` tables.
        for (style, count, sum) in [
            (BodyFaceStyle::Regular, 113usize, -16087i64),
            (BodyFaceStyle::Bold, 117, -15901),
            (BodyFaceStyle::Italic, 112, -15323),
        ] {
            let face = serif_face(style);
            assert_eq!(face.kern.len(), count, "{style:?} pair count");
            assert_eq!(face.kern.values().map(|value| i64::from(*value)).sum::<i64>(), sum, "{style:?} pair sum");
            assert!(!face.kerning_unavailable, "{style:?} declares a readable `kern` feature");
        }
        assert!(BodyMetrics::bundled().kerning_unavailable().is_empty());
    }

    #[test]
    fn gpos_kern_matches_the_face_units() {
        let metrics = BodyMetrics::bundled();
        let regular = key_for_run("serif", FontStyle::Normal);
        let bold = key_for_run("serif", FontStyle::Bold);
        let italic = key_for_run("serif", FontStyle::Italic);
        let em = 2048.0;
        assert_eq!(metrics.kerning_em(regular, 'A', 'V'), Some(-264.0 / em));
        assert_eq!(metrics.kerning_em(regular, ' ', 'T'), Some(-37.0 / em));
        assert_eq!(metrics.kerning_em(regular, 'V', 'e'), Some(-227.0 / em));
        assert_eq!(metrics.kerning_em(bold, 'V', 'e'), Some(-188.0 / em));
        assert_eq!(metrics.kerning_em(italic, 'A', 'V'), Some(-102.0 / em));
        assert_eq!(metrics.kerning_em(regular, 'A', 'A'), None);
        // A pair split across two faces has no shared glyph pair.
        assert_eq!(metrics.kerning_em(regular, 'A', '同'), None);
    }

    #[test]
    fn class_based_pair_pos_is_reported_as_unavailable() {
        // A minimal `GPOS` whose only `kern` lookup is a `PairPos` format 2
        // subtable: the reader must reject the whole feature, not guess.
        let mut data = vec![
            0x00, 0x01, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x1E, 0x00, 0x2C, // header
            0x00, 0x01, // script count
        ];
        data.extend_from_slice(b"latn");
        data.extend_from_slice(&[0x00, 0x08]); // script offset
        data.extend_from_slice(&[0x00, 0x04, 0x00, 0x00]); // script: default langsys, no others
        data.extend_from_slice(&[0x00, 0x00, 0xFF, 0xFF, 0x00, 0x01, 0x00, 0x00]); // langsys
        data.extend_from_slice(&[0x00, 0x01]); // feature count
        data.extend_from_slice(b"kern");
        data.extend_from_slice(&[0x00, 0x08]); // feature offset
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x00, 0x00]); // feature: lookup 0
        data.extend_from_slice(&[0x00, 0x01, 0x00, 0x04]); // lookup list
        data.extend_from_slice(&[0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x08]); // lookup 0
        data.extend_from_slice(&[0x00, 0x02]); // pair pos format 2
        assert_eq!(read_gpos_kern(&data), Err(KernReadError::Format2));
    }
}

/// Why a `GPOS` `kern` feature could not be turned into pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KernReadError {
    /// The feature uses class-based `PairPos` format 2, which is out of scope.
    Format2,
    /// A table, offset, or count runs outside the bytes the face carries.
    Malformed,
}

/// Read the horizontal `kern` feature of a `GPOS` table.
///
/// `Ok(None)` means the table has no `kern` feature, so the caller may fall
/// back to the legacy `kern` table. `Err` means the feature exists but cannot
/// be applied: the face must then run advance-only, without mixing the two
/// sources and without guessing adjustments.
fn read_gpos_kern(data: &[u8]) -> Result<Option<BTreeMap<(u16, u16), i16>>, KernReadError> {
    let script_list = usize::from(u16_at(data, 4).ok_or(KernReadError::Malformed)?);
    let feature_list = usize::from(u16_at(data, 6).ok_or(KernReadError::Malformed)?);
    let lookup_list = usize::from(u16_at(data, 8).ok_or(KernReadError::Malformed)?);

    // Every script may attach the same `kern` feature. The scripts are read as
    // a union, because one face can map DFLT and latn to different lookups.
    let mut feature_indices = BTreeSet::new();
    let script_count = u16_at(data, script_list).ok_or(KernReadError::Malformed)?;
    for index in 0..script_count {
        let record = script_list + 2 + usize::from(index) * 6;
        let script = script_list + usize::from(u16_at(data, record + 4).ok_or(KernReadError::Malformed)?);
        let default = u16_at(data, script).ok_or(KernReadError::Malformed)?;
        if default != 0 {
            collect_langsys_features(data, script + usize::from(default), &mut feature_indices)?;
        }
        let langsys_count = u16_at(data, script + 2).ok_or(KernReadError::Malformed)?;
        for langsys in 0..langsys_count {
            let langsys_record = script + 4 + usize::from(langsys) * 6;
            let offset = script + usize::from(u16_at(data, langsys_record + 4).ok_or(KernReadError::Malformed)?);
            collect_langsys_features(data, offset, &mut feature_indices)?;
        }
    }

    let feature_count = u16_at(data, feature_list).ok_or(KernReadError::Malformed)?;
    let mut kern_lookups = BTreeSet::new();
    for index in feature_indices {
        if index >= feature_count {
            return Err(KernReadError::Malformed);
        }
        let record = feature_list + 2 + usize::from(index) * 6;
        let tag = data.get(record..record + 4).ok_or(KernReadError::Malformed)?;
        if tag != b"kern" {
            continue;
        }
        let feature = feature_list + usize::from(u16_at(data, record + 4).ok_or(KernReadError::Malformed)?);
        let count = u16_at(data, feature + 2).ok_or(KernReadError::Malformed)?;
        for lookup in 0..count {
            kern_lookups.insert(u16_at(data, feature + 4 + usize::from(lookup) * 2).ok_or(KernReadError::Malformed)?);
        }
    }
    if kern_lookups.is_empty() {
        return Ok(None);
    }

    let lookup_count = u16_at(data, lookup_list).ok_or(KernReadError::Malformed)?;
    let mut pairs = BTreeMap::new();
    for index in kern_lookups {
        if index >= lookup_count {
            return Err(KernReadError::Malformed);
        }
        let lookup =
            lookup_list + usize::from(u16_at(data, lookup_list + 2 + usize::from(index) * 2).ok_or(KernReadError::Malformed)?);
        read_lookup_pairs(data, lookup, &mut pairs)?;
    }
    Ok(Some(pairs))
}

/// Collect the feature indices one `LangSys` table requests.
fn collect_langsys_features(data: &[u8], offset: usize, out: &mut BTreeSet<u16>) -> Result<(), KernReadError> {
    let required = u16_at(data, offset + 2).ok_or(KernReadError::Malformed)?;
    if required != u16::MAX {
        out.insert(required);
    }
    let count = u16_at(data, offset + 4).ok_or(KernReadError::Malformed)?;
    for index in 0..count {
        out.insert(u16_at(data, offset + 6 + usize::from(index) * 2).ok_or(KernReadError::Malformed)?);
    }
    Ok(())
}

/// Read the pair subtables of one lookup, following extension lookups.
fn read_lookup_pairs(data: &[u8], offset: usize, pairs: &mut BTreeMap<(u16, u16), i16>) -> Result<(), KernReadError> {
    let lookup_type = u16_at(data, offset).ok_or(KernReadError::Malformed)?;
    let count = u16_at(data, offset + 4).ok_or(KernReadError::Malformed)?;
    for index in 0..count {
        let subtable = offset + usize::from(u16_at(data, offset + 6 + usize::from(index) * 2).ok_or(KernReadError::Malformed)?);
        match lookup_type {
            2 => read_pair_pos(data, subtable, pairs)?,
            9 => {
                if u16_at(data, subtable).ok_or(KernReadError::Malformed)? != 1 {
                    return Err(KernReadError::Malformed);
                }
                let inner_type = u16_at(data, subtable + 2).ok_or(KernReadError::Malformed)?;
                let inner = subtable
                    + usize::try_from(u32_at(data, subtable + 4).ok_or(KernReadError::Malformed)?)
                        .map_err(|_| KernReadError::Malformed)?;
                if inner_type == 2 {
                    read_pair_pos(data, inner, pairs)?;
                }
            }
            // A `kern` feature that lists a non-pairing lookup contributes no
            // horizontal pair adjustment.
            _ => {}
        }
    }
    Ok(())
}

/// Read one horizontal `PairPos` subtable.
fn read_pair_pos(data: &[u8], offset: usize, pairs: &mut BTreeMap<(u16, u16), i16>) -> Result<(), KernReadError> {
    match u16_at(data, offset).ok_or(KernReadError::Malformed)? {
        1 => {}
        2 => return Err(KernReadError::Format2),
        _ => return Err(KernReadError::Malformed),
    }
    let coverage_offset = usize::from(u16_at(data, offset + 2).ok_or(KernReadError::Malformed)?);
    let value_format1 = u16_at(data, offset + 4).ok_or(KernReadError::Malformed)?;
    let value_format2 = u16_at(data, offset + 6).ok_or(KernReadError::Malformed)?;
    let pair_set_count = u16_at(data, offset + 8).ok_or(KernReadError::Malformed)?;
    let coverage = coverage_glyphs(data, offset + coverage_offset).ok_or(KernReadError::Malformed)?;
    if coverage.len() != usize::from(pair_set_count) {
        return Err(KernReadError::Malformed);
    }
    let record_len = 2 + value_record_len(value_format1) + value_record_len(value_format2);
    let x_advance1 = x_advance_offset(value_format1);
    let x_advance2 = x_advance_offset(value_format2);
    for (index, left) in coverage.into_iter().enumerate() {
        if index >= usize::from(pair_set_count) {
            break;
        }
        let pair_set = offset + usize::from(u16_at(data, offset + 10 + index * 2).ok_or(KernReadError::Malformed)?);
        let count = u16_at(data, pair_set).ok_or(KernReadError::Malformed)?;
        for index in 0..count {
            let record = pair_set + 2 + usize::from(index) * record_len;
            let right = u16_at(data, record).ok_or(KernReadError::Malformed)?;
            let mut adjustment = 0i32;
            if let Some(at) = x_advance1 {
                adjustment += i32::from(i16_at(data, record + 2 + at).ok_or(KernReadError::Malformed)?);
            }
            if let Some(at) = x_advance2 {
                adjustment +=
                    i32::from(i16_at(data, record + 2 + value_record_len(value_format1) + at).ok_or(KernReadError::Malformed)?);
            }
            if adjustment != 0 {
                pairs.insert((left, right), i16::try_from(adjustment).map_err(|_| KernReadError::Malformed)?);
            }
        }
    }
    Ok(())
}

/// Glyph ids one `Coverage` table lists, in coverage index order.
fn coverage_glyphs(data: &[u8], offset: usize) -> Option<Vec<u16>> {
    match u16_at(data, offset)? {
        1 => {
            let count = u16_at(data, offset + 2)?;
            let mut glyphs = Vec::with_capacity(usize::from(count));
            for index in 0..count {
                glyphs.push(u16_at(data, offset + 4 + usize::from(index) * 2)?);
            }
            Some(glyphs)
        }
        2 => {
            let count = u16_at(data, offset + 2)?;
            let mut glyphs = Vec::new();
            for index in 0..count {
                let start = u16_at(data, offset + 4 + usize::from(index) * 6)?;
                let end = u16_at(data, offset + 6 + usize::from(index) * 6)?;
                if end < start {
                    return None;
                }
                glyphs.extend(start..=end);
            }
            Some(glyphs)
        }
        _ => None,
    }
}

/// Bytes one value record occupies, given its format flags.
fn value_record_len(format: u16) -> usize {
    usize::try_from((format & 0x00FF).count_ones()).unwrap_or(0) * 2
}

/// Offset of the horizontal advance field inside one value record.
fn x_advance_offset(format: u16) -> Option<usize> {
    if format & 0x0004 == 0 {
        return None;
    }
    let mut offset = 0;
    for bit in [0x0001u16, 0x0002] {
        if format & bit != 0 {
            offset += 2;
        }
    }
    Some(offset)
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at.checked_add(2)?).map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at.checked_add(4)?).map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    data.get(at..at.checked_add(2)?).map(|bytes| i16::from_be_bytes([bytes[0], bytes[1]]))
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
