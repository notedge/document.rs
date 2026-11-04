//! OpenType MATH font metrics projection.
//!
//! Projects host-installed OpenType MATH font bytes into typed
//! [`MathFontFacts`] and the [`FontMetricsData`] cache the layout engine reads:
//! the MATH constants, glyph advances and ink extents with SVG outline paths,
//! and the MATH Variants constructions delimiters and large operators grow
//! from.
//!
//! The `MATH` table has exactly one reader — `MathFontFacts` — so this module
//! only re-keys its facts into the cache's [`GlyphKey`]-indexed shape. Glyph
//! geometry comes from the same [`FontFace`]: the outline paths of everything
//! the cache can paint, and the ink boxes plus `hmtx` advances it stores per
//! glyph. Values the font does not publish are explicit stand-ins, collected in
//! the `Synthesized fallbacks` section below.
//!
//! Falls back gracefully to synthetic metrics in
//! [`super::font_metrics::builtin_font_metrics`] when the font cannot be
//! parsed or carries no MATH table.

use acorn_ttf::{FontFace, OutlineBuilder};

use document_compose::math::{
    DelimiterComponent, DelimiterConstruction, DelimiterVariant, Fixed, FontMetricsData, FontStyle, GlyphKey, GlyphMetrics,
    alphalect_sources, alphalect_styles, math_alphalect,
};

use super::facts::{MathFontFacts, StretchAxis};

/// Selectable OpenType MATH font families.
///
/// This crate owns font *processing* only. Actual OTF payloads live in the
/// host product (VuTeX) and must be installed through [`install_math_font`]
/// before [`MathFontFamily::bytes`] returns `Some`. A missing font causes
/// [`opentype_font_metrics`] to return `None` and callers fall back to
/// synthetic metrics — [`builtin_default`][Self::builtin_default] picks the
/// first installed family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathFontFamily {
    /// Times-like, same lineage as KaTeX_Main; the modern default face.
    XitsMath,
    /// STIX Two Math — full MATH table upstream; install when the host ships it.
    StixTwoMath,
    /// Computer Modern OpenType successor.
    LmMath,
}

#[derive(Debug, Default)]
struct InstalledMathFonts {
    lm: Option<&'static [u8]>,
    xits: Option<&'static [u8]>,
    stix: Option<&'static [u8]>,
}

fn installed_fonts() -> &'static std::sync::RwLock<InstalledMathFonts> {
    static INSTALLED: std::sync::RwLock<InstalledMathFonts> =
        std::sync::RwLock::new(InstalledMathFonts { lm: None, xits: None, stix: None });
    &INSTALLED
}

/// Install host-owned OpenType MATH font bytes for one family.
///
/// VuTeX (or another product host) calls this once at startup with the
/// `'static` payload it embeds. document.rs never ships the font files.
pub fn install_math_font(family: MathFontFamily, bytes: &'static [u8]) {
    let mut guard = installed_fonts().write().expect("math font registry");
    match family {
        MathFontFamily::LmMath => guard.lm = Some(bytes),
        MathFontFamily::XitsMath => guard.xits = Some(bytes),
        MathFontFamily::StixTwoMath => guard.stix = Some(bytes),
    }
}

impl MathFontFamily {
    /// Stable family label used to key glyphs in [`FontMetricsData`].
    pub const fn label(self) -> &'static str {
        match self {
            Self::XitsMath => "xits-math",
            Self::StixTwoMath => "stix-two-math",
            Self::LmMath => "latin-modern-math",
        }
    }

    /// Raw OTF bytes previously installed for this family, if any.
    pub fn bytes(self) -> Option<&'static [u8]> {
        let guard = installed_fonts().read().ok()?;
        match self {
            Self::LmMath => guard.lm,
            Self::XitsMath => guard.xits,
            Self::StixTwoMath => guard.stix,
        }
    }

    /// Installed families in preference order.
    ///
    /// The metrics cache and the font facts must resolve the default face
    /// through this list so they always describe the same font.
    pub const BUILTIN_PREFERENCE: [Self; 2] = [Self::LmMath, Self::XitsMath];

    /// The first installed family, i.e. the host-provided default face.
    pub fn builtin_default() -> Option<Self> {
        Self::BUILTIN_PREFERENCE.into_iter().find(|family| family.bytes().is_some())
    }
}

// ---------------------------------------------------------------------------
// SVG path OutlineBuilder
// ---------------------------------------------------------------------------

/// An [`OutlineBuilder`] that serializes outline segments to an SVG path
/// string with coordinates normalized to a 0–1 em box.
pub(crate) struct SvgPathBuilder {
    path: String,
    /// Divisor applied to every (x, y) coordinate — typically `units_per_em`.
    scale: f32,
}

impl SvgPathBuilder {
    /// Creates a new builder. Coordinates will be divided by `scale`.
    pub fn new(scale: f32) -> Self {
        SvgPathBuilder { path: String::with_capacity(128), scale }
    }

    /// Returns the accumulated SVG path string.
    pub fn finish(self) -> String {
        self.path
    }

    #[inline]
    fn emit_move(&mut self, x: f32, y: f32) {
        let nx = x / self.scale;
        let ny = y / self.scale;
        self.path.push_str(&format!("M {x:.4},{y:.4} ", x = nx, y = ny));
    }

    #[inline]
    fn emit_line(&mut self, x: f32, y: f32) {
        let nx = x / self.scale;
        let ny = y / self.scale;
        self.path.push_str(&format!("L {x:.4},{y:.4} ", x = nx, y = ny));
    }

    #[inline]
    fn emit_quad(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let nx1 = x1 / self.scale;
        let ny1 = y1 / self.scale;
        let nx = x / self.scale;
        let ny = y / self.scale;
        self.path.push_str(&format!("Q {x1:.4},{y1:.4} {x:.4},{y:.4} ", x1 = nx1, y1 = ny1, x = nx, y = ny));
    }

    #[inline]
    fn emit_curve(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let nx1 = x1 / self.scale;
        let ny1 = y1 / self.scale;
        let nx2 = x2 / self.scale;
        let ny2 = y2 / self.scale;
        let nx = x / self.scale;
        let ny = y / self.scale;
        self.path.push_str(&format!(
            "C {x1:.4},{y1:.4} {x2:.4},{y2:.4} {x:.4},{y:.4} ",
            x1 = nx1,
            y1 = ny1,
            x2 = nx2,
            y2 = ny2,
            x = nx,
            y = ny
        ));
    }
}

impl OutlineBuilder for SvgPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.emit_move(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.emit_line(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.emit_quad(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.emit_curve(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.path.push('Z');
        self.path.push(' ');
    }
}

// ---------------------------------------------------------------------------
// Character scope used for glyph outlines & delimiter variants.
// ---------------------------------------------------------------------------

/// Non-ASCII symbols whose glyph outlines and MATH variant coverage we extract.
///
/// Printable ASCII (`!` through `~`) is always extracted, so relations such as
/// `<` and `>` never fall back to a host font. This list adds the symbols the
/// math parser can emit beyond ASCII.
const TARGET_SYMBOLS: &[char] = &[
    'α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ', 'ν', 'ξ', 'ο', 'π', 'ρ', 'σ', 'τ', 'υ', 'φ', 'χ', 'ψ', 'ω',
    'Γ', 'Δ', 'Θ', 'Λ', 'Ξ', 'Π', 'Σ', 'Φ', 'Ψ', 'Ω', '±', '×', '÷', '≤', '≥', '≠', '≈', '≡', '≅', '∼', '≃', '≪', '≫', '∓',
    '→', '←', '↑', '↓', '↔', '⇒', '⇐', '⇔', '↦', '∞', '√', '∑', '∏', '∫', '∮', '∂', '∇', '∅', '∈', '∉', '∋', '⊂', '⊃', '⊆',
    '⊇', '∪', '∩', '∧', '∨', '¬', '∀', '∃', '⋅', '∘', '∗', '†', '‡', '…', '⋯', '′', '″', '⊥', '∥', '|', '‖', '⟨', '⟩', '−',
    'ℝ', 'ℕ', 'ℤ', 'ℚ', 'ℂ', 'ℓ', 'ℏ', 'ℜ', 'ℑ', 'ℵ',
];

/// Accent characters the math parser stacks over a base.
///
/// `\hat` uses `ˆ`, `\bar` uses `¯`, `\tilde` uses `˜`, `\dot` uses `˙` and
/// `\ddot` uses `¨`, so those five must reach the outline and metric tables.
/// `\vec` already resolves through `→` in [`TARGET_SYMBOLS`] and `\acute`,
/// `\grave` through printable ASCII (ASCII `` ` ``), so only the modifier
/// letters the parser can emit beyond ASCII are listed here. The extra marks
/// (`˘` breve, `ˇ` caron, `˚` ring, U+20D7 combining vector arrow) cover the
/// remaining TeX accent spellings, so an accent never silently degrades to a
/// host-font `<text>` run when the font does ship the mark.
const ACCENT_SYMBOLS: &[char] = &['ˆ', '¯', '˜', '˙', '¨', '˘', 'ˇ', '˚', '\u{20D7}'];

/// The dedicated math-alphabet codepoints the alphalect contract can redirect
/// an authored letter to (math italic, bold, script, double-struck, fraktur,
/// sans-serif, monospace).
///
/// Layout swaps a variable's key onto these codepoints only when the font
/// actually ships them, so they must be inside the glyph scope the projection
/// extracts — otherwise the real italic face would never be found and every
/// variable would fall back to the upright cmap entry of the authored letter.
fn alphalect_targets() -> Vec<char> {
    let mut targets: Vec<char> = alphalect_styles()
        .iter()
        .flat_map(|style| alphalect_sources().iter().map(move |c| math_alphalect(*style, *c)))
        .flatten()
        .collect();
    targets.sort_unstable();
    targets.dedup();
    targets
}

/// Delimiter characters whose MATH Variants constructions we pull from the font.
const DELIMITER_CHARS: &[char] = &['(', ')', '[', ']', '{', '}', '|', '‖', '⟨', '⟩', '/'];

/// Large operators whose display-style size variants we pull from the font.
///
/// MATH keeps them in the same `MathVariants` construction table delimiters
/// stretch from: `\sum`, `\prod`, `\coprod`, `\int`, `\iint` and `\oint` grow
/// from their text-size design to a taller display design rather than being
/// scaled. The list is exactly the characters the parser emits as
/// `SymbolRole::LargeOperator`.
const LARGE_OPERATOR_CHARS: &[char] = &['∑', '∏', '∐', '∫', '∬', '∮'];

/// The radical sign, whose growing designs live in the same `MathVariants`
/// table.
///
/// `√` is not a delimiter — it never seeds the synthetic fallback variants in
/// `font_metrics` — but TeX's `make_radical` picks the sign from exactly this
/// construction machinery, so the projection must carry its prepared designs
/// and assembly pieces just like a fence.
const RADICAL_CHARS: &[char] = &['√'];

// ---------------------------------------------------------------------------
// Synthesized fallbacks
// ---------------------------------------------------------------------------
//
// `FontMetricsData` has no way to say "the font publishes nothing", so a
// handful of numbers below are this crate's own stand-ins. They are all defined
// here so every synthesized value the cache can hold is visible in one place
// instead of scattered through the projection.

/// Narrowest advance the cache reports, in em.
///
/// A glyph the font gives no advance is laid out as a hair of ink rather than
/// as a zero-width box.
const MINIMUM_ADVANCE: f64 = 0.01;

/// Ink box `(height, depth)` assumed for a glyph whose design publishes no
/// bounds, in em.
const SYNTHETIC_INK: (f64, f64) = (0.6, 0.2);

/// Ink box assumed for the `index`-th prepared design of a construction when
/// the font publishes no bounds for it, in em.
///
/// The construction already says the designs grow, so each successive one is
/// assumed a step larger than the previous.
fn synthesized_variant_ink(index: usize) -> (f64, f64) {
    (SYNTHETIC_INK.0 + index as f64 * 0.2, SYNTHETIC_INK.1 + index as f64 * 0.1)
}

/// The MATH metric the cache reports for a glyph the font's `MathGlyphInfo`
/// does not cover.
///
/// The glyph is stored with a zero correction and a zero accent attachment so
/// the cache keeps one shape per glyph. Which glyphs the font actually covers
/// stays visible through `MathFontFacts::glyph_math`.
const UNCOVERED_MATH_METRIC: Fixed = Fixed::ZERO;

// ---------------------------------------------------------------------------
// Font geometry
// ---------------------------------------------------------------------------

/// Pen advance of `glyph` in em, from `hmtx`.
///
/// A glyph with no `hmtx` record has no advance: it answers zero here and the
/// callers clamp it to [`MINIMUM_ADVANCE`].
fn advance_em(face: &mut FontFace, glyph: u16, one_em: f64) -> f64 {
    face.glyph_hor_advance(glyph).unwrap_or(0) as f64 / one_em
}

/// Ink box `(height, depth)` of `glyph` in em, from its font bounds.
///
/// `None` when the font publishes no bounds, so every caller picks its own
/// documented stand-in.
fn ink_from_bounds(face: &mut FontFace, glyph: u16, one_em: f64) -> Option<(f64, f64)> {
    let bounds = face.glyph_bounding_box(glyph)?;
    Some(((bounds.y_max as f64 / one_em).max(0.0), ((-bounds.y_min) as f64 / one_em).max(0.0)))
}

/// The cache's width for a pen advance, clamped to [`MINIMUM_ADVANCE`].
fn width_from_advance(advance_em: f64) -> Fixed {
    Fixed::from_f64(advance_em.max(MINIMUM_ADVANCE))
}

// ---------------------------------------------------------------------------
// Top-level projection
// ---------------------------------------------------------------------------

/// Try to build a [`FontMetricsData`] from the embedded OpenType MATH font.
///
/// Returns `Some(data)` if the requested family has an embedded font that
/// parsed successfully and carries a MATH table. Returns `None` otherwise
/// (family not embedded, parse error, missing MATH table). The caller should
/// then use the synthetic fallback in
/// [`super::font_metrics::builtin_font_metrics`] or try a smaller family.
pub fn opentype_font_metrics(family: MathFontFamily) -> Option<FontMetricsData> {
    let bytes = family.bytes()?;
    let mut face = FontFace::load(bytes).ok()?;
    let facts = MathFontFacts::from_face(&mut face)?;
    let family_label = family.label();
    let one_em = facts.units_per_em() as f64;

    let mut data = FontMetricsData::new(family_label.to_string(), facts.units_per_em());
    data.constants.extend(facts.constants().font_constants());

    extract_glyph_metrics_and_outlines(&mut face, &facts, &mut data, one_em, family_label);
    extract_delimiter_constructions(&mut face, &facts, &mut data, one_em, family_label);

    Some(data)
}

// ---------------------------------------------------------------------------
// Glyph metrics + outline paths
// ---------------------------------------------------------------------------

/// Project the metrics and outline of every character in the product's glyph
/// scope.
///
/// The math metrics come from the facts' glyph coverage: a glyph the font does
/// not cover is stored with [`UNCOVERED_MATH_METRIC`] rather than a guess.
fn extract_glyph_metrics_and_outlines(
    face: &mut FontFace,
    facts: &MathFontFacts,
    data: &mut FontMetricsData,
    one_em: f64,
    family: &str,
) {
    let family = Some(family.to_string());

    // U+0020 is included: text-mode spaces are horizontal advances taken from
    // the font, and skipping it made every `\text{a b}` space fall back to the
    // synthetic 0.5em default instead of the font's 0.332em advance.
    for ch in (' '..='~').chain(TARGET_SYMBOLS.iter().copied()).chain(ACCENT_SYMBOLS.iter().copied()).chain(alphalect_targets())
    {
        let Some(glyph) = facts.glyph_for_char(ch)
        else {
            continue;
        };
        let gid = glyph.get();

        let advance = advance_em(face, gid, one_em);
        let ink = ink_from_bounds(face, gid, one_em).unwrap_or(SYNTHETIC_INK);
        let math = facts.glyph_math(glyph);

        let metrics = GlyphMetrics {
            width: width_from_advance(advance),
            height: Fixed::from_f64(ink.0),
            depth: Fixed::from_f64(ink.1),
            italic_correction: math.and_then(|math| math.italic_correction()).unwrap_or(UNCOVERED_MATH_METRIC),
            top_accent_attachment: math.and_then(|math| math.top_accent_attachment()).unwrap_or(UNCOVERED_MATH_METRIC),
        };

        // The same metrics serve the math face and the upright face, so accent
        // marks resolve without a second lookup.
        let math_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
        let normal_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Normal);
        data.glyphs.insert(math_key.clone(), metrics);
        data.glyphs.insert(normal_key.clone(), metrics);

        if let Some(path) = glyph_outline(face, gid, one_em) {
            data.glyph_paths.insert(math_key, path.clone());
            data.glyph_paths.insert(normal_key, path);
        }
    }
}

// ---------------------------------------------------------------------------
// MATH Variants projection
// ---------------------------------------------------------------------------

/// Project the growing designs of every delimiter and large operator into the
/// cache: their prepared variants, their assembly parts and the metrics and
/// outlines needed to draw either.
///
/// Which designs exist, in which order, with which connector lengths comes
/// from the facts. Only glyph geometry — `hmtx` advances, ink boxes and
/// outlines — is read from the face.
fn extract_delimiter_constructions(
    face: &mut FontFace,
    facts: &MathFontFacts,
    data: &mut FontMetricsData,
    one_em: f64,
    family: &str,
) {
    let family = Some(family.to_string());

    for &ch in DELIMITER_CHARS.iter().chain(LARGE_OPERATOR_CHARS).chain(RADICAL_CHARS) {
        let Some(glyph) = facts.glyph_for_char(ch)
        else {
            continue;
        };

        // Try vertical constructions first (covers nearly all delimiters),
        // then horizontal as a fallback for a few horizontal stretchers.
        //
        // The construction's `stretch_measurement` is measured along the
        // stretching direction. For a vertical construction it is a height, so
        // it must never be mixed into the horizontal pen advance: doing that
        // gives `(` a width as tall as its tall variant and `\left(` then
        // leaves exactly that much blank space before the body. Horizontal
        // constructions advance horizontally, where the measurement *is* the
        // advance.
        let vertical = facts.variants().construction(glyph, StretchAxis::Vertical).map(|c| (StretchAxis::Vertical, c));
        let Some((axis, construction)) = vertical
            .or_else(|| facts.variants().construction(glyph, StretchAxis::Horizontal).map(|c| (StretchAxis::Horizontal, c)))
        else {
            continue;
        };
        let is_vertical_construction = axis == StretchAxis::Vertical;

        let base_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
        let mut construction_variants: Vec<DelimiterVariant> = Vec::new();
        let mut construction_components: Vec<DelimiterComponent> = Vec::new();

        for (index, variant) in construction.variants.iter().enumerate() {
            let variant_glyph_id = variant.glyph.get();

            let variant_key = GlyphKey {
                char: Some(ch),
                glyph_id: Some(variant_glyph_id),
                font_family: family.clone(),
                style: FontStyle::Math,
            };

            // Variant metrics so callers can ask for the ink and the width of a
            // stretched design.
            let variant_advance = if is_vertical_construction {
                advance_em(face, variant_glyph_id, one_em)
            }
            else {
                variant.stretch_measurement.to_f64()
            };
            let ink = ink_from_bounds(face, variant_glyph_id, one_em).unwrap_or_else(|| synthesized_variant_ink(index));
            data.glyphs.insert(
                variant_key.clone(),
                GlyphMetrics {
                    width: width_from_advance(variant_advance),
                    height: Fixed::from_f64(ink.0),
                    depth: Fixed::from_f64(ink.1),
                    italic_correction: UNCOVERED_MATH_METRIC,
                    top_accent_attachment: UNCOVERED_MATH_METRIC,
                },
            );
            // The variant keeps its own outline so stretching never falls back
            // to the base glyph shape.
            if let Some(path) = glyph_outline(face, variant_glyph_id, one_em) {
                data.glyph_paths.insert(variant_key.clone(), path);
            }

            construction_variants.push(DelimiterVariant { glyph: variant_key });
        }

        // Piecewise assembly parts are the only way to build delimiters taller
        // than the largest prepared variant.
        if let Some(assembly) = &construction.assembly {
            for part in &assembly.parts {
                let part_glyph_id = part.glyph.get();
                let part_key = GlyphKey::from_gid(part_glyph_id, family.clone(), FontStyle::Math);
                if let Some(path) = glyph_outline(face, part_glyph_id, one_em) {
                    data.glyph_paths.insert(part_key.clone(), path);
                }
                // A part the font publishes no bounds for carries no ink
                // metrics here, while its connectors below stay exact.
                if let Some(ink) = ink_from_bounds(face, part_glyph_id, one_em) {
                    // Same rule as variants: `full_advance` measures the
                    // stacking direction (kept in the component table), while a
                    // vertical part's horizontal advance is `hmtx`.
                    let part_advance = if is_vertical_construction {
                        advance_em(face, part_glyph_id, one_em)
                    }
                    else {
                        part.full_advance.to_f64()
                    };
                    data.glyphs.insert(
                        part_key.clone(),
                        GlyphMetrics {
                            width: width_from_advance(part_advance),
                            height: Fixed::from_f64(ink.0),
                            depth: Fixed::from_f64(ink.1),
                            italic_correction: UNCOVERED_MATH_METRIC,
                            top_accent_attachment: UNCOVERED_MATH_METRIC,
                        },
                    );
                }
                construction_components.push(DelimiterComponent {
                    glyph: part_key,
                    is_extender: part.is_extender,
                    start_connector: part.start_connector,
                    end_connector: part.end_connector,
                    full_advance: part.full_advance,
                });
            }
        }

        if !construction_variants.is_empty() || !construction_components.is_empty() {
            data.delimiters.insert(
                base_key,
                DelimiterConstruction { variants: construction_variants, components: construction_components },
            );
        }
    }
}

/// Extract an em-normalized SVG outline path for one glyph.
fn glyph_outline(face: &mut FontFace, gid: u16, one_em: f64) -> Option<String> {
    let mut builder = SvgPathBuilder::new(one_em as f32);
    face.outline_glyph(gid, &mut builder)?;
    let path = builder.finish();
    (!path.is_empty()).then_some(path)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
