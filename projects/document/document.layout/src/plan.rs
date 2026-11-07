//! Computed layout output: pages of positioned primitives in points.

use document_compose::math::FontStyle;

/// One positioned text line.
///
/// `x` is the left edge of the line and `y` the text baseline. Layouts place
/// the baseline at `y = line_box_top + 0.8 * size`, so consumers that only
/// know CSS-style top offsets can recover the line box top.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// Left edge in points, measured from the page left.
    pub x: f64,
    /// Text baseline in points, measured from the page top.
    pub y: f64,
    /// Font size in points.
    pub size: f64,
    /// Horizontal width of the run in page coordinates, in points.
    ///
    /// Measured with the same advances the layout pass uses for line breaking,
    /// so a renderer can outline the exact glyph box instead of re-measuring.
    pub width: f64,
    /// Font family name such as `serif`, `sans-serif`, or `monospace`.
    pub font_family: String,
    /// Face style carried from the inline content, such as bold or italic.
    pub style: FontStyle,
    /// Line text.
    pub text: String,
    /// External link destination the run was authored inside, if any.
    ///
    /// Only [`document_ir::RelationEndpoint::ExternalUri`] targets resolve to a
    /// destination. Internal `Node` targets resolve through [`Self::anchor`]
    /// instead, and `Document`, `Asset`, and `Unresolved` targets stay `None`
    /// because a fabricated destination would point nowhere.
    pub href: Option<String>,
    /// Internal anchor this run links to, if any.
    ///
    /// The name matches one [`LayoutItem::Anchor`] in the same plan. Backends
    /// with named destinations, such as PDF, resolve it to a page jump; backends
    /// without them keep the run and its text.
    pub anchor: Option<String>,
    /// Hover text authored on the link that contains the run, if any.
    ///
    /// Only link display content carries a title; all other runs keep `None`.
    /// Renderers emit it with the anchor, never for a dropped destination.
    pub title: Option<String>,
    /// Ink colour the theme asked for, or `None` for the renderer default.
    pub color: Option<&'static str>,
    /// Whether the digits in this run should use tabular figures.
    pub numeric: bool,
    /// Whether this run is page furniture rather than authored content.
    ///
    /// The running header and folio live inside the page margins, so text
    /// extraction over the painted page would otherwise read them as body
    /// paragraphs. Renderers that write a text layer mark these runs as PDF
    /// `Artifact` content, which the reader drops by specification instead of
    /// guessing from geometry.
    pub artifact: bool,
}

/// A straight stroked line, used for thematic breaks and table rules.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// Start x in points.
    pub x1: f64,
    /// Start y in points.
    pub y1: f64,
    /// End x in points.
    pub x2: f64,
    /// End y in points.
    pub y2: f64,
    /// Stroke width in points.
    pub thickness: f64,
    /// Stroke colour, or `None` for the renderer default.
    pub color: Option<&'static str>,
}

/// A closed box in page coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Rect {
    /// Left edge in points.
    pub x: f64,
    /// Top edge in points.
    pub y: f64,
    /// Box width in points.
    pub width: f64,
    /// Box height in points.
    pub height: f64,
    /// Filled boxes carry a light background, unfilled boxes only an outline.
    pub filled: bool,
    /// Explicit fill colour, or `None` for the renderer's filled default.
    pub fill: Option<&'static str>,
    /// Outline colour, or `None` for no outline.
    pub stroke: Option<&'static str>,
    /// Outline width in points.
    pub stroke_width: f64,
}

/// Reserved area for an image whose intrinsic size is not known to layout.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePlaceholder {
    /// Left edge in points.
    pub x: f64,
    /// Top edge in points.
    pub y: f64,
    /// Box width in points.
    pub width: f64,
    /// Box height in points.
    pub height: f64,
    /// Alternative text carried for accessibility and diagnostics.
    pub alt: String,
    /// Authored image location, kept so a renderer can resolve the pixels.
    ///
    /// The value is the destination the document named, such as a `data:` URI
    /// or a filesystem path. Layout reserves the box without reading the
    /// source, so a renderer keeps the dashed slot when the value is absent or
    /// cannot be resolved.
    pub source: Option<String>,
}

/// A vector path in page coordinates.
///
/// The `d` string is an SVG `d` attribute already scaled to page points by the
/// layout pass, so renderers can translate it directly to their own path
/// representation. Coordinates inside `d` are relative to the path origin at
/// `(x, y)` — a typical path `M 0,0 L 10,0 ...` sits at `x`, `y` on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    /// SVG path data string (M/L/C/Q/Z/H/V/S/T/A operators, already page-scaled).
    pub d: String,
    /// Path origin x in points (left edge of the path's local coordinate space).
    pub x: f64,
    /// Path origin y in points (baseline of the path's local coordinate space).
    pub y: f64,
    /// Advance width in points.
    pub width: f64,
    /// Bounding-box height in points (above baseline).
    pub height: f64,
    /// Bounding-box depth in points (below baseline).
    pub depth: f64,
}

/// A diagram scene placed into a fixed box on the page.
///
/// The scene keeps its own canvas coordinate system; `width` and `height` are
/// the page-space box the renderer scales the scene into. Layout preserves the
/// scene aspect ratio, so the two scales match.
#[derive(Debug, Clone, PartialEq)]
pub struct Graphic {
    /// Left edge in points.
    pub x: f64,
    /// Top edge in points.
    pub y: f64,
    /// Placed box width in points.
    pub width: f64,
    /// Placed box height in points.
    pub height: f64,
    /// Resolved display scene emitted by the diagram producer.
    pub scene: plotter_scene::DisplayScene,
    /// Alternative text for backends that cannot paint the scene.
    pub alt: String,
}

/// A positioned page primitive produced by the document layout pass.
///
/// All coordinates use points with the origin at the page's top-left corner.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutItem {
    /// A single text line.
    TextRun(TextRun),
    /// A stroked line.
    Rule(Rule),
    /// A box, either outlined or filled.
    Rect(Rect),
    /// An image slot reserved at a fixed size.
    ImagePlaceholder(ImagePlaceholder),
    /// A diagram scene placed at a fixed size.
    Graphic(Graphic),
    /// A vector path, typically a glyph outline emitted directly from the
    /// embedded font so renderers can paint the exact curves instead of
    /// relying on a system font or synthetic metrics.
    Path(Path),
    /// A named destination at the top of a heading.
    ///
    /// In-document links authored as `#fragment` resolve to this name, so a
    /// backend with link annotations can emit a jump instead of dropping the
    /// destination and keeping only the display text.
    Anchor {
        /// Slug the authored fragment names, unique within the document.
        name: String,
        /// Top edge in points, measured from the page top.
        y: f64,
    },
}

/// One laid-out page before presentation lowering.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutPage {
    /// Page width in points.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// Positioned primitives in paint order.
    pub items: Vec<LayoutItem>,
}

/// Computed layout output consumed by presentation builders.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutPlan {
    /// Pages in document order.
    pub pages: Vec<LayoutPage>,
}
