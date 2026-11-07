//! Format-native PresentationML read model.
//!
//! The model mirrors the package: geometry stays in English Metric Units,
//! inherited values are resolved onto each slide shape and nothing is lowered
//! into `notedown-ir` here. `lower` performs that projection separately, so the
//! reader never invents a second semantic graph.

use std::collections::BTreeMap;

/// English Metric Units per CSS pixel at 96 dpi.
pub const EMU_PER_PX: f64 = 9525.0;
/// English Metric Units per typographic point.
pub const EMU_PER_PT: f64 = 12700.0;

/// Converts English Metric Units to CSS pixels at 96 dpi.
pub fn emu_to_px(emu: i64) -> f64 {
    emu as f64 / EMU_PER_PX
}

/// Converts English Metric Units to points.
pub fn emu_to_pt(emu: i64) -> f64 {
    emu as f64 / EMU_PER_PT
}

/// Whether an issue is a partial loss or a refused construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueStatus {
    /// The construct is carried with a known semantic gap.
    Partial,
    /// The construct is refused and never silently dropped.
    Unsupported,
}

/// Non-fatal import issue with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PptxIssue {
    /// Stable diagnostic code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Whether the construct is partial or refused.
    pub status: IssueStatus,
    /// One-based slide number when the issue is slide-scoped.
    pub slide: Option<u32>,
    /// Shape id when the issue is shape-scoped.
    pub shape: Option<u32>,
}

/// Slide canvas size in English Metric Units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideSize {
    /// Canvas width.
    pub cx: i64,
    /// Canvas height.
    pub cy: i64,
}

impl SlideSize {
    /// Canvas width in CSS pixels at 96 dpi.
    pub fn width_px(&self) -> f64 {
        emu_to_px(self.cx)
    }

    /// Canvas height in CSS pixels at 96 dpi.
    pub fn height_px(&self) -> f64 {
        emu_to_px(self.cy)
    }

    /// Canvas width in points.
    pub fn width_pt(&self) -> f64 {
        emu_to_pt(self.cx)
    }

    /// Canvas height in points.
    pub fn height_pt(&self) -> f64 {
        emu_to_pt(self.cy)
    }
}

/// Shape placement in English Metric Units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Left offset.
    pub x: i64,
    /// Top offset.
    pub y: i64,
    /// Width.
    pub cx: i64,
    /// Height.
    pub cy: i64,
    /// Clockwise rotation in degrees.
    pub rotation_deg: f64,
}

/// Shape or table-cell fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fill {
    /// No fill.
    None,
    /// Solid color as `#rrggbb`.
    Solid(String),
}

/// Shape outline.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Outline color as `#rrggbb` when resolved.
    pub color: Option<String>,
    /// Outline width in points.
    pub width_pt: f64,
}

/// Placeholder role declared by `p:ph`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceholderKind {
    /// Slide title.
    Title,
    /// Centered title on a title slide.
    CenterTitle,
    /// Subtitle.
    Subtitle,
    /// Body content.
    Body,
    /// Date.
    Date,
    /// Footer.
    Footer,
    /// Slide number.
    SlideNumber,
    /// Slide image on a notes page.
    SlideImage,
    /// Header.
    Header,
    /// Object placeholder.
    Object,
    /// Any other authored role, kept verbatim.
    Other(String),
}

impl PlaceholderKind {
    /// Parses the `type` attribute of `p:ph`.
    pub fn parse(value: &str) -> Self {
        match value {
            "title" => Self::Title,
            "ctrTitle" => Self::CenterTitle,
            "subTitle" => Self::Subtitle,
            "body" => Self::Body,
            "dt" => Self::Date,
            "ftr" => Self::Footer,
            "sldNum" => Self::SlideNumber,
            "sldImg" => Self::SlideImage,
            "hdr" => Self::Header,
            "obj" => Self::Object,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Stable spelling used by reports.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Title => "title",
            Self::CenterTitle => "ctrTitle",
            Self::Subtitle => "subTitle",
            Self::Body => "body",
            Self::Date => "dt",
            Self::Footer => "ftr",
            Self::SlideNumber => "sldNum",
            Self::SlideImage => "sldImg",
            Self::Header => "hdr",
            Self::Object => "obj",
            Self::Other(value) => value,
        }
    }

    /// Whether the role carries a slide title.
    pub fn is_title(&self) -> bool {
        matches!(self, Self::Title | Self::CenterTitle)
    }
}

/// Placeholder reference kept on a shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceholderRef {
    /// Declared role.
    pub kind: PlaceholderKind,
    /// Declared `idx` used to pair slide placeholders with layout placeholders.
    pub index: Option<u32>,
    /// Declared size hint such as `half` or `quarter`.
    pub size_hint: Option<String>,
}

/// Run-level text formatting after inheritance.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextStyle {
    /// Bold flag.
    pub bold: bool,
    /// Italic flag.
    pub italic: bool,
    /// Underline flag.
    pub underline: bool,
    /// Strikethrough flag.
    pub strike: bool,
    /// Font size in points.
    pub size_pt: Option<f64>,
    /// Text color as `#rrggbb`.
    pub color: Option<String>,
    /// Latin typeface.
    pub font: Option<String>,
    /// Declared language tag.
    pub language: Option<String>,
}

/// One text run.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// Authored text. `<a:br/>` becomes `\n`.
    pub text: String,
    /// Resolved run formatting.
    pub style: TextStyle,
    /// Hyperlink target when the run carries one.
    pub hyperlink: Option<String>,
}

/// One paragraph inside a text frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    /// Indent level in `0..=8`.
    pub level: u8,
    /// Declared alignment such as `l`, `ctr` or `r`.
    pub align: Option<String>,
    /// Runs in order.
    pub runs: Vec<Run>,
}

impl Paragraph {
    /// Concatenated run text.
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
}

/// Body properties carried by `a:bodyPr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyProperties {
    /// Whether text wraps inside the frame. `wrap="none"` disables wrapping.
    pub wrap: bool,
    /// Vertical anchor such as `t`, `ctr` or `b`.
    pub anchor: Option<String>,
    /// Declared autofit mode such as `normAutofit` or `spAutoFit`.
    pub autofit: Option<String>,
}

impl Default for BodyProperties {
    fn default() -> Self {
        Self { wrap: true, anchor: None, autofit: None }
    }
}

/// Text frame with resolved paragraphs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextFrame {
    /// Body properties.
    pub body: BodyProperties,
    /// Paragraphs in order.
    pub paragraphs: Vec<Paragraph>,
}

impl TextFrame {
    /// Concatenated paragraph text separated by newlines.
    pub fn text(&self) -> String {
        self.paragraphs.iter().map(Paragraph::text).collect::<Vec<_>>().join("\n")
    }

    /// Whether every paragraph carries no text.
    pub fn is_empty(&self) -> bool {
        self.paragraphs.iter().all(|paragraph| paragraph.text().trim().is_empty())
    }
}

/// One table cell.
#[derive(Debug, Clone, PartialEq)]
pub struct PptxTableCell {
    /// Cell text.
    pub text: TextFrame,
    /// Cell fill when authored.
    pub fill: Option<Fill>,
    /// Horizontal merge span.
    pub colspan: u32,
    /// Vertical merge span.
    pub rowspan: u32,
}

/// One table row.
#[derive(Debug, Clone, PartialEq)]
pub struct PptxTableRow {
    /// Row height in English Metric Units.
    pub height: i64,
    /// Cells in order.
    pub cells: Vec<PptxTableCell>,
}

/// Presentation table.
#[derive(Debug, Clone, PartialEq)]
pub struct PptxTable {
    /// Column widths in English Metric Units.
    pub column_widths: Vec<i64>,
    /// Rows in order.
    pub rows: Vec<PptxTableRow>,
}

/// Picture carried by a slide.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideImage {
    /// Package part path.
    pub part: String,
    /// Media type derived from the part extension.
    pub media_type: String,
    /// Image bytes when the part could be read.
    pub bytes: Option<Vec<u8>>,
}

/// One chart series with its cached values.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    /// Series name when the chart part carries one.
    pub name: Option<String>,
    /// Cached numeric values in plotting order.
    pub values: Vec<f64>,
}

/// Chart reference kept next to its frame.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartRef {
    /// Package part path.
    pub part: String,
    /// First chart plot family such as `barChart` or `pieChart`.
    pub kind: Option<String>,
    /// Declared series count.
    pub series_count: u32,
    /// Cached category labels.
    pub categories: Vec<String>,
    /// Series with cached values.
    pub series: Vec<ChartSeries>,
}

/// Shape family declared by the `p:spTree` child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    /// `p:sp` shape, including placeholders and text boxes.
    Shape,
    /// `p:pic` picture.
    Picture,
    /// `p:graphicFrame` carrying a table.
    Table,
    /// `p:graphicFrame` carrying a chart.
    Chart,
    /// Group or connector this reader refuses to flatten.
    Group,
    /// Any other frame.
    Other,
}

/// One positioned object on a slide.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// Authored shape id.
    pub id: u32,
    /// Authored shape name.
    pub name: String,
    /// Shape family.
    pub kind: ShapeKind,
    /// Placeholder reference when the shape is one.
    pub placeholder: Option<PlaceholderRef>,
    /// Resolved placement. `None` means the geometry could not be resolved.
    pub transform: Option<Transform>,
    /// Whether the placement came from the layout or master chain.
    pub inherited_transform: bool,
    /// Resolved fill.
    pub fill: Option<Fill>,
    /// Resolved outline.
    pub line: Option<Line>,
    /// Preset geometry name such as `rect` or `roundRect`.
    pub geometry: Option<String>,
    /// Text frame when the shape carries text.
    pub text: Option<TextFrame>,
    /// Picture payload when the shape is a picture.
    pub image: Option<SlideImage>,
    /// Table payload when the frame carries a table.
    pub table: Option<PptxTable>,
    /// Chart reference when the frame carries a chart.
    pub chart: Option<ChartRef>,
}

/// One slide.
#[derive(Debug, Clone, PartialEq)]
pub struct Slide {
    /// One-based slide number.
    pub index: u32,
    /// Package part path of the slide.
    pub part: String,
    /// Whether the slide is marked `show="0"`.
    pub hidden: bool,
    /// Package part path of the slide layout.
    pub layout_part: Option<String>,
    /// Authored layout name when readable.
    pub layout_name: Option<String>,
    /// Package part path of the slide master.
    pub master_part: Option<String>,
    /// Objects in `spTree` order.
    pub shapes: Vec<Shape>,
    /// Notes page text when the slide has one.
    pub notes: Option<String>,
    /// Slide background fill when authored.
    pub background: Option<Fill>,
}

/// Counts used by acceptance evidence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PptxStats {
    /// Number of slides.
    pub slide_count: u32,
    /// Number of slides marked hidden.
    pub hidden_slide_count: u32,
    /// Number of slide layouts referenced by the master.
    pub layout_count: u32,
    /// Number of slide masters.
    pub master_count: u32,
    /// Number of resolved shapes.
    pub shape_count: u32,
    /// Number of pictures.
    pub image_count: u32,
    /// Number of tables.
    pub table_count: u32,
    /// Number of charts.
    pub chart_count: u32,
    /// Number of notes pages with text.
    pub notes_count: u32,
    /// Total characters in text frames and table cells.
    pub char_count: u32,
    /// Non-whitespace characters in text frames and table cells.
    pub non_whitespace_chars: u32,
}

/// Parsed presentation.
#[derive(Debug, Clone, PartialEq)]
pub struct PptxPresentation {
    /// Source label used in reports.
    pub label: String,
    /// Canvas size.
    pub size: SlideSize,
    /// Document title from `docProps/core.xml` when present.
    pub title: Option<String>,
    /// Theme color slots resolved from `clrScheme` and `clrMap`.
    pub theme_colors: BTreeMap<String, String>,
    /// Slides in presentation order.
    pub slides: Vec<Slide>,
    /// Counts.
    pub stats: PptxStats,
    /// Issues collected while reading.
    pub issues: Vec<PptxIssue>,
}

impl PptxPresentation {
    /// Looks up a slide by its one-based number.
    pub fn slide(&self, index: u32) -> Option<&Slide> {
        self.slides.iter().find(|slide| slide.index == index)
    }
}

/// Stable codes for PresentationML import and view issues.
pub mod codes {
    /// A chart is detected and refused as a drawable object.
    pub const CHART: &str = "import.pptx.chart";
    /// SmartArt or another diagram part is refused.
    pub const SMART_ART: &str = "import.pptx.smartart";
    /// An embedded OLE object is refused.
    pub const OLE_OBJECT: &str = "import.pptx.ole-object";
    /// Audio or video media is refused.
    pub const MEDIA: &str = "import.pptx.media";
    /// A group shape or connector is refused instead of flattened.
    pub const GROUP: &str = "import.pptx.group";
    /// A shape rotation has no IR carrier.
    pub const ROTATION: &str = "import.pptx.rotation";
    /// Placeholder geometry or style was inherited from the layout or master.
    pub const PLACEHOLDER_INHERITANCE: &str = "import.pptx.placeholder-inheritance";
    /// Speaker notes are carried outside the slide body.
    pub const NOTES: &str = "import.pptx.notes";
    /// A hidden slide is excluded from the IR projection.
    pub const HIDDEN_SLIDE: &str = "import.pptx.hidden-slide";
    /// A table style id is detected but not applied.
    pub const TABLE_STYLE: &str = "import.pptx.table-style";
    /// A table cell carries several authored paragraphs.
    pub const TABLE_CELL_PARAGRAPHS: &str = "import.pptx.table-cell-paragraphs";
    /// A gradient, pattern or picture fill is flattened to no fill.
    pub const FILL_APPROXIMATED: &str = "import.pptx.fill-approximated";
    /// A theme color slot could not be resolved.
    pub const THEME_COLOR: &str = "import.pptx.theme-color";
    /// Shapes authored on the master or layout are not part of the slide model.
    pub const MASTER_SHAPE: &str = "import.pptx.master-shape";
    /// A transition or animation is detected but not modeled.
    pub const ANIMATION: &str = "import.pptx.animation";
    /// A macro project is refused.
    pub const MACRO: &str = "import.pptx.macro";
    /// The view text exceeded the frame and was clipped.
    pub const VIEW_TEXT_OVERFLOW: &str = "render.pptx.text-overflow";
    /// The view painted a chart frame as a refusal placeholder.
    pub const VIEW_CHART_PLACEHOLDER: &str = "render.pptx.chart-placeholder";
    /// The view skipped an object whose frame never resolved.
    pub const VIEW_SHAPE_SKIPPED: &str = "render.pptx.shape-skipped";
    /// The view could not embed image bytes.
    pub const VIEW_IMAGE_MISSING: &str = "render.pptx.image-missing";
    /// The view substituted a font family for an unresolved typeface.
    pub const VIEW_FONT_SUBSTITUTION: &str = "render.pptx.font-substitution";
}
