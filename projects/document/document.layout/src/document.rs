//! Document layout pass: Notedown block tree to positioned page primitives.
//!
//! The pass walks [`DocumentGraph`] blocks in reading order, measures inline
//! content from the embedded body faces' advance widths, wraps lines greedily
//! inside the content width, and paginates at line granularity.

use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap, HashSet},
};

use document_compose::{
    Diagnostic, Severity, SourceSpan,
    math::{FontStyle, FormulaLayoutResult, GlyphKey, MathError, MathResult},
};
use document_fonts::{BodyMetrics, key_for_run};
use document_ir::{AssetId, Block, DocumentGraph, Inline, ListItem, MathFormula, NodeId, RelationEndpoint, TableRow};
use document_text::math::{FontMetricsSystem, builtin_font_metrics};

use crate::{
    diagram::Diagrams,
    hint::{Align, ChromeHeader, LayoutHint},
    math::{LayoutNode, collect_paint, is_tex_formula, layout_formula, layout_formula_for},
    plan::{Graphic, ImagePlaceholder, LayoutItem, LayoutPage, LayoutPlan, Rect, Rule, TextRun},
};

/// Text baseline height as a fraction of the font size.
const ASCENT: f64 = 0.8;
/// Height of a strikethrough rule above the text baseline, as a fraction of
/// the font size.
const STRIKE_OFFSET: f64 = 0.3;
/// Stroke width of a strikethrough rule, as a fraction of the font size.
const STRIKE_THICKNESS: f64 = 0.06;
/// Font family used for body text.
const SERIF: &str = "serif";
/// Font family used for code and preformatted text.
const MONOSPACE: &str = "monospace";
/// Fixed advance width of every monospace glyph, in em units.
const MONOSPACE_ADVANCE: f64 = 0.6;

// ---- TeX display-math skips ----
//
// A `Block::Math` always begins its own paragraph, so TeX applies the long
// `\abovedisplayskip` / `\belowdisplayskip` pair, never the short forms that a
// display sharing a paragraph with the preceding line would pick. The skips are
// not a bare number: TeX's distance from the previous baseline to the formula
// baseline is the skip plus the interline glue the body baselineskip supplies,
// and it bottoms out at the skip alone once the formula is taller than a line.
// This page builder has no interline glue, so the same visible distance is
// reconstructed here from the line box and the formula's own height and depth.
//
// The constants below are the distances measured from a `tectonic` reference at
// a 12 pt body expressed in em: `1.0` is the skip itself, `1.32` is the visible
// gap the formula's own height leaves above, and `2.21` is the skip plus one
// `\baselineskip` below.  The reference page must stay one page per set with
// `\raggedbottom`, otherwise the page filler consumes `\belowdisplayskip`'s
// `minus` component and the measured below gap silently shrinks by about
// `0.21 em`.  Only the natural width and the `minus` shrink are consumed: this
// page builder fills pages top-down and never justifies them to the bottom
// margin, so the `plus` stretch component would be invented vertical padding
// rather than TeX's flush-bottom behaviour.

/// Natural space above a display formula, in em of the body size.
const ABOVE_DISPLAY_SKIP_EM: f64 = 1.0;
/// Smallest visible gap above a display taller than a body line, in em.
const ABOVE_DISPLAY_GAP_MIN_EM: f64 = 1.32;
/// Natural gap from a display baseline down to the next line, in em.
const BELOW_DISPLAY_GAP_EM: f64 = 1.32;
/// Natural distance from a display baseline down to the next text baseline, in em.
///
/// TeX puts `\belowdisplayskip` (12 pt) and then one full `\baselineskip`
/// (14.5 pt) between the display baseline and the next line, so the visible
/// distance is `26.5 pt = 2.208 em` of the 12 pt body, less the display's own
/// depth. `1.54` was only the depth floor of the deep-display cases and left
/// every shallow display about `0.66 em` short.
///
/// Plain `\[...\]` keeps this distance whatever the formula size and whatever
/// the preceding line ends at. Loading `amsmath` shifts the formula down by
/// about `0.21 em` and takes `\belowdisplayshortskip` on some displays, but
/// that is a package policy this pass does not model.
const BELOW_DISPLAY_GAP_MIN_EM: f64 = 2.21;
/// How far a display's space may shrink, in em of the body size.
const DISPLAY_SKIP_SHRINK_EM: f64 = 0.5;

/// Family and face style shared by consecutive inline text atoms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextFace {
    /// Font family name carried into the text run.
    family: &'static str,
    /// Face style carried into the text run.
    style: FontStyle,
}

/// Body text face used by every unstyled run.
const BODY_FACE: TextFace = TextFace { family: SERIF, style: FontStyle::Normal };
/// Face used by inline code spans.
const CODE_FACE: TextFace = TextFace { family: MONOSPACE, style: FontStyle::Normal };

/// Measuring face for a run emitted with an explicit family name.
///
/// Only the two families layout emits reach this helper, so anything that is
/// not monospace measures as body text.
fn face_for_family(family: &str) -> TextFace {
    if family == MONOSPACE { CODE_FACE } else { BODY_FACE }
}

/// Width source for body text.
///
/// Line breaking must reserve what the renderer paints, so every character is
/// measured from the advance width of the embedded face that serves it. The
/// heuristic remains only as a fallback: a face the host did not ship, or a
/// character no face covers, still gets a width instead of collapsing to zero,
/// and the gap is reported once per layout pass.
struct TextMeasure<'a> {
    /// Heuristic widths from `vutex-text`, used where no face covers a glyph.
    heuristic: FontMetricsSystem,
    /// Advance widths of the faces the PDF renderer paints.
    faces: &'a BodyMetrics,
    /// Characters no face covered, so the gap is reported once per pass.
    missing: RefCell<BTreeSet<char>>,
}

impl<'a> TextMeasure<'a> {
    fn new(faces: &'a BodyMetrics) -> Self {
        Self { heuristic: builtin_font_metrics(), faces, missing: RefCell::new(BTreeSet::new()) }
    }

    /// Advance width of one character in em units under `face`.
    ///
    /// The face that serves the run wins; the character class heuristic only
    /// fills in for characters and faces the metrics do not cover.
    fn advance_em(&self, ch: char, face: TextFace) -> f64 {
        if let Some(advance) = self.faces.advance_em(key_for_run(face.family, face.style), ch) {
            return advance;
        }
        self.missing.borrow_mut().insert(ch);
        heuristic_advance_em(&self.heuristic, ch, face)
    }

    /// Kerning adjustment between adjacent characters in points.
    ///
    /// Zero means the face has no pair, the pair crosses two faces, or the
    /// face runs advance-only because its `kern` feature was not applicable.
    fn kern(&self, left: char, right: char, face: TextFace, size: f64) -> f64 {
        self.faces.kerning_em(key_for_run(face.family, face.style), left, right).unwrap_or(0.0) * size
    }

    /// Width of a run of text in points, advances plus adjacent-pair kerning.
    fn measure(&self, text: &str, size: f64, face: TextFace) -> f64 {
        let mut width = 0.0;
        let mut previous = None;
        for ch in text.chars() {
            if let Some(left) = previous {
                width += self.kern(left, ch, face, size);
            }
            width += self.advance_em(ch, face) * size;
            previous = Some(ch);
        }
        width
    }

    /// Degradations this pass had to apply, in diagnostic form.
    ///
    /// A face that could not be parsed and the characters no face covered are
    /// each reported once, because per-character diagnostics would flood the
    /// caller without adding information.
    fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let unreadable = self.faces.unreadable();
        if !unreadable.is_empty() {
            diagnostics.push(Diagnostic {
                code: "vutex.layout.metrics.face_unreadable".to_string(),
                severity: Severity::Warning,
                message: format!(
                    "body faces {} supplied no readable metrics, so their text was measured with the heuristic width model",
                    unreadable.join("`, `")
                ),
                span: None,
            });
        }
        let unavailable = self.faces.kerning_unavailable();
        if !unavailable.is_empty() {
            diagnostics.push(Diagnostic {
                code: "vutex.layout.metrics.kerning_unavailable".to_string(),
                severity: Severity::Warning,
                message: format!(
                    "body faces {} declare a `kern` feature that could not be applied as horizontal `PairPos` format 1, so their text was measured and painted advance-only",
                    unavailable.join("`, `")
                ),
                span: None,
            });
        }
        let missing = self.missing.borrow();
        if !missing.is_empty() {
            let codepoints: Vec<String> = missing.iter().map(|ch| format!("U+{:04X}", *ch as u32)).collect();
            diagnostics.push(Diagnostic {
                code: "vutex.layout.metrics.glyph_missing".to_string(),
                severity: Severity::Warning,
                message: format!(
                    "no body face covers {} character(s) ({}), so the heuristic width model measured them",
                    missing.len(),
                    codepoints.join(", ")
                ),
                span: None,
            });
        }
        diagnostics
    }
}

/// Map an inline style name onto the shared text face.
///
/// Only the emphasis names the document model recognises carry a visible
/// face; any other style string is laid out as body text instead of being
/// guessed at.
fn face_for_style(style: &str) -> TextFace {
    match style.to_ascii_lowercase().as_str() {
        "bold" | "strong" => TextFace { family: SERIF, style: FontStyle::Bold },
        "italic" | "em" => TextFace { family: SERIF, style: FontStyle::Italic },
        "mono" | "code" => TextFace { family: MONOSPACE, style: FontStyle::Monospace },
        "strike" => TextFace { family: SERIF, style: FontStyle::Strike },
        _ => BODY_FACE,
    }
}

/// Resolve the destination of a link target.
///
/// Only authored external URIs become destinations. Internal `Node`,
/// `Document`, `Asset`, and `Unresolved` targets stay without a destination,
/// because layout emits no matching anchor ids and a fabricated URL would point
/// nowhere.
fn link_href(target: &RelationEndpoint) -> Option<&str> {
    match target {
        RelationEndpoint::ExternalUri(uri) if !uri.is_empty() => Some(uri),
        _ => None,
    }
}

/// Resolve the hover text of a link target.
///
/// An authored empty title carries no tooltip, so it resolves to `None` instead
/// of an empty tooltip in the output.
fn link_title(title: Option<&String>) -> Option<&str> {
    title.map(String::as_str).filter(|value| !value.is_empty())
}

/// Whether a code fence language tag names a host-lowerable TikZ picture.
///
/// TikZ pictures still arrive as a code fence tagged `tikz`, so a registered
/// scene replaces that fence. Mermaid arrives as `Block::Diagram` and never
/// reaches this helper.
fn is_tikz_language(language: Option<&str>) -> bool {
    language.is_some_and(|value| value.trim().eq_ignore_ascii_case("tikz"))
}

/// The degradation reported when math carries a language the formula engine
/// cannot lower.
///
/// The source is still painted as text, so the formula range gives the report a
/// precise site instead of an unpositioned warning.
fn unsupported_math_language_diagnostic(formula: &document_ir::MathFormula) -> Diagnostic {
    Diagnostic {
        code: "vutex.layout.math.unsupported_language".to_string(),
        severity: Severity::Warning,
        message: format!(
            "math language `{}` has no lowering in the TeX formula engine, the source was kept as text",
            formula.language.as_deref().unwrap_or_default()
        ),
        span: Some(SourceSpan { start: 0, end: formula.source.len() as u32 }),
    }
}

/// The loss reported when the formula engine cannot typeset a formula.
///
/// A host without a math font gets [`MathError`] from the layout pipeline; the
/// authored source is still painted so the page keeps the formula visible, but
/// nothing on it is typeset math. That is a loss, not a clean conversion, so it
/// carries `Error` severity and a span over the whole formula source instead of
/// disappearing from the diagnostic list.
fn math_not_typeset_diagnostic(formula: &document_ir::MathFormula, error: &MathError) -> Diagnostic {
    Diagnostic {
        code: "vutex.layout.math.not_typeset".to_string(),
        severity: Severity::Error,
        message: format!("math was not typeset ({error}); the source was kept as text `{}`", formula.source),
        span: Some(SourceSpan { start: 0, end: formula.source.len() as u32 }),
    }
}

/// The loss reported when a citation reaches a PDF page.
///
/// A PDF has no citation structure to carry the IR key or its mode. The page
/// keeps the authored display, or the key in brackets when the display is
/// absent, so the reader loses no visible text. The semantic key is gone, which
/// needs a code instead of a clean export.
fn citation_degraded_diagnostic(key: &str) -> Diagnostic {
    Diagnostic {
        code: "export.pdf.citation_degraded".to_string(),
        severity: Severity::Warning,
        message: format!("citation key `{key}` has no PDF structure, only its display text was laid out"),
        span: Some(SourceSpan { start: 0, end: key.len() as u32 }),
    }
}

/// The loss reported when image alternative text reaches a PDF page.
///
/// The PDF backend emits untagged pages: there is no `/StructTreeRoot` to hold
/// an `/Alt` entry and no marked-content sequence to carry `/ActualText`. An
/// image box alone therefore has no channel for the authored description, so
/// the alt text is reported as dropped instead of disappearing silently.
fn image_alt_dropped_diagnostic(source: &str, alt: &str) -> Diagnostic {
    Diagnostic {
        code: "export.pdf.image_alt_dropped".to_string(),
        severity: Severity::Warning,
        message: format!(
            "image `{source}` alternate text `{alt}` has no tagged-PDF structure or marked-content slot, so the page keeps only the image box"
        ),
        span: Some(SourceSpan { start: 0, end: alt.len() as u32 }),
    }
}

/// The loss reported when an intra-document reference reaches a PDF page.
fn reference_target_dropped_diagnostic(target: NodeId) -> Diagnostic {
    Diagnostic {
        code: "export.pdf.reference_target_dropped".to_string(),
        severity: Severity::Warning,
        message: format!(
            "intra-document reference to node {} has no PDF destination map, so only the display text is painted",
            target.0
        ),
        span: None,
    }
}

/// The loss reported when an image title reaches a PDF page.
///
/// A page image slot has no annotation or alternate-text channel, so the title
/// cannot be written anywhere. The image itself stays, and the title is reported
/// instead of disappearing from the document.
fn image_title_dropped_diagnostic(source: &str, title: &str) -> Diagnostic {
    Diagnostic {
        code: "export.pdf.image_title_dropped".to_string(),
        severity: Severity::Warning,
        message: format!("image `{source}` title `{title}` has no PDF annotation or alternate-text slot"),
        span: Some(SourceSpan { start: 0, end: title.len() as u32 }),
    }
}

/// Compute the layout plan for a Notedown document graph.
///
/// The document layout pass walks the Notedown block tree, measures inline
/// content through `vutex-text`, fills pages, and materialises positioned
/// items.  This module owns that pass; the crate root only re-exports it.
pub fn layout(graph: &DocumentGraph) -> LayoutPlan {
    layout_with_hint(graph, &LayoutHint::a4())
}

/// Compute the layout plan with explicit page geometry and body metrics.
pub fn layout_with_hint(graph: &DocumentGraph, hint: &LayoutHint) -> LayoutPlan {
    let diagrams = Diagrams::default();
    layout_with_diagrams(graph, hint, &diagrams)
}

/// Compute the layout plan, placing the diagram scenes the host registered for
/// `diagram` blocks.
///
/// A diagram block without a registered scene falls back to its authored source
/// text, because layout never invents diagram geometry on its own.
pub fn layout_with_diagrams(graph: &DocumentGraph, hint: &LayoutHint, diagrams: &Diagrams) -> LayoutPlan {
    layout_with_diagnostics(graph, hint, diagrams, &BodyMetrics::default()).0
}

/// Like [`layout_with_diagrams`] but also reports the degradation the pass had
/// to apply.
///
/// Today the sources are a math glyph the embedded font supplies no outline for,
/// which is skipped instead of being painted with a host serif face, and body
/// text a face metric could not cover, which keeps the heuristic width.
///
/// The host injects the metrics of the body faces it paints. A host without
/// faces passes [`BodyMetrics::default`], and every width falls back to the
/// heuristic model.
pub fn layout_with_diagnostics(
    graph: &DocumentGraph,
    hint: &LayoutHint,
    diagrams: &Diagrams,
    metrics: &BodyMetrics,
) -> (LayoutPlan, Vec<Diagnostic>) {
    layout_with_metrics(graph, hint, diagrams, metrics)
}

/// Like [`layout_with_diagnostics`] but measures body text with `metrics`.
///
/// Hosts that paint their own faces pass the metrics of exactly those faces, so
/// the reserved width and the painted width come from the same `hmtx` tables.
pub fn layout_with_metrics(
    graph: &DocumentGraph,
    hint: &LayoutHint,
    diagrams: &Diagrams,
    metrics: &BodyMetrics,
) -> (LayoutPlan, Vec<Diagnostic>) {
    let mut builder = PageBuilder::new(*hint, diagrams, metrics, graph);
    builder.render_blocks(graph, &root_ids(graph), 0.0);
    builder.emit_page_chrome(graph);
    let mut diagnostics = builder.diagnostics;
    diagnostics.extend(builder.text.diagnostics());
    (LayoutPlan { pages: builder.pages }, diagnostics)
}

/// Lay one formula out with the MATH family the hint selects.
///
/// Without an explicit family the pass keeps the host-installed default, so
/// existing callers observe exactly the face they installed at startup.
fn lay_formula(
    formula: &MathFormula,
    size: f64,
    hint: &LayoutHint,
) -> MathResult<(FormulaLayoutResult, LayoutNode, Vec<Diagnostic>)> {
    match hint.math_font {
        Some(family) => layout_formula_for(formula, size, family),
        None => layout_formula(formula, size),
    }
}

/// Block ids that no section, quote, list item or table cell claims as a child,
/// in graph order.
///
/// Table cells hold child block ids, and the table's own emission paints those
/// children inside the grid. Visiting them again as root blocks would lay the
/// same text out a second time outside the table, so they stay referenced.
fn root_ids(graph: &DocumentGraph) -> Vec<NodeId> {
    let mut referenced = HashSet::new();
    for node in &graph.blocks {
        match &node.block {
            Block::Section { children, .. } => referenced.extend(children.iter().copied()),
            Block::Quote { children, .. } => referenced.extend(children.iter().copied()),
            Block::List { items, .. } => {
                for item in items {
                    referenced.extend(item.children.iter().copied());
                }
            }
            Block::Table { head, body, foot, .. } => {
                for cell in head.iter().chain(body).chain(foot).flat_map(|row| row.cells.iter()) {
                    referenced.extend(cell.children.iter().copied());
                }
            }
            _ => {}
        }
    }
    graph.blocks.iter().map(|node| node.id).filter(|id| !referenced.contains(id)).collect()
}

/// One indivisible atom inside a composed paragraph line.
#[derive(Debug, Clone)]
enum InlineAtom {
    /// Text break atom: a word, a single space, or one CJK glyph.
    Text {
        /// Atom text.
        text: String,
        /// Face shared with the surrounding styled span.
        face: TextFace,
        /// External destination shared with the surrounding link, if any.
        href: Option<String>,
        /// Hover text shared with the surrounding link, if any.
        title: Option<String>,
    },
    /// Laid-out inline formula, kept whole across line breaks.
    Math(MathAtom),
    /// Forced line ending produced by an authored hard break.
    Break,
}

/// Text buffered until the face, the link destination, or the link title
/// changes.
#[derive(Debug, Clone)]
struct PendingText {
    text: String,
    face: TextFace,
    href: Option<String>,
    title: Option<String>,
    numeric: bool,
}

impl PendingText {
    fn body() -> Self {
        Self { text: String::new(), face: BODY_FACE, href: None, title: None, numeric: false }
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Buffer `text` under `face`, `href`, and `title`, flushing first when any
    /// key changes.
    ///
    /// Splitting on a face, destination, or hover-text change is what turns a
    /// styled span or a link into its own [`LayoutItem`], so emphasis and link
    /// metadata survive into every renderer without swallowing the neighbouring
    /// text.
    fn push(&mut self, text: &str, face: TextFace, href: Option<&str>, title: Option<&str>, atoms: &mut Vec<InlineAtom>) {
        if text.is_empty() {
            return;
        }
        if !self.is_empty() && (self.face != face || self.href.as_deref() != href || self.title.as_deref() != title) {
            self.flush(atoms);
        }
        self.face = face;
        self.href = href.map(str::to_string);
        self.title = title.map(str::to_string);
        self.text.push_str(text);
    }

    /// Move the buffered text into break atoms.
    fn flush(&mut self, atoms: &mut Vec<InlineAtom>) {
        if self.is_empty() {
            self.text.clear();
            return;
        }
        atoms.extend(split_inline_atoms(&self.text, self.face, self.href.as_deref(), self.title.as_deref()));
        self.text.clear();
    }
}

/// How an inline image joins the atom flow of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageMode {
    /// Flush the atoms at the cursor and reserve an image placeholder.
    Placeholder,
    /// Fold the alternative text into the text flow where no image box fits.
    AltText,
}

/// Atom buffer and pending text shared while walking inline content.
#[derive(Debug, Clone)]
struct AtomFlow {
    /// Composed break atoms in reading order.
    atoms: Vec<InlineAtom>,
    /// Text buffered until the face, destination, or hover text changes.
    pending: PendingText,
}

impl AtomFlow {
    /// Start a flow whose unstyled text uses `face`.
    fn new(face: TextFace) -> Self {
        Self { atoms: Vec::new(), pending: PendingText { text: String::new(), face, href: None, title: None, numeric: false } }
    }

    /// Close the buffered text into atoms and return the composed flow.
    fn finish(mut self) -> Vec<InlineAtom> {
        self.pending.flush(&mut self.atoms);
        self.atoms
    }
}

/// One table cell prepared for painting: wrapped lines and their column span.
#[derive(Debug, Clone)]
struct CellLines {
    /// Columns occupied by the cell.
    span: usize,
    /// Width available to the wrapped lines, in points.
    available: f64,
    /// Broken lines of the cell content.
    lines: Vec<Vec<InlineAtom>>,
    /// Whether the cell holds a number and aligns to the right.
    numeric: bool,
}

/// Vertical extent of one composed line.
#[derive(Debug, Clone, Copy)]
struct LineMetrics {
    /// Extra space above the text line box taken by a tall formula.
    raise: f64,
    /// Extent from the raised line top to the lowest formula depth.
    height: f64,
}

/// A formula box carried alongside the paragraph flow.
///
/// The atom holds the single immutable layout result every backend consumes;
/// the flat `width`/`height`/`depth` are its metrics in points for line
/// assembly.
#[derive(Debug, Clone)]
struct MathAtom {
    result: document_compose::math::FormulaLayoutResult,
    width: f64,
    height: f64,
    depth: f64,
}

/// One TeX display skip resolved to points.
///
/// `natural` is the glue width the page places; `shrink` is how far TeX lets
/// that glue contract so a display that lands near the bottom margin stays on
/// its page instead of moving to the next one.
#[derive(Debug, Clone, Copy)]
struct DisplaySkip {
    /// Natural glue width in points.
    natural: f64,
    /// Shrink allowance in points, at most this much shorter than `natural`.
    shrink: f64,
}

/// The pair of skips TeX puts around a display formula.
///
/// A `Block::Math` always starts its own paragraph, so the preceding paragraph
/// line can never be the line TeX's short-display test compares against. TeX
/// applies `\abovedisplayskip` and `\belowdisplayskip` here, not their short
/// forms, which is what `\par` followed by `\[...\]` produces.
///
/// The glue is measured from the previous text baseline, but the cursor sits at
/// the bottom of that line box, already `pad` below the baseline. Subtracting
/// that reserved padding from the distance TeX keeps is what makes the visible
/// gap come out the same instead of inheriting the page builder's line box.
fn display_skips(size: f64, line: f64, height: f64, depth: f64) -> (DisplaySkip, DisplaySkip) {
    let pad = (line - ASCENT * size).max(0.0);
    let above_gap = (ABOVE_DISPLAY_SKIP_EM * size + line - height).max(ABOVE_DISPLAY_GAP_MIN_EM * size);
    let above = (above_gap - pad).max(0.0);
    let below_gap = (BELOW_DISPLAY_GAP_MIN_EM * size - depth).max(BELOW_DISPLAY_GAP_EM * size);
    let below = (below_gap - ASCENT * size).max(0.0);
    let shrink = DISPLAY_SKIP_SHRINK_EM * size;
    (DisplaySkip { natural: above, shrink: shrink.min(above) }, DisplaySkip { natural: below, shrink: shrink.min(below) })
}

/// Page-filling state for one layout pass.
struct PageBuilder<'a> {
    hint: LayoutHint,
    text: TextMeasure<'a>,
    pages: Vec<LayoutPage>,
    /// Scenes registered by the host for `diagram` and `chart` blocks.
    diagrams: &'a Diagrams,
    /// Storage locations the graph bound to its image assets.
    asset_sources: HashMap<AssetId, String>,
    /// Intrinsic width over height for assets whose bytes were materialized.
    asset_aspects: HashMap<AssetId, f64>,
    /// Page index and zero-based heading level of every heading that started a
    /// section, in document order, so chrome can switch per section.
    section_starts: Vec<(usize, usize)>,
    /// Top edge of the next line box, in page coordinates.
    cursor: f64,
    /// Degradations the pass applied, such as a math glyph without an outline.
    diagnostics: Vec<Diagnostic>,
}

impl<'a> PageBuilder<'a> {
    fn new(hint: LayoutHint, diagrams: &'a Diagrams, faces: &'a BodyMetrics, graph: &DocumentGraph) -> Self {
        let asset_sources =
            graph.assets.iter().filter_map(|asset| asset.source.as_ref().map(|source| (asset.id, source.clone()))).collect();
        let asset_aspects = graph
            .assets
            .iter()
            .filter_map(|asset| asset.bytes.as_deref().and_then(crate::image::aspect_ratio).map(|ratio| (asset.id, ratio)))
            .collect();
        Self {
            hint,
            text: TextMeasure::new(faces),
            pages: vec![blank_page(&hint)],
            diagrams,
            asset_sources,
            asset_aspects,
            section_starts: Vec::new(),
            cursor: hint.margin.top,
            diagnostics: Vec::new(),
        }
    }

    fn push(&mut self, item: LayoutItem) {
        if let Some(page) = self.pages.last_mut() {
            page.items.push(item);
        }
    }

    fn page_has_items(&self) -> bool {
        self.pages.last().is_some_and(|page| !page.items.is_empty())
    }

    /// Skip inter-block spacing at the top of a page.
    fn pad(&mut self, amount: f64) {
        if self.page_has_items() {
            self.cursor += amount;
        }
    }

    /// Start a new page when `height` does not fit below the cursor.
    fn ensure_space(&mut self, height: f64) {
        if !self.page_has_items() {
            return;
        }
        if self.cursor + height > self.hint.content_bottom() {
            self.pages.push(blank_page(&self.hint));
            self.cursor = self.hint.margin.top;
        }
    }

    /// Start a new page unconditionally, unless the current page is empty.
    fn force_page_break(&mut self) {
        if self.page_has_items() {
            self.pages.push(blank_page(&self.hint));
            self.cursor = self.hint.margin.top;
        }
    }

    fn left(&self, indent: f64) -> f64 {
        self.hint.margin.left + indent
    }

    fn content_width(&self, indent: f64) -> f64 {
        (self.hint.content_width() - indent).max(24.0)
    }

    fn body_size(&self) -> f64 {
        self.hint.base_font_size
    }

    fn line_box(&self, size: f64) -> f64 {
        size * self.hint.line_height
    }

    fn wrap(&self, text: &str, size: f64, indent: f64) -> Vec<String> {
        wrap_text(&self.text, text, size, self.content_width(indent))
    }

    /// Paint one line at the cursor and advance past its line box.
    fn emit_line(&mut self, text: String, size: f64, family: &str, indent: f64) {
        let line_box = self.line_box(size);
        self.ensure_space(line_box);
        self.emit_baseline(text, size, family, indent);
        self.cursor += line_box;
    }

    /// Paint one line at the cursor without advancing, so a list marker can
    /// share the baseline of the first content line.
    fn emit_baseline(&mut self, text: String, size: f64, family: &str, indent: f64) {
        let x = self.left(indent);
        let y = self.cursor + ASCENT * size;
        let width = self.text.measure(&text, size, face_for_family(family));
        self.push(LayoutItem::TextRun(TextRun {
            x,
            y,
            size,
            width,
            font_family: family.to_string(),
            style: FontStyle::Normal,
            text,
            href: None,
            title: None,
            color: Some(self.hint.style.ink),
            numeric: false,
            artifact: false,
        }));
    }

    fn emit_wrapped(&mut self, text: &str, size: f64, family: &str, indent: f64) {
        for line in self.wrap(text, size, indent) {
            if line.is_empty() {
                continue;
            }
            self.emit_line(line, size, family, indent);
        }
    }

    fn emit_heading(&mut self, level: u8, title: &[Inline], indent: f64) {
        let tokens = self.hint.style;
        let base = self.body_size();
        let index = usize::from(level.clamp(1, 4)) - 1;
        if self.hint.section_breaks[index] {
            self.force_page_break();
        }
        let size = base * tokens.heading_scales[index];
        let line_box = self.line_box(size);
        self.pad(tokens.heading_gap_before[index]);
        // Keep the heading on the same page as its first body line.
        self.ensure_space(line_box * 2.0 + self.line_box(base));
        self.section_starts.push((self.pages.len().saturating_sub(1), index));
        let face = if tokens.heading_bold { TextFace { family: SERIF, style: FontStyle::Bold } } else { BODY_FACE };
        let atoms = self.compose_atoms(title, size, indent, face, ImageMode::AltText);
        self.emit_inline_lines_aligned(atoms, size, indent, tokens.heading_align);
        if level == 1 && tokens.heading_rule > 0.0 {
            self.pad(tokens.heading_gap_after[index] * 0.2);
            let x = self.left(indent);
            let width = if tokens.heading_rule_width > 0.0 { tokens.heading_rule_width } else { self.content_width(indent) };
            let y = self.cursor;
            self.push(LayoutItem::Rule(Rule {
                x1: x,
                y1: y,
                x2: x + width,
                y2: y,
                thickness: tokens.heading_rule,
                color: Some(tokens.heading_rule_color),
            }));
            self.cursor += tokens.heading_rule;
            self.pad(tokens.heading_gap_after[index] * 0.8);
        }
        else {
            self.pad(tokens.heading_gap_after[index]);
        }
    }

    /// Compose a paragraph from text, styled spans, inline math, and image atoms.
    ///
    /// Text keeps the greedy break atoms of [`wrap_text`]; a style change
    /// splits the run so emphasis reaches the renderers. Inline math becomes a
    /// real formula box that flows on the text baseline and is never split.
    /// When the math engine rejects a formula, its raw source joins the text
    /// flow, matching the display-math fallback.
    fn emit_paragraph(&mut self, content: &[Inline], indent: f64) {
        let tokens = self.hint.style;
        let size = self.body_size();
        let atoms = self.compose_atoms(content, size, indent, BODY_FACE, ImageMode::Placeholder);
        let first_indent = tokens.paragraph_indent.min(self.content_width(indent) * 0.5);
        let lines = break_inline_atoms(&self.text, atoms, size, self.content_width(indent), first_indent);
        for (index, line) in lines.iter().enumerate() {
            let shift = if index == 0 { first_indent } else { 0.0 };
            self.emit_inline_line_at(line, size, indent, shift, None);
        }
        self.pad(tokens.paragraph_gap);
    }

    /// Compose inline content into break atoms under `face`.
    ///
    /// Paragraphs and every other inline-bearing block share this composition
    /// path, so emphasis keeps the same semantics wherever it appears.
    fn compose_atoms(
        &mut self,
        content: &[Inline],
        size: f64,
        indent: f64,
        face: TextFace,
        image_mode: ImageMode,
    ) -> Vec<InlineAtom> {
        let mut flow = AtomFlow::new(face);
        self.compose_inline(content, size, indent, &mut flow, face, None, None, image_mode);
        flow.finish()
    }

    /// Walk inline content into paragraph atoms under the enclosing face and
    /// link metadata.
    ///
    /// A link destination and hover text are inherited by nested inline
    /// content, so styled text inside an external link keeps its face, its
    /// destination, and its tooltip.
    fn compose_inline(
        &mut self,
        content: &[Inline],
        size: f64,
        indent: f64,
        flow: &mut AtomFlow,
        face: TextFace,
        href: Option<&str>,
        title: Option<&str>,
        image_mode: ImageMode,
    ) {
        for inline in content {
            match inline {
                Inline::Image { source, alt, title: image_title, asset } => {
                    if let Some(title) = image_title.as_deref().filter(|title| !title.trim().is_empty()) {
                        self.diagnostics.push(image_title_dropped_diagnostic(source, title));
                    }
                    let alt_text: String = alt.iter().map(Inline::plain_text).collect();
                    let aspect = asset.as_ref().and_then(|asset| self.asset_aspects.get(asset)).copied();
                    let source = asset
                        .as_ref()
                        .and_then(|asset| self.asset_sources.get(asset))
                        .cloned()
                        .unwrap_or_else(|| source.clone());
                    match image_mode {
                        ImageMode::Placeholder => {
                            if !alt_text.trim().is_empty() {
                                self.diagnostics.push(image_alt_dropped_diagnostic(&source, &alt_text));
                            }
                            flow.pending.flush(&mut flow.atoms);
                            self.emit_inline_lines(std::mem::take(&mut flow.atoms), size, indent);
                            self.emit_image_placeholder(&alt_text, &source, aspect, indent);
                        }
                        ImageMode::AltText => flow.pending.push(&alt_text, face, href, title, &mut flow.atoms),
                    }
                }
                Inline::InlineMath { formula } => {
                    flow.pending.flush(&mut flow.atoms);
                    if is_tex_formula(formula) {
                        match lay_formula(formula, size, &self.hint) {
                            Ok((result, _root, math_diagnostics)) => {
                                self.diagnostics.extend(math_diagnostics);
                                let metrics = &result.metrics;
                                let (width, height, depth) =
                                    (metrics.width.to_f64(), metrics.height.to_f64(), metrics.depth.to_f64());
                                flow.atoms.push(InlineAtom::Math(MathAtom { result, width, height, depth }));
                            }
                            Err(error) => {
                                // No math font means no typeset formula: the
                                // source still reaches the page as text, but the
                                // degradation must be reported instead of
                                // silently passing as a clean layout.
                                self.diagnostics.push(math_not_typeset_diagnostic(formula, &error));
                                flow.atoms.extend(split_inline_atoms(&formula.source, face, href, title));
                            }
                        }
                    }
                    else {
                        self.diagnostics.push(unsupported_math_language_diagnostic(formula));
                        flow.atoms.extend(split_inline_atoms(&formula.source, face, href, title));
                    }
                }
                Inline::Styled { style, children } => {
                    self.compose_inline(children, size, indent, flow, face_for_style(style), href, title, image_mode);
                }
                Inline::InlineCode { text } => flow.pending.push(text, CODE_FACE, href, title, &mut flow.atoms),
                // Attributes carry no printable face, but their wrapped content
                // keeps every authored style and destination.
                Inline::Attributed { children, .. } => {
                    self.compose_inline(children, size, indent, flow, face, href, title, image_mode);
                }
                Inline::Link { target, display, title: link_hover } => {
                    // A nested external link wins. A nested internal link keeps
                    // the enclosing destination so its text stays in the anchor.
                    let inner_href = link_href(target).or(href);
                    let inner_title = link_title(link_hover.as_ref()).or(title);
                    self.compose_inline(display, size, indent, flow, face, inner_href, inner_title, image_mode);
                }
                Inline::Note { display: Some(display), .. } => {
                    self.compose_inline(display, size, indent, flow, face, href, title, image_mode);
                }
                // A citation key has no PDF structure to carry it and the IR
                // keeps no citation mode. The display text stays on the page,
                // the key stays visible when there is no display, and both
                // cases report the dropped semantic instead of exporting clean.
                Inline::Citation { key, display } => {
                    self.diagnostics.push(citation_degraded_diagnostic(key));
                    flow.pending.flush(&mut flow.atoms);
                    match display {
                        Some(display) => self.compose_inline(display, size, indent, flow, face, href, title, image_mode),
                        None => flow.atoms.extend(split_inline_atoms(&format!("[{key}]"), face, href, title)),
                    }
                }
                // Internal targets have no PDF destination map yet: the page
                // keeps the display text, and the dropped target is named so
                // the report does not read as clean coverage.
                Inline::Reference { display, target } => {
                    self.diagnostics.push(reference_target_dropped_diagnostic(*target));
                    flow.pending.push(display, face, href, title, &mut flow.atoms);
                }
                // A footnote reference with no authored display is still a
                // semantic marker, not the `[^label]` source literal. Paint the
                // label as a superscript so the reader sees a reference and the
                // definition body below carries the text.
                Inline::Note { label, display: None } => {
                    flow.pending.flush(&mut flow.atoms);
                    flow.atoms.extend(split_inline_atoms(&superscript_label(label), face, href, title));
                }
                other => flow.pending.push(&other.plain_text(), face, href, title, &mut flow.atoms),
            }
        }
    }

    /// Break inline atoms into lines and paint each composed line.
    fn emit_inline_lines(&mut self, atoms: Vec<InlineAtom>, size: f64, indent: f64) {
        let max_width = self.content_width(indent);
        for line in break_inline_atoms(&self.text, atoms, size, max_width, 0.0) {
            self.emit_inline_line(&line, size, indent);
        }
    }

    /// Break and paint inline atoms with the theme's heading alignment.
    fn emit_inline_lines_aligned(&mut self, atoms: Vec<InlineAtom>, size: f64, indent: f64, align: Align) {
        let max_width = self.content_width(indent);
        for line in break_inline_atoms(&self.text, atoms, size, max_width, 0.0) {
            let line_width = self.line_width(&line, size);
            let shift = match align {
                Align::Left => 0.0,
                Align::Center => ((max_width - line_width) / 2.0).max(0.0),
                Align::Right => (max_width - line_width).max(0.0),
            };
            self.emit_inline_line_at(&line, size, indent, shift, None);
        }
    }

    /// Natural width of one composed line, in points.
    fn line_width(&self, line: &[InlineAtom], size: f64) -> f64 {
        let mut width = 0.0;
        for atom in line {
            match atom {
                InlineAtom::Text { text, face, .. } => width += self.text.measure(text, size, *face),
                InlineAtom::Math(math) => width += math.width,
                InlineAtom::Break => {}
            }
        }
        width
    }

    /// Paint one composed line, sizing its box by the tallest formula it holds.
    fn emit_inline_line(&mut self, line: &[InlineAtom], size: f64, indent: f64) {
        self.emit_inline_line_at(line, size, indent, 0.0, None);
    }

    /// Paint one composed line shifted inside its column, with an optional ink
    /// override for quote text.
    fn emit_inline_line_at(&mut self, line: &[InlineAtom], size: f64, indent: f64, shift: f64, color: Option<&'static str>) {
        let line_metrics = self.inline_line_metrics(line, size);
        self.ensure_space(line_metrics.raise + line_metrics.height);
        let top = self.cursor + line_metrics.raise;
        let baseline = top + ASCENT * size;
        self.paint_line(line, size, self.left(indent) + shift, baseline, self.content_width(indent), color, false);
        self.cursor = top + line_metrics.height;
    }

    /// Vertical extent of one composed line, grown by any tall formula.
    ///
    /// A formula taller than the text ascent pushes the line top down so the
    /// formula and the text share one baseline without crossing the previous
    /// line; the line box then covers the formula's full vertical extent, depth
    /// included.
    fn inline_line_metrics(&self, line: &[InlineAtom], size: f64) -> LineMetrics {
        let mut formula_height = 0.0f64;
        let mut formula_depth = 0.0f64;
        for atom in line {
            if let InlineAtom::Math(math) = atom {
                formula_height = formula_height.max(math.height);
                formula_depth = formula_depth.max(math.depth);
            }
        }
        let raise = (formula_height - ASCENT * size).max(0.0);
        LineMetrics { raise, height: self.line_box(size).max(raise + ASCENT * size + formula_depth) }
    }

    /// Paint the atoms of one composed line from `x` on one baseline, and return
    /// the right edge the line reached.
    ///
    /// Consecutive atoms sharing a face, a destination, and a hover text stay
    /// one run, and a change of any key starts a new run so emphasis and link
    /// metadata survive into every renderer.
    fn paint_line(
        &mut self,
        line: &[InlineAtom],
        size: f64,
        x: f64,
        baseline: f64,
        max_width: f64,
        color: Option<&'static str>,
        numeric: bool,
    ) -> f64 {
        let mut x = x;
        let mut run = PendingText::body();
        run.numeric = numeric;
        let mut run_x = x;
        for atom in line {
            match atom {
                InlineAtom::Text { text, face, href, title } => {
                    if run.is_empty() {
                        run_x = x;
                        run.face = *face;
                        run.href = href.clone();
                        run.title = title.clone();
                        run.numeric = numeric;
                    }
                    else if run.face != *face
                        || run.href.as_deref() != href.as_deref()
                        || run.title.as_deref() != title.as_deref()
                    {
                        self.paint_inline_run(&mut run, run_x, baseline, size, color);
                        run_x = x;
                        run.face = *face;
                        run.href = href.clone();
                        run.title = title.clone();
                        run.numeric = numeric;
                    }
                    // A pair that crosses an atom boundary is still one painted
                    // pair when both atoms stay in the same run.
                    let cross = run
                        .text
                        .chars()
                        .last()
                        .zip(text.chars().next())
                        .map_or(0.0, |(left, right)| self.text.kern(left, right, *face, size));
                    run.text.push_str(text);
                    x += self.text.measure(text, size, *face) + cross;
                }
                InlineAtom::Math(math) => {
                    self.paint_inline_run(&mut run, run_x, baseline, size, color);
                    self.emit_inline_math(math, x, baseline, max_width);
                    x += math.width;
                }
                InlineAtom::Break => {}
            }
        }
        self.paint_inline_run(&mut run, run_x, baseline, size, color);
        x
    }

    /// Paint the buffered paragraph text as one run and clear the buffer.
    ///
    /// A strikethrough run also emits its decoration as a rule right after the
    /// text run, so paint order keeps the rule above the glyphs. The rule spans
    /// exactly the measured run width through the same face-aware metrics.
    fn paint_inline_run(&mut self, run: &mut PendingText, x: f64, baseline: f64, size: f64, color: Option<&'static str>) {
        if run.is_empty() {
            return;
        }
        let width = self.text.measure(&run.text, size, run.face);
        let struck = run.face.style == FontStyle::Strike;
        let tokens = self.hint.style;
        let ink = if run.href.is_some() { Some(tokens.accent) } else { color.or(Some(tokens.ink)) };
        self.push(LayoutItem::TextRun(TextRun {
            x,
            y: baseline,
            size,
            width,
            font_family: run.face.family.to_string(),
            style: run.face.style,
            text: std::mem::take(&mut run.text),
            href: run.href.clone(),
            title: run.title.clone(),
            color: ink,
            numeric: run.numeric,
            artifact: false,
        }));
        if struck {
            let y = baseline - STRIKE_OFFSET * size;
            let rule = Rule { x1: x, y1: y, x2: x + width, y2: y, thickness: STRIKE_THICKNESS * size, color: Some(tokens.ink) };
            self.push(LayoutItem::Rule(rule));
        }
    }

    /// Paint a formula at the inline baseline, clamped to the content width.
    fn emit_inline_math(&mut self, math: &MathAtom, x: f64, baseline: f64, max_width: f64) {
        let scale = if math.width > max_width && math.width > 0.0 { (max_width / math.width).max(0.0) } else { 1.0 };
        let items = collect_paint(&math.result, x, baseline, &mut self.diagnostics);
        for item in items {
            self.push(clamp_inline_item(item, x, baseline, scale));
        }
    }

    /// Paint one footnote definition body under the last body line.
    ///
    /// The definition payload is authored text, so it is wrapped at the
    /// footnote size instead of being dropped as an opaque block. The theme's
    /// gap separates the body from the text above, and a positive
    /// `footnote_rule` paints a separator under the last body line of the page.
    fn emit_footnote_definition(&mut self, label: &str, body: &str, indent: f64) {
        let tokens = self.hint.style;
        let size = self.body_size() * 0.82;
        self.pad(tokens.footnote_gap);
        if tokens.footnote_rule > 0.0 {
            self.ensure_space(tokens.footnote_rule);
            let x = self.left(indent);
            let y = self.cursor;
            self.push(LayoutItem::Rule(Rule {
                x1: x,
                y1: y,
                x2: x + self.content_width(indent),
                y2: y,
                thickness: tokens.footnote_rule,
                color: Some(tokens.rule_faint),
            }));
            self.cursor += tokens.footnote_rule;
        }
        self.ensure_space(size * 1.4);
        let text = format!("{} {}", superscript_label(label), body);
        self.emit_wrapped(&text, size, SERIF, indent);
    }

    /// Reserve a box for one authored image that renderers resolve.
    ///
    /// The box spans the content column unless the theme caps it through
    /// `image_max_width`. When the import layer materialized the asset bytes,
    /// the header gives the intrinsic aspect ratio and the box keeps it, also
    /// shrunk when a tall image would not fit the page. Layout reads headers
    /// only, never pixels, so a missing payload falls back to the documented
    /// 0.625 ratio.
    fn emit_image_placeholder(&mut self, alt: &str, source: &str, aspect: Option<f64>, indent: f64) {
        let tokens = self.hint.style;
        let size = self.body_size();
        let cap = if tokens.image_max_width > 0.0 { tokens.image_max_width } else { f64::INFINITY };
        let available = self.content_width(indent).min(cap);
        let page_content = (self.hint.page_height - self.hint.margin.top - self.hint.margin.bottom).max(0.0);
        let (width, height) = match aspect {
            Some(ratio) if ratio > 0.0 => {
                let mut width = available;
                let mut height = width / ratio;
                if page_content > 0.0 && height > page_content {
                    height = page_content;
                    width = height * ratio;
                }
                (width, height)
            }
            _ => (available, available * 0.625),
        };
        self.pad(size * 0.3);
        self.ensure_space(height);
        let x = self.left(indent);
        let source = (!source.trim().is_empty()).then(|| source.to_string());
        self.push(LayoutItem::ImagePlaceholder(ImagePlaceholder {
            x,
            y: self.cursor,
            width,
            height,
            alt: alt.to_string(),
            source,
        }));
        self.cursor += height;
        self.pad(size * 0.4);
    }

    /// Place the scene the host registered for a `diagram` block.
    ///
    /// The scene keeps its aspect ratio and is scaled down to the content
    /// column when it is wider than the column. A block without a scene, or a
    /// scene without a positive canvas, is dropped instead of being guessed at.
    /// Returns whether a graphic was placed.
    fn emit_diagram(&mut self, id: NodeId, indent: f64) -> bool {
        let Some(diagram) = self.diagrams.get(id)
        else {
            return false;
        };
        let intrinsic = diagram.scene.size;
        if intrinsic.width <= 0.0 || intrinsic.height <= 0.0 {
            return false;
        }
        let size = self.body_size();
        let scale = (self.content_width(indent) / intrinsic.width).min(1.0);
        let width = intrinsic.width * scale;
        let height = intrinsic.height * scale;
        self.pad(size * 0.4);
        self.ensure_space(height);
        let x = self.left(indent);
        self.push(LayoutItem::Graphic(Graphic {
            x,
            y: self.cursor,
            width,
            height,
            scene: diagram.scene.clone(),
            alt: diagram.alt.clone(),
        }));
        self.cursor += height;
        self.pad(size * 0.6);
        true
    }

    fn emit_list(&mut self, ordered: bool, items: &[ListItem], indent: f64, graph: &DocumentGraph) {
        let tokens = self.hint.style;
        let size = self.body_size();
        let line_box = self.line_box(size);
        for (index, item) in items.iter().enumerate() {
            let marker = match (ordered, item.number) {
                (true, Some(number)) => format!("{number}."),
                (true, None) => format!("{}.", index + 1),
                (false, _) => tokens.list_marker.to_string(),
            };
            let marker_width = self.text.measure(&marker, size, BODY_FACE);
            let content_indent = indent + tokens.list_indent.max(marker_width + tokens.list_marker_gap);
            let atoms = self.compose_atoms(&item.content, size, content_indent, BODY_FACE, ImageMode::AltText);
            self.pad(tokens.list_gap_before);
            self.ensure_space(line_box * 2.0);
            // The marker keeps the baseline of the first content line.
            self.emit_baseline(marker, size, SERIF, indent);
            self.emit_inline_lines(atoms, size, content_indent);
            self.render_blocks(graph, &item.children, content_indent);
            self.pad(tokens.list_item_gap);
        }
    }

    fn emit_quote(&mut self, graph: &DocumentGraph, children: &[NodeId], indent: f64) {
        let tokens = self.hint.style;
        let size = self.body_size();
        let quote_indent = indent + tokens.quote_indent;
        self.pad(size * 0.4);
        // Keep the opening line and the rule on one page whenever possible.
        self.ensure_space(self.line_box(size) * 2.0);
        let top = self.cursor;
        self.render_blocks(graph, children, quote_indent);
        let bottom = self.cursor;
        if tokens.quote_rule > 0.0 && bottom > top {
            let x = self.left(indent) + 2.0;
            self.push(LayoutItem::Rule(Rule {
                x1: x,
                y1: top,
                x2: x,
                y2: bottom,
                thickness: tokens.quote_rule,
                color: Some(tokens.quote_rule_color),
            }));
        }
        self.pad(size * 0.5);
    }

    fn emit_code(&mut self, content: &str, indent: f64) {
        let tokens = self.hint.style;
        let size = tokens.code_size;
        let line_box = size * tokens.code_line_height;
        let padding = tokens.code_padding;
        let lines: Vec<&str> = content.lines().collect();
        let height = lines.len().max(1) as f64 * line_box + padding * 2.0;
        self.pad(tokens.code_margin);
        self.ensure_space(height.min(self.hint.content_height()));
        let top = self.cursor;
        let x = self.left(indent);
        let width = self.content_width(indent);
        self.push(LayoutItem::Rect(Rect {
            x,
            y: top,
            width,
            height,
            filled: true,
            fill: Some(tokens.code_fill),
            stroke: (tokens.code_border > 0.0).then_some(tokens.code_border_color),
            stroke_width: tokens.code_border,
        }));
        for line in &lines {
            // Code lines stay verbatim: no wrapping, overflow is visible rather than hidden.
            let y = self.cursor + padding + ASCENT * size;
            let width = self.text.measure(line, size, CODE_FACE);
            self.push(LayoutItem::TextRun(TextRun {
                x: x + padding,
                y,
                size,
                width,
                font_family: MONOSPACE.to_string(),
                style: FontStyle::Normal,
                text: (*line).to_string(),
                href: None,
                title: None,
                color: Some(tokens.ink),
                numeric: false,
                artifact: false,
            }));
            self.cursor += line_box;
        }
        self.cursor = top + height;
        self.pad(tokens.code_margin);
    }

    fn emit_table(
        &mut self,
        graph: &DocumentGraph,
        caption: Option<&str>,
        head: &[TableRow],
        body: &[TableRow],
        foot: &[TableRow],
        indent: f64,
    ) {
        let rows: Vec<(&TableRow, bool)> = head
            .iter()
            .map(|row| (row, true))
            .chain(body.iter().map(|row| (row, false)))
            .chain(foot.iter().map(|row| (row, false)))
            .collect();
        let tokens = self.hint.style;
        let size = self.body_size();
        let line_box = self.line_box(size);
        let x = self.left(indent);
        let width = self.content_width(indent);
        let padding = tokens.table_pad_h;
        let columns = rows
            .iter()
            .map(|(row, _)| row.cells.iter().map(|cell| cell.colspan.max(1) as usize).sum::<usize>())
            .max()
            .unwrap_or(1)
            .max(1);
        let column_width = width / columns as f64;
        if let Some(caption) = caption {
            self.pad(size * 0.3);
            self.emit_wrapped(caption, size * 0.9, SERIF, indent);
        }
        self.pad(size * 0.4);
        for (row_index, (row, is_header)) in rows.iter().enumerate() {
            let header_face =
                if *is_header && tokens.heading_bold { TextFace { family: SERIF, style: FontStyle::Bold } } else { BODY_FACE };
            let cells: Vec<CellLines> = row
                .cells
                .iter()
                .map(|cell| {
                    let span = cell.colspan.max(1) as usize;
                    let available = (column_width * span as f64 - padding * 2.0).max(24.0);
                    let content = cell_inlines(graph, cell);
                    let atoms = self.compose_atoms(&content, size, indent, header_face, ImageMode::AltText);
                    let numeric = is_numeric_content(&content);
                    CellLines { span, available, lines: break_inline_atoms(&self.text, atoms, size, available, 0.0), numeric }
                })
                .collect();
            let max_lines = cells.iter().map(|cell| cell.lines.len()).max().unwrap_or(1).max(1);
            let height = max_lines as f64 * line_box + tokens.table_pad_v * 2.0;
            self.ensure_space(height);
            let top = self.cursor;
            if *is_header && let Some(tint) = tokens.table_header_tint {
                self.push(LayoutItem::Rect(Rect {
                    x,
                    y: top,
                    width,
                    height,
                    filled: true,
                    fill: Some(tint),
                    stroke: None,
                    stroke_width: 0.0,
                }));
            }
            let header_thickness = if *is_header { tokens.table_header_rule } else { tokens.table_row_rule };
            if header_thickness > 0.0 {
                let color = if *is_header { tokens.table_rule_strong_color } else { tokens.table_rule_color };
                self.push(LayoutItem::Rule(Rule {
                    x1: x,
                    y1: top,
                    x2: x + width,
                    y2: top,
                    thickness: header_thickness,
                    color: Some(color),
                }));
            }
            let mut column = 0usize;
            for cell in &cells {
                let cell_x = x + column_width * column as f64 + padding;
                let mut line_top = top + tokens.table_pad_v;
                for line in &cell.lines {
                    let shift = if cell.numeric { (cell.available - self.line_width(line, size)).max(0.0) } else { 0.0 };
                    self.paint_line(line, size, cell_x + shift, line_top + ASCENT * size, cell.available, None, cell.numeric);
                    line_top += line_box;
                }
                column += cell.span;
            }
            self.cursor = top + height;
            if row_index + 1 == rows.len() && tokens.table_final_rule > 0.0 {
                self.push(LayoutItem::Rule(Rule {
                    x1: x,
                    y1: self.cursor,
                    x2: x + width,
                    y2: self.cursor,
                    thickness: tokens.table_final_rule,
                    color: Some(tokens.table_rule_strong_color),
                }));
            }
        }
        self.pad(size * 0.8);
    }

    /// Lay out display math and paint its positioned primitives.
    ///
    /// The formula is centred inside the content column like TeX display math
    /// and follows the document body size. Its vertical space comes from TeX's
    /// display skips: `\abovedisplayskip` above and `\belowdisplayskip` below.
    /// When the math engine cannot parse or lay out
    /// the source, the raw TeX is rendered as text with an `Error` diagnostic
    /// (`vutex.layout.math.not_typeset`): the formula stays visible, but the
    /// degradation is never reported as a clean layout.
    fn emit_math(&mut self, formula: &document_ir::MathFormula, indent: f64) {
        let size = self.body_size();
        if !is_tex_formula(formula) {
            self.diagnostics.push(unsupported_math_language_diagnostic(formula));
            self.emit_math_text_fallback(&formula.source, indent);
            return;
        }
        match lay_formula(formula, size, &self.hint) {
            Ok((result, _root, math_diagnostics)) => {
                self.diagnostics.extend(math_diagnostics);
                let metrics = &result.metrics;
                let (width, height, depth) = (metrics.width.to_f64(), metrics.height.to_f64(), metrics.depth.to_f64());
                let left = self.left(indent);
                let offset = ((self.content_width(indent) - width) / 2.0).max(0.0);
                let (above, below) = display_skips(size, self.line_box(size), height, depth);
                let (above_space, below_space) = self.fit_display(height + depth, above, below);
                self.ensure_space(above_space + height + depth + below_space);
                self.pad(above_space);
                let baseline = self.cursor + height;
                let items = collect_paint(&result, left + offset, baseline, &mut self.diagnostics);
                for item in items {
                    self.push(item);
                }
                self.cursor = baseline + depth;
                self.pad(below_space);
            }
            Err(error) => {
                self.diagnostics.push(math_not_typeset_diagnostic(formula, &error));
                self.emit_math_text_fallback(&formula.source, indent);
            }
        }
    }

    /// Resolve the display skips against the space left on the current page.
    ///
    /// TeX decides page breaks on the glue's natural width but lets the glue
    /// shrink by its `minus` component before the display moves to another page.
    /// The stretch component is never used here: pages are filled top-down, so
    /// stretching would fabricate vertical padding rather than fill a justified
    /// page. [`PageBuilder::ensure_space`] still moves the display when even the
    /// fully shrunk glue does not fit.
    fn fit_display(&self, ink: f64, above: DisplaySkip, below: DisplaySkip) -> (f64, f64) {
        if !self.page_has_items() {
            return (above.natural, below.natural);
        }
        let available = self.hint.content_bottom() - self.cursor - ink;
        let natural = above.natural + below.natural;
        if available >= natural {
            return (above.natural, below.natural);
        }
        let capacity = above.shrink + below.shrink;
        if capacity <= 0.0 || available < natural - capacity {
            return (above.natural, below.natural);
        }
        let ratio = ((natural - available) / capacity).min(1.0).max(0.0);
        (above.natural - above.shrink * ratio, below.natural - below.shrink * ratio)
    }

    /// Plain-text rendering used when the math engine rejects the source.
    fn emit_math_text_fallback(&mut self, content: &str, indent: f64) {
        let size = self.body_size();
        self.pad(size * 0.4);
        self.emit_wrapped(content, size, SERIF, indent + 12.0);
        self.pad(size * 0.5);
    }

    /// Resolve a chrome header source into the text one page paints.
    ///
    /// `Title` prefers the document metadata title and falls back to the first
    /// level-1 section title, so a template that switches headers per section
    /// never has to invent the document title itself.
    fn chrome_header_text(&self, source: ChromeHeader, graph: &DocumentGraph) -> Option<String> {
        match source {
            ChromeHeader::None => None,
            ChromeHeader::Literal(text) => Some(text.to_string()),
            ChromeHeader::Title => {
                graph.metadata.title.as_deref().map(str::trim).filter(|title| !title.is_empty()).map(str::to_string).or_else(
                    || {
                        graph.blocks.iter().find_map(|node| match &node.block {
                            Block::Section { level: 1, title, .. } => {
                                let text = title.iter().map(Inline::plain_text).collect::<String>();
                                (!text.trim().is_empty()).then_some(text)
                            }
                            _ => None,
                        })
                    },
                )
            }
        }
    }

    /// Add the running header and folio to every page after the first.
    ///
    /// Chrome lives inside the page margins, so it never moves body text. A
    /// theme that turns chrome off, such as the legacy traditional preset, gets
    /// no extra runs and keeps its old page count. When a template declares
    /// per-level section chrome, the entry of the deepest heading that started
    /// on or before a page replaces the top-level header and folio for that page.
    fn emit_page_chrome(&mut self, graph: &DocumentGraph) {
        let tokens = self.hint.style;
        if !tokens.chrome {
            return;
        }
        let page_width = self.hint.page_width;
        let page_height = self.hint.page_height;
        let left = self.hint.margin.left;
        let right = page_width - self.hint.margin.right;
        let base_header = self.chrome_header_text(self.hint.chrome.header, graph);
        let base_folio = self.hint.chrome.folio;
        let start = self.hint.chrome.start_page.saturating_sub(1);
        let mut section: Option<usize> = None;
        let mut next_section = 0usize;
        for index in start..self.pages.len() {
            while next_section < self.section_starts.len() && self.section_starts[next_section].0 <= index {
                section = Some(self.section_starts[next_section].1);
                next_section += 1;
            }
            let override_ = section.and_then(|level| self.hint.chrome.sections[level]);
            let header = match override_.and_then(|entry| entry.header) {
                Some(source) => self.chrome_header_text(source, graph),
                None => base_header.clone(),
            };
            let folio = override_.and_then(|entry| entry.folio).unwrap_or(base_folio);
            if let Some(title) = header.as_deref() {
                let width = self.text.measure(title, tokens.header_size, BODY_FACE);
                let x = match tokens.header_align {
                    Align::Left => left,
                    Align::Center => ((left + right - width) / 2.0).max(left),
                    Align::Right => (right - width).max(left),
                };
                self.pages[index].items.push(LayoutItem::TextRun(TextRun {
                    x,
                    y: tokens.chrome_top,
                    size: tokens.header_size,
                    width,
                    font_family: SERIF.to_string(),
                    style: FontStyle::Normal,
                    text: title.to_string(),
                    href: None,
                    title: None,
                    color: Some(tokens.header_color),
                    numeric: false,
                    artifact: true,
                }));
            }
            if folio {
                let folio = format!("{}", index + 1);
                let width = self.text.measure(&folio, tokens.folio_size, BODY_FACE);
                let x = ((left + right - width) / 2.0).max(left);
                self.pages[index].items.push(LayoutItem::TextRun(TextRun {
                    x,
                    y: page_height - tokens.chrome_bottom,
                    size: tokens.folio_size,
                    width,
                    font_family: SERIF.to_string(),
                    style: FontStyle::Normal,
                    text: folio,
                    href: None,
                    title: None,
                    color: Some(tokens.folio_color),
                    numeric: true,
                    artifact: true,
                }));
            }
        }
    }

    fn emit_thematic_break(&mut self, indent: f64) {
        let tokens = self.hint.style;
        let size = self.body_size();
        self.pad(size * 0.6);
        self.ensure_space(1.0);
        let x = self.left(indent);
        let width = self.content_width(indent);
        let y = self.cursor;
        self.push(LayoutItem::Rule(Rule {
            x1: x,
            y1: y,
            x2: x + width,
            y2: y,
            thickness: 0.6,
            color: Some(tokens.rule_strong),
        }));
        self.cursor += size * 0.6;
    }

    fn render_blocks(&mut self, graph: &DocumentGraph, ids: &[NodeId], indent: f64) {
        for id in ids {
            let Some(node) = graph.block(*id)
            else {
                continue;
            };
            match &node.block {
                Block::Section { level, title, children } => {
                    self.emit_heading(*level, title, indent);
                    self.render_blocks(graph, children, indent);
                }
                Block::Paragraph { content } => {
                    self.emit_paragraph(content, indent);
                }
                Block::List { ordered, items } => {
                    self.emit_list(*ordered, items, indent, graph);
                    self.pad(self.body_size() * 0.4);
                }
                Block::Quote { children } => {
                    self.emit_quote(graph, children, indent);
                }
                Block::Code { content, language } => {
                    // A TikZ fence whose scene the host registered is a
                    // diagram, not preformatted code. A failed or missing
                    // lowering keeps the authored fence text visible.
                    if !is_tikz_language(language.as_deref()) || !self.emit_diagram(*id, indent) {
                        self.emit_code(content, indent);
                    }
                }
                // A formal diagram block is either placed from the scene the
                // host registered or keeps its authored source visible, so a
                // missing lowering never leaves a blank page region.
                Block::Diagram { source, .. } => {
                    if !self.emit_diagram(*id, indent) {
                        self.emit_code(source, indent);
                    }
                }
                // Charts share the host-scene channel with diagrams. Without a
                // registered scene the authored spec stays visible as code.
                Block::Chart { source, .. } => {
                    if !self.emit_diagram(*id, indent) {
                        self.emit_code(source, indent);
                    }
                }
                Block::Math { formula } => self.emit_math(formula, indent),
                Block::Table { caption, head, body, foot } => {
                    self.emit_table(graph, caption.as_deref(), head, body, foot, indent)
                }
                Block::Opaque { kind, .. } if kind == "thematic_break" => self.emit_thematic_break(indent),
                // An imported TeX picture environment carries no geometry
                // itself; the host-supplied scene is the only source of its
                // placement. A picture without a scene is dropped, which
                // matches the previous treatment of the opaque environment.
                Block::Opaque { kind, .. } if kind == "tex.environment.tikzpicture" => {
                    self.emit_diagram(*id, indent);
                }
                // Footnote definitions carry their body as text, not as an
                // opaque payload with no layout meaning. Lay the body out so a
                // reader sees the footnote instead of its authored source
                // literal.
                Block::Opaque { kind, payload_hint, .. } if kind == "footnote_definition" => {
                    if let Some((label, body)) = split_footnote_definition(payload_hint) {
                        self.emit_footnote_definition(&label, &body, indent);
                    }
                    else {
                        self.diagnostics.push(Diagnostic {
                            code: "vutex.layout.footnote_definition_unparsed".to_string(),
                            severity: Severity::Warning,
                            message: "footnote definition payload has no label and body separator".to_string(),
                            span: None,
                        });
                    }
                }
                // Opaque payloads with no layout meaning are left out of the
                // page, but never silently: the kind is reported as a
                // diagnostic so the caller sees which construct was not laid
                // out.
                Block::Opaque { kind, .. } => self.diagnostics.push(Diagnostic {
                    code: "vutex.layout.opaque_skipped".to_string(),
                    severity: Severity::Warning,
                    message: format!("opaque block `{kind}` has no layout meaning and was left out of the page"),
                    span: None,
                }),
                // Raw HTML is emitted as-is at render time; layout drops it.
                Block::RawHtml { .. } => {}
            }
        }
    }
}

/// Map a footnote label to Unicode superscript characters when every character
/// has a superscript form, so a flow target paints a reference instead of the
/// authored `[^label]` literal.
fn superscript_label(label: &str) -> String {
    label
        .trim()
        .chars()
        .map(|ch| match ch {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            '+' => '⁺',
            '-' => '⁻',
            '=' => '⁼',
            '(' => '⁽',
            ')' => '⁾',
            other => other,
        })
        .collect()
}

/// Split an authored `[^label]: body` footnote definition into its label and
/// body. Continuation lines are folded into one body string.
fn split_footnote_definition(payload: &str) -> Option<(String, String)> {
    let payload = payload.trim();
    let rest = payload.strip_prefix("[^")?;
    let (label, body) = rest.split_once("]:")?;
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    let body = body.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" ");
    if body.is_empty() {
        return None;
    }
    Some((label.to_string(), body))
}

/// Split text into paragraph atoms: words, spaces, CJK glyphs, and hard breaks.
/// Authored newlines stay forced line endings instead of collapsing into spaces.
fn split_inline_atoms(text: &str, face: TextFace, href: Option<&str>, title: Option<&str>) -> Vec<InlineAtom> {
    let mut atoms = Vec::new();
    let mut word = String::new();
    let mut in_space = false;
    let push_text = |value: String, atoms: &mut Vec<InlineAtom>| {
        atoms.push(InlineAtom::Text { text: value, face, href: href.map(str::to_string), title: title.map(str::to_string) });
    };
    for ch in text.chars() {
        if ch == '\n' {
            if !word.is_empty() {
                push_text(std::mem::take(&mut word), &mut atoms);
            }
            atoms.push(InlineAtom::Break);
            in_space = false;
            continue;
        }
        if ch.is_whitespace() {
            if !word.is_empty() {
                push_text(std::mem::take(&mut word), &mut atoms);
            }
            if !in_space {
                push_text(" ".to_string(), &mut atoms);
                in_space = true;
            }
            continue;
        }
        in_space = false;
        if (ch as u32) >= 0x2E80 {
            if !word.is_empty() {
                push_text(std::mem::take(&mut word), &mut atoms);
            }
            push_text(ch.to_string(), &mut atoms);
            continue;
        }
        word.push(ch);
    }
    if !word.is_empty() {
        push_text(word, &mut atoms);
    }
    atoms
}

/// Greedy first-fit line breaking over paragraph atoms.
///
/// Formula atoms are never split; one that does not fit starts the next line,
/// and one wider than the whole column occupies a line of its own.
fn break_inline_atoms(
    widths: &TextMeasure<'_>,
    atoms: Vec<InlineAtom>,
    size: f64,
    max_width: f64,
    first_indent: f64,
) -> Vec<Vec<InlineAtom>> {
    let mut lines: Vec<Vec<InlineAtom>> = Vec::new();
    let mut current: Vec<InlineAtom> = Vec::new();
    let mut width = 0.0f64;
    for atom in atoms {
        let line_width = (max_width - if lines.is_empty() { first_indent } else { 0.0 }).max(24.0);
        match atom {
            InlineAtom::Break => finish_inline_line(&mut lines, &mut current, &mut width),
            InlineAtom::Text { text, face, href, title } => {
                let atom_width = widths.measure(&text, size, face);
                // Adjacent atoms that stay in one painted run are kerned as one
                // pair, so breaking must reserve that pair as well.
                let tail = match current.last() {
                    Some(InlineAtom::Text { text: last, face: last_face, href: last_href, title: last_title })
                        if *last_face == face
                            && last_href.as_deref() == href.as_deref()
                            && last_title.as_deref() == title.as_deref() =>
                    {
                        last.chars().last()
                    }
                    _ => None,
                };
                let cross = tail.zip(text.chars().next()).map_or(0.0, |(left, right)| widths.kern(left, right, face, size));
                if text.trim().is_empty() {
                    // A space never starts a line and never widens past the column.
                    if !current.is_empty() && width + atom_width + cross <= line_width {
                        current.push(InlineAtom::Text { text, face, href, title });
                        width += atom_width + cross;
                    }
                    continue;
                }
                let mut cross = cross;
                if !current.is_empty() && width + atom_width + cross > line_width {
                    finish_inline_line(&mut lines, &mut current, &mut width);
                    // Nothing is painted across the line break, so no pair.
                    cross = 0.0;
                }
                if atom_width > line_width && current.is_empty() {
                    // Break an oversized word or URL character by character.
                    let mut chunk = String::new();
                    let mut chunk_width = 0.0;
                    let mut previous = None;
                    for ch in text.chars() {
                        let advance =
                            widths.advance_em(ch, face) * size + previous.map_or(0.0, |left| widths.kern(left, ch, face, size));
                        if !chunk.is_empty() && chunk_width + advance > line_width {
                            lines.push(vec![InlineAtom::Text {
                                text: chunk.trim_end().to_string(),
                                face,
                                href: href.clone(),
                                title: title.clone(),
                            }]);
                            chunk.clear();
                            // The pair across the break is not painted.
                            chunk.push(ch);
                            chunk_width = widths.advance_em(ch, face) * size;
                            previous = Some(ch);
                            continue;
                        }
                        chunk.push(ch);
                        chunk_width += advance;
                        previous = Some(ch);
                    }
                    current.push(InlineAtom::Text { text: chunk, face, href, title });
                    width = chunk_width;
                    continue;
                }
                current.push(InlineAtom::Text { text, face, href, title });
                width += atom_width + cross;
            }
            InlineAtom::Math(math) => {
                if !current.is_empty() && width + math.width > line_width {
                    finish_inline_line(&mut lines, &mut current, &mut width);
                }
                let oversized = math.width > line_width;
                width += math.width;
                current.push(InlineAtom::Math(math));
                if oversized {
                    // A formula wider than the column never shares its line.
                    finish_inline_line(&mut lines, &mut current, &mut width);
                }
            }
        }
    }
    finish_inline_line(&mut lines, &mut current, &mut width);
    lines
}

/// Flatten a table cell's child blocks into the inline run the layout composes.
///
/// Every paragraph child keeps its inline semantics, and children are separated
/// by a space so two authored paragraphs never glue their words together.
fn cell_inlines(graph: &DocumentGraph, cell: &document_ir::TableCell) -> Vec<Inline> {
    let mut content = Vec::new();
    for child_id in &cell.children {
        if !content.is_empty() {
            content.push(Inline::Text { text: " ".into() });
        }
        append_block_inlines(graph, *child_id, &mut content);
    }
    content
}

/// Project nested cell blocks into the inline flow of one cell.
///
/// A PDF table cell is a single text block, so a list or a nested table cannot
/// own its own grid. Their authored values still belong in the cell, so lists
/// keep their markers and nested tables keep their row and cell boundaries as
/// separators instead of disappearing from the page.
fn append_block_inlines(graph: &DocumentGraph, node_id: NodeId, out: &mut Vec<Inline>) {
    let Some(node) = graph.block(node_id)
    else {
        return;
    };
    match &node.block {
        Block::Paragraph { content } | Block::Section { title: content, .. } => out.extend(content.iter().cloned()),
        Block::Quote { children } => {
            for child in children {
                if !out.is_empty() {
                    out.push(Inline::Text { text: " ".into() });
                }
                append_block_inlines(graph, *child, out);
            }
        }
        Block::List { ordered, items } => {
            for (index, item) in items.iter().enumerate() {
                if !out.is_empty() {
                    out.push(Inline::Text { text: " ".into() });
                }
                let number = item.number.unwrap_or(index as u32 + 1);
                let marker = if *ordered { format!("{number}. ") } else { "• ".to_string() };
                out.push(Inline::Text { text: marker });
                out.extend(item.content.iter().cloned());
                for child in &item.children {
                    out.push(Inline::Text { text: " ".into() });
                    append_block_inlines(graph, *child, out);
                }
            }
        }
        Block::Table { caption, head, body, foot } => {
            if let Some(caption) = caption {
                out.push(Inline::Text { text: caption.clone() });
                out.push(Inline::Text { text: " ".into() });
            }
            for (row_index, row) in head.iter().chain(body.iter()).chain(foot.iter()).enumerate() {
                if row_index > 0 {
                    out.push(Inline::Text { text: "  ".into() });
                }
                for (cell_index, cell) in row.cells.iter().enumerate() {
                    if cell_index > 0 {
                        out.push(Inline::Text { text: " | ".into() });
                    }
                    for child_id in &cell.children {
                        if !out.is_empty() {
                            out.push(Inline::Text { text: " ".into() });
                        }
                        append_block_inlines(graph, *child_id, out);
                    }
                }
            }
        }
        Block::Code { content, .. } => out.push(Inline::Text { text: content.clone() }),
        _ => {}
    }
}

/// Whether a table cell holds a number and should align to the right.
///
/// A numeric cell has at least one digit and contains only digits, digit
/// separators, signs, and currency or percent marks, so a sentence that merely
/// mentions a number keeps its reading-position alignment.
fn is_numeric_content(content: &[Inline]) -> bool {
    let text: String = content.iter().map(Inline::plain_text).collect();
    let text = text.trim();
    text.chars().any(|ch| ch.is_ascii_digit())
        && text.chars().all(|ch| {
            ch.is_ascii_digit() || matches!(ch, '.' | ',' | '-' | '+' | '%' | '/' | ':' | ' ' | '(' | ')' | '$' | '¥' | '€')
        })
}

/// Close one line: drop trailing spaces and keep it only when it has content.
fn finish_inline_line(lines: &mut Vec<Vec<InlineAtom>>, current: &mut Vec<InlineAtom>, width: &mut f64) {
    while matches!(current.last(), Some(InlineAtom::Text { text, .. }) if text.trim().is_empty()) {
        current.pop();
    }
    if !current.is_empty() {
        lines.push(std::mem::take(current));
    }
    else {
        current.clear();
    }
    *width = 0.0;
}

/// Scale an inline formula primitive about its origin and baseline.
///
/// Used only when a formula is wider than the content column: scaling keeps
/// every glyph and rule inside the column instead of dropping the formula.
fn clamp_inline_item(item: LayoutItem, origin_x: f64, baseline: f64, scale: f64) -> LayoutItem {
    if (scale - 1.0).abs() < f64::EPSILON {
        return item;
    }
    match item {
        LayoutItem::TextRun(run) => LayoutItem::TextRun(TextRun {
            x: origin_x + (run.x - origin_x) * scale,
            y: baseline + (run.y - baseline) * scale,
            size: run.size * scale,
            width: run.width * scale,
            font_family: run.font_family,
            style: run.style,
            text: run.text,
            href: run.href,
            title: run.title,
            color: run.color,
            numeric: run.numeric,
            artifact: run.artifact,
        }),
        LayoutItem::Rule(rule) => LayoutItem::Rule(Rule {
            x1: origin_x + (rule.x1 - origin_x) * scale,
            y1: baseline + (rule.y1 - baseline) * scale,
            x2: origin_x + (rule.x2 - origin_x) * scale,
            y2: baseline + (rule.y2 - baseline) * scale,
            thickness: rule.thickness * scale,
            color: rule.color,
        }),
        other => other,
    }
}

fn blank_page(hint: &LayoutHint) -> LayoutPage {
    LayoutPage { width: hint.page_width, height: hint.page_height, items: Vec::new() }
}

/// Advance width of one character in em units under `face`.
///
/// Fallback only: it applies when the embedded face metrics publish no width,
/// either because the face file is unavailable or the character is uncovered.
/// `vutex-text` exposes a uniform 0.5 em advance for known glyphs, so the
/// character class refines that base advance for book-text line breaking.
/// Monospace text gets a fixed advance, so positioned runs line up with the
/// monospace font the renderers actually select.
fn heuristic_advance_em(metrics: &FontMetricsSystem, ch: char, face: TextFace) -> f64 {
    if face.family == MONOSPACE {
        return MONOSPACE_ADVANCE;
    }
    if ch.is_whitespace() {
        return 0.25;
    }
    let key = GlyphKey::from_char(ch, Some("default".to_string()), FontStyle::Normal);
    let base = metrics.get_metrics(&key).map(|glyph| glyph.width.to_f64()).unwrap_or(0.5);
    match ch {
        'i' | 'l' | 'j' | 't' | 'f' | 'r' | 'I' | '.' | ',' | '\'' | '!' | '|' | ':' | ';' | '(' | ')' | '[' | ']' => {
            base * 0.62
        }
        'W' | 'M' | 'm' | 'w' | '@' => base * 1.7,
        'A'..='Z' => base * 1.25,
        '0'..='9' => base * 1.1,
        ch if (ch as u32) >= 0x2E80 => 1.0,
        _ => base,
    }
}

/// Split text into breakable atoms: words, single spaces, and single CJK glyphs.
fn split_atoms(text: &str) -> Vec<String> {
    let mut atoms = Vec::new();
    let mut word = String::new();
    let mut in_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !word.is_empty() {
                atoms.push(std::mem::take(&mut word));
            }
            if !in_space {
                atoms.push(" ".to_string());
                in_space = true;
            }
            continue;
        }
        in_space = false;
        if (ch as u32) >= 0x2E80 {
            if !word.is_empty() {
                atoms.push(std::mem::take(&mut word));
            }
            atoms.push(ch.to_string());
            continue;
        }
        word.push(ch);
    }
    if !word.is_empty() {
        atoms.push(word);
    }
    atoms
}

/// Greedy first-fit line breaking with word and CJK break opportunities.
fn wrap_text(widths: &TextMeasure<'_>, text: &str, size: f64, max_width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for raw in text.split('\n') {
        let mut line = String::new();
        let mut width = 0.0;
        for atom in split_atoms(raw) {
            let atom_width = widths.measure(&atom, size, BODY_FACE);
            // The pair between the previous line end and this atom is painted,
            // so it counts toward the column even though the atoms are separate
            // strings.
            let cross = line
                .chars()
                .last()
                .zip(atom.chars().next())
                .map_or(0.0, |(left, right)| widths.kern(left, right, BODY_FACE, size));
            if atom.trim().is_empty() {
                if line.is_empty() || width + atom_width + cross > max_width {
                    continue;
                }
                line.push(' ');
                width += atom_width + cross;
                continue;
            }
            let mut cross = cross;
            if !line.is_empty() && width + atom_width + cross > max_width {
                lines.push(line.trim_end().to_string());
                line = String::new();
                width = 0.0;
                cross = 0.0;
            }
            if atom_width > max_width && line.is_empty() {
                // Break an oversized word or URL character by character.
                let mut chunk = String::new();
                let mut chunk_width = 0.0;
                let mut previous = None;
                for ch in atom.chars() {
                    let advance = widths.advance_em(ch, BODY_FACE) * size
                        + previous.map_or(0.0, |left| widths.kern(left, ch, BODY_FACE, size));
                    if !chunk.is_empty() && chunk_width + advance > max_width {
                        lines.push(chunk.trim_end().to_string());
                        chunk = String::new();
                        // The pair across the break is not painted.
                        chunk.push(ch);
                        chunk_width = widths.advance_em(ch, BODY_FACE) * size;
                        previous = Some(ch);
                        continue;
                    }
                    chunk.push(ch);
                    chunk_width += advance;
                    previous = Some(ch);
                }
                line = chunk;
                width = chunk_width;
                continue;
            }
            line.push_str(&atom);
            width += atom_width + cross;
        }
        if !line.trim().is_empty() {
            lines.push(line.trim_end().to_string());
        }
    }
    lines
}

#[cfg(test)]
mod kerning_tests {
    use super::*;
    use crate::hint::{ChromeHint, SectionChrome};

    /// Body bytes are host-owned, so read the VuTeX pack beside this
    /// repository, or the directory named by `VUTEX_FONT_DIR`. A missing pack
    /// fails the test instead of skipping the kerning assertions.
    fn host_font_dir() -> std::path::PathBuf {
        if let Ok(dir) = std::env::var("VUTEX_FONT_DIR") {
            return std::path::PathBuf::from(dir);
        }
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .expect("workspace root")
            .join("VuTeX")
            .join("projects")
            .join("vutex.rs")
            .join("fonts")
    }

    fn metrics() -> &'static BodyMetrics {
        static METRICS: std::sync::OnceLock<BodyMetrics> = std::sync::OnceLock::new();
        METRICS.get_or_init(|| {
            let dir = host_font_dir();
            assert!(dir.is_dir(), "kerning regression needs the host font pack at {}; set VUTEX_FONT_DIR", dir.display());
            let set = document_fonts::BodyFaceSet::load_from(&dir).expect("body face pack");
            BodyMetrics::from_set(&set)
        })
    }

    fn measure() -> TextMeasure<'static> {
        TextMeasure::new(metrics())
    }

    fn atom(text: &str) -> InlineAtom {
        InlineAtom::Text { text: text.to_string(), face: BODY_FACE, href: None, title: None }
    }

    #[test]
    fn measure_counts_adjacent_pairs() {
        let widths = measure();
        let size = 10.0;
        let pair = widths.kern('A', 'V', BODY_FACE, 1.0);
        assert!((pair - -264.0 / 2048.0).abs() < 1e-12, "Tinos `A` + `V` is -264/2048 em, got {pair}");
        let advances = (widths.advance_em('A', BODY_FACE) + widths.advance_em('V', BODY_FACE)) * size;
        let expected = advances - 264.0 / 2048.0 * size;
        let width = widths.measure("AV", size, BODY_FACE);
        assert!((width - expected).abs() < 1e-9, "measured {width}, expected {expected}");
    }

    #[test]
    fn wrap_text_counts_the_pair_between_atoms() {
        let widths = measure();
        let size = 10.0;
        let without_cross =
            widths.measure("A", size, BODY_FACE) + widths.measure(" ", size, BODY_FACE) + widths.measure("V", size, BODY_FACE);
        let with_cross = widths.measure("A V", size, BODY_FACE);
        assert!(with_cross < without_cross, "the pairs must narrow the line");
        let lines = wrap_text(&widths, "A V", size, without_cross - 0.01);
        assert_eq!(lines, vec!["A V".to_string()]);
    }

    #[test]
    fn inline_break_counts_the_pair_between_atoms() {
        let widths = measure();
        let size = 10.0;
        let max_width =
            widths.measure("A", size, BODY_FACE) + widths.measure(" ", size, BODY_FACE) + widths.measure("V", size, BODY_FACE)
                - 0.01;
        let lines = break_inline_atoms(&widths, vec![atom("A"), atom(" "), atom("V")], size, max_width, 0.0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 3);
    }

    #[test]
    fn unreadable_body_face_falls_back_with_an_explicit_diagnostic() {
        // A truncated face that cannot supply `head`/`hmtx`: the first 64 bytes
        // of a zeroed buffer are not a parseable TrueType face.
        let damaged: &[u8] = &[0u8; 64];
        let metrics = BodyMetrics::from_faces([(
            document_fonts::BodyFaceKey {
                family: document_fonts::BodyFamily::Serif,
                style: document_fonts::BodyFaceStyle::Regular,
            },
            "damaged/Tinos".to_string(),
            damaged,
        )]);
        assert_eq!(metrics.unreadable(), ["damaged/Tinos".to_string()]);

        let mut graph = DocumentGraph::new(document_ir::DocumentId(7));
        graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: "Hello world".to_string() }] });
        let (plan, diagnostics) = layout_with_metrics(&graph, &LayoutHint::a4(), &Diagrams::new(), &metrics);

        let unreadable = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "vutex.layout.metrics.face_unreadable")
            .expect("an unreadable face is reported, never silently degraded");
        assert_eq!(unreadable.severity, Severity::Warning);
        assert!(unreadable.message.contains("damaged/Tinos"), "the message names the face: {}", unreadable.message);

        // The fallback still lays the text out with the heuristic width model,
        // so the user sees a page with a warning instead of an empty document.
        assert_eq!(plan.pages.len(), 1);
        assert!(plan.pages[0].items.iter().any(|item| matches!(item, LayoutItem::TextRun(_))));
    }

    #[test]
    fn template_section_break_starts_the_next_heading_on_a_new_page() {
        fn section(level: u8, title: &str) -> Block {
            Block::Section { level, title: vec![Inline::Text { text: title.to_string() }], children: Vec::new() }
        }
        let metrics = metrics();
        let mut graph = DocumentGraph::new(document_ir::DocumentId(11));
        graph.push_block(section(1, "One"));
        graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: "Body".to_string() }] });
        graph.push_block(section(1, "Two"));

        let plain = LayoutHint::a4();
        let (plain_plan, _) = layout_with_metrics(&graph, &plain, &Diagrams::new(), &metrics);
        assert_eq!(plain_plan.pages.len(), 1, "without a break both headings share the page");

        let mut broken = LayoutHint::a4();
        broken.section_breaks[0] = true;
        let (broken_plan, _) = layout_with_metrics(&graph, &broken, &Diagrams::new(), &metrics);
        assert_eq!(broken_plan.pages.len(), 2, "the second H1 opens a new page");
        assert!(
            broken_plan.pages[0].items.iter().any(|item| matches!(item, LayoutItem::TextRun(run) if run.text == "One")),
            "the first section keeps the opening page"
        );
        assert!(
            broken_plan.pages[1].items.iter().any(|item| matches!(item, LayoutItem::TextRun(run) if run.text == "Two")),
            "the second section starts the new page"
        );
    }

    #[test]
    fn template_chrome_paints_a_literal_header_on_the_first_page_without_a_folio() {
        let metrics = metrics();
        let mut graph = DocumentGraph::new(document_ir::DocumentId(12));
        graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: "Body".to_string() }] });

        let mut hint = LayoutHint::a4();
        hint.chrome =
            ChromeHint { header: ChromeHeader::Literal("ACME REPORT"), folio: false, start_page: 1, sections: [None; 4] };
        let (plan, _) = layout_with_metrics(&graph, &hint, &Diagrams::new(), &metrics);

        let runs: Vec<&TextRun> = plan.pages[0]
            .items
            .iter()
            .filter_map(|item| match item {
                LayoutItem::TextRun(run) => Some(run),
                _ => None,
            })
            .collect();
        assert!(
            runs.iter().any(|run| run.artifact && !run.numeric && run.text == "ACME REPORT"),
            "the template header paints on page one"
        );
        assert!(!runs.iter().any(|run| run.artifact && run.numeric), "folio is off for this template");
    }

    #[test]
    fn template_section_chrome_switches_header_and_folio_per_heading_level() {
        fn section(level: u8, title: &str) -> Block {
            Block::Section { level, title: vec![Inline::Text { text: title.to_string() }], children: Vec::new() }
        }
        let metrics = metrics();
        let mut graph = DocumentGraph::new(document_ir::DocumentId(14));
        graph.push_block(section(1, "One"));
        graph.push_block(section(2, "Sub"));
        graph.push_block(section(1, "Two"));

        let mut hint = LayoutHint::a4();
        hint.section_breaks = [true, true, false, false];
        hint.chrome = ChromeHint { header: ChromeHeader::Literal("TOP"), folio: true, start_page: 1, sections: [None; 4] };
        hint.chrome.sections[0] = Some(SectionChrome { header: Some(ChromeHeader::Literal("PART ONE")), folio: Some(false) });
        hint.chrome.sections[1] = Some(SectionChrome { header: Some(ChromeHeader::Literal("SUBSECTION")), folio: Some(true) });

        let (plan, _) = layout_with_metrics(&graph, &hint, &Diagrams::new(), &metrics);
        assert_eq!(plan.pages.len(), 3, "each forced heading opens a page");
        let chrome = |page: usize| -> Vec<(String, bool)> {
            plan.pages[page]
                .items
                .iter()
                .filter_map(|item| match item {
                    LayoutItem::TextRun(run) if run.artifact => Some((run.text.clone(), run.numeric)),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(chrome(0), vec![("PART ONE".to_string(), false)], "H1 chrome paints without a folio");
        assert_eq!(
            chrome(1),
            vec![("SUBSECTION".to_string(), false), ("2".to_string(), true)],
            "H2 chrome replaces the H1 entry and turns the folio back on"
        );
        assert_eq!(chrome(2), vec![("PART ONE".to_string(), false)], "the next H1 returns to the H1 entry");
    }

    #[test]
    fn template_style_reflow_changes_body_and_heading_sizes() {
        let metrics = metrics();
        let mut graph = DocumentGraph::new(document_ir::DocumentId(13));
        graph.push_block(Block::Section {
            level: 1,
            title: vec![Inline::Text { text: "Heading".to_string() }],
            children: Vec::new(),
        });
        graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: "Body".to_string() }] });

        let mut hint = LayoutHint::a4();
        hint.base_font_size = 12.0;
        hint.line_height = 1.8;
        hint.style.heading_scales = [1.6, 1.35, 1.18, 1.08];
        let (plan, _) = layout_with_metrics(&graph, &hint, &Diagrams::new(), &metrics);

        let sizes: Vec<(String, f64)> = plan.pages[0]
            .items
            .iter()
            .filter_map(|item| match item {
                LayoutItem::TextRun(run) => Some((run.text.clone(), run.size)),
                _ => None,
            })
            .collect();
        assert!(sizes.iter().any(|(text, size)| text == "Body" && (size - 12.0).abs() < 1e-9), "{sizes:?}");
        assert!(sizes.iter().any(|(text, size)| text == "Heading" && (size - 19.2).abs() < 1e-9), "{sizes:?}");
    }
}
