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
