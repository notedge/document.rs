//! Four-corner `MathKernInfo` from the font's `MathGlyphInfo`.
//!
//! Every glyph may publish a kern for each of its four corners, and each corner
//! kern is a step function of a *correction height*: the height of whatever sits
//! next to the corner decides how much the two shapes are pulled together.

use std::collections::{BTreeMap, HashMap};

use acorn_ttf::tables::math::{MathGlyphInfo, MathKernTable};

use document_compose::math::Fixed;

use super::GlyphId;

/// One of the four corners a glyph can kern at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KernCorner {
    /// Above and to the right of the glyph, where a superscript meets it.
    TopRight,
    /// Above and to the left of the glyph.
    TopLeft,
    /// Below and to the right of the glyph.
    BottomRight,
    /// Below and to the left of the glyph, where a subscript meets it.
    BottomLeft,
}

impl KernCorner {
    /// Every corner, in the order the font's `MathKernInfo` lists them.
    pub const ALL: [KernCorner; 4] = [Self::TopRight, Self::TopLeft, Self::BottomRight, Self::BottomLeft];
}

/// One step of a corner's kern function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KernStep {
    /// Correction height this step starts at, in em.
    pub correction_height: Fixed,
    /// Kern applied once the correction height reaches
    /// [`KernStep::correction_height`], in em.
    pub kern: Fixed,
}

/// The kern function of one corner.
///
/// The font stores `CorrectionHeight` values and `KernValue` values where one
/// more kern than height exists: the extra leading value belongs to correction
/// heights below the first step. That leading value is kept in
/// [`CornerKern::kern_below_first`] so nothing the font publishes is dropped,
/// while [`CornerKern::kern`] answers with the steps themselves.
#[derive(Debug, Clone, PartialEq)]
pub struct CornerKern {
    steps: Vec<KernStep>,
    kern_below_first: Fixed,
}

impl CornerKern {
    pub(crate) fn new(steps: Vec<KernStep>, kern_below_first: Fixed) -> Self {
        Self { steps, kern_below_first }
    }

    /// Read one corner `MathKern` subtable.
    ///
    /// Returns `None` when the subtable does not carry the `heights + 1` kern
    /// values its own format requires, which makes the function unreadable
    /// rather than empty.
    pub(crate) fn from_table(table: &MathKernTable, one_em: f64) -> Option<Self> {
        if table.kern_values.len() != table.correction_heights.len() + 1 {
            return None;
        }
        let steps = table
            .correction_heights
            .iter()
            .enumerate()
            .map(|(index, height)| KernStep {
                correction_height: Fixed::from_f64(*height as f64 / one_em),
                kern: Fixed::from_f64(table.kern_values[index + 1] as f64 / one_em),
            })
            .collect();
        Some(Self::new(steps, Fixed::from_f64(table.kern_values[0] as f64 / one_em)))
    }

    /// The steps, in ascending correction height order.
    pub fn steps(&self) -> &[KernStep] {
        &self.steps
    }

    /// The kern the font publishes for correction heights below the first step.
    ///
    /// This is the leading `KernValue` of the raw table. It is not part of the
    /// step lookup, which answers zero when no step is reached.
    pub fn kern_below_first(&self) -> Fixed {
        self.kern_below_first
    }

    /// The kern for `correction_height`.
    ///
    /// Takes the largest step whose correction height does not exceed the
    /// query, and answers zero when the query is below every step. The raw
    /// OpenType step function closes each step at its own height instead of
    /// starting there, so an exact hit on a step's correction height answers
    /// with the following step here.
    pub fn kern(&self, correction_height: Fixed) -> Fixed {
        self.steps.iter().rev().find(|step| step.correction_height <= correction_height).map_or(Fixed::ZERO, |step| step.kern)
    }
}

/// The four corner kerns of one glyph.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlyphKerns {
    top_right: Option<CornerKern>,
    top_left: Option<CornerKern>,
    bottom_right: Option<CornerKern>,
    bottom_left: Option<CornerKern>,
}

impl GlyphKerns {
    /// The kern function of one corner, if the font publishes it.
    pub fn corner(&self, corner: KernCorner) -> Option<&CornerKern> {
        match corner {
            KernCorner::TopRight => self.top_right.as_ref(),
            KernCorner::TopLeft => self.top_left.as_ref(),
            KernCorner::BottomRight => self.bottom_right.as_ref(),
            KernCorner::BottomLeft => self.bottom_left.as_ref(),
        }
    }

    fn slot(&mut self, corner: KernCorner) -> &mut Option<CornerKern> {
        match corner {
            KernCorner::TopRight => &mut self.top_right,
            KernCorner::TopLeft => &mut self.top_left,
            KernCorner::BottomRight => &mut self.bottom_right,
            KernCorner::BottomLeft => &mut self.bottom_left,
        }
    }
}

/// The corner kern data of the whole font.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MathKernInfo {
    glyphs: BTreeMap<GlyphId, GlyphKerns>,
}

impl MathKernInfo {
    /// Read every corner the font's `MathGlyphInfo` publishes.
    pub(crate) fn from_glyph_info(info: &MathGlyphInfo, one_em: f64) -> Self {
        let tables: [(KernCorner, &HashMap<u16, MathKernTable>); 4] = [
            (KernCorner::TopRight, &info.math_kern_top_right),
            (KernCorner::TopLeft, &info.math_kern_top_left),
            (KernCorner::BottomRight, &info.math_kern_bottom_right),
            (KernCorner::BottomLeft, &info.math_kern_bottom_left),
        ];

        let mut glyphs: BTreeMap<GlyphId, GlyphKerns> = BTreeMap::new();
        for (corner, tables) in tables {
            for (&glyph, table) in tables {
                let Some(kern) = CornerKern::from_table(table, one_em)
                else {
                    continue;
                };
                *glyphs.entry(GlyphId::new(glyph)).or_default().slot(corner) = Some(kern);
            }
        }
        Self { glyphs }
    }

    /// Glyphs that publish at least one corner kern, in glyph order.
    pub fn glyphs(&self) -> impl Iterator<Item = GlyphId> + '_ {
        self.glyphs.keys().copied()
    }

    /// Every corner the font publishes for `glyph`.
    pub fn corners(&self, glyph: GlyphId) -> Option<&GlyphKerns> {
        self.glyphs.get(&glyph)
    }

    /// One corner's kern function for `glyph`.
    pub fn corner(&self, glyph: GlyphId, corner: KernCorner) -> Option<&CornerKern> {
        self.glyphs.get(&glyph)?.corner(corner)
    }

    /// The kern the font applies at `correction_height` for one corner.
    ///
    /// Answers zero when the glyph publishes no kern at that corner or when the
    /// correction height is below every step.
    pub fn math_kern(&self, glyph: GlyphId, corner: KernCorner, correction_height: Fixed) -> Fixed {
        self.corner(glyph, corner).map_or(Fixed::ZERO, |kern| kern.kern(correction_height))
    }
}
