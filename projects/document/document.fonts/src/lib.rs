#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("readme.md")]

use std::sync::Once;

use document_text::math::{MathFontFamily, install_math_font};

/// Latin Modern Math (Computer Modern OpenType successor), GUST Font License.
///
/// The license text ships next to the binary in
/// `fonts/LatinModernMath-Regular.GUST-FONT-LICENSE.md`.
pub const LATIN_MODERN_MATH: &[u8] = include_bytes!("../fonts/LatinModernMath-Regular.otf");

/// XITS Math (Times-like, KaTeX lineage), SIL Open Font License.
///
/// The license text ships next to the binary in
/// `fonts/XITSMath-Regular.OFL.md`.
pub const XITS_MATH: &[u8] = include_bytes!("../fonts/XITSMath-Regular.otf");

static INSTALL_DEFAULT: Once = Once::new();

/// Install the bundled math fonts as the process-wide default math faces.
///
/// Registers [`LATIN_MODERN_MATH`] for [`MathFontFamily::LmMath`] and
/// [`XITS_MATH`] for [`MathFontFamily::XitsMath`] through
/// `document_text::math::install_math_font`. The install runs once per
/// process; repeated calls are safe and do nothing.
///
/// LmMath wins the layout preference order, so after this call
/// `document_layout::math::layout_formula` typesets with Latin Modern Math.
pub fn install_default_math_font() {
    INSTALL_DEFAULT.call_once(|| {
        install_math_font(MathFontFamily::LmMath, LATIN_MODERN_MATH);
        install_math_font(MathFontFamily::XitsMath, XITS_MATH);
    });
}

/// Family of one bundled body face.
///
/// Body prose and code blocks need different faces for different reasons: prose
/// wants a reading face whose weight carries structure, while code needs a fixed
/// advance so columns line up. The two never substitute for each other, except
/// that a code block containing Han characters falls back to the body CJK face
/// because no monospaced CJK face is bundled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BodyFamily {
    /// Upright reading face for prose.
    Serif,
    /// Fixed-advance face for code and verbatim text.
    Mono,
}

/// Script coverage of one bundled body face.
///
/// Latin and CJK are separate assets on purpose: a CJK face carries tens of
/// thousands of outlines and would bury a Latin document under megabytes of
/// unused glyphs, while a Latin face has no Han coverage at all. A renderer
/// builds a fallback chain from both, so a mixed line paints from two programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyScript {
    /// Latin, Greek-free Western text: ASCII, Latin-1, Latin Extended-A/B,
    /// general punctuation, and the common mathematical relation signs a body
    /// sentence may quote.
    Latin,
    /// Simplified Chinese body text: GB 2312 hanzi, CJK punctuation, and
    /// full-width forms.
    Cjk,
}

/// Style of one bundled body face.
///
/// Only the styles a body paragraph actually asks for are bundled. There is no
/// bold-italic face and no CJK italic face, because the document IR has no
/// bold-italic style and a slanted Han face is not part of the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BodyFaceStyle {
    /// Upright regular weight.
    Regular,
    /// Bold weight.
    Bold,
    /// Slanted regular weight, Latin only.
    Italic,
}

/// One bundled body face.
#[derive(Debug)]
pub struct BodyFace {
    /// Family this face belongs to.
    pub family: BodyFamily,
    /// Script this face covers.
    pub script: BodyScript,
    /// Style this face carries.
    pub style: BodyFaceStyle,
    /// Complete TrueType file, embedded through a per-document subset.
    pub bytes: &'static [u8],
}

/// Body text faces, in the fallback order a renderer tries them.
///
/// The order groups by family, then Latin before CJK, so a Latin character
/// always comes from the Latin face even when the CJK face happens to carry it
/// too. A requested style is served by the faces of that style first, then by
/// the regular faces, which is why the list repeats script order per style.
///
/// The Latin faces are Tinos, which shares Times New Roman's advance widths and
/// therefore keeps the width model `document_layout` uses to break lines: a face
/// wider than that model paints past the advance layout reserved and overlaps
/// the next run. The CJK faces are Noto Serif SC, subset to GB 2312 plus CJK
/// punctuation and full-width forms. Every licence permits redistribution and
/// ships beside the binary as `fonts/<face>.OFL.md`.
///
/// Sizes are the stored file sizes: the two CJK faces dominate the crate because
/// GB 2312 alone is 6,763 hanzi, and the three Latin faces plus the mono face
/// together stay under 220 KB.
pub const BODY_FACES: &[BodyFace] = &[
    BodyFace { family: BodyFamily::Serif, script: BodyScript::Latin, style: BodyFaceStyle::Regular, bytes: include_bytes!("../fonts/Tinos-Regular.ttf") },
    BodyFace { family: BodyFamily::Serif, script: BodyScript::Latin, style: BodyFaceStyle::Bold, bytes: include_bytes!("../fonts/Tinos-Bold.ttf") },
    BodyFace { family: BodyFamily::Serif, script: BodyScript::Latin, style: BodyFaceStyle::Italic, bytes: include_bytes!("../fonts/Tinos-Italic.ttf") },
    BodyFace { family: BodyFamily::Serif, script: BodyScript::Cjk, style: BodyFaceStyle::Regular, bytes: include_bytes!("../fonts/NotoSerifSC-Regular.ttf") },
    BodyFace { family: BodyFamily::Serif, script: BodyScript::Cjk, style: BodyFaceStyle::Bold, bytes: include_bytes!("../fonts/NotoSerifSC-Bold.ttf") },
    BodyFace { family: BodyFamily::Mono, script: BodyScript::Latin, style: BodyFaceStyle::Regular, bytes: include_bytes!("../fonts/NotoSansMono-Regular.ttf") },
];
