pub(crate) struct PdfStream<'a> {
    pub(crate) object_id: Option<u32>,
    pub(crate) dictionary: &'a [u8],
    pub(crate) data: &'a [u8],
}

/// Decoded or referenced image payload extracted from a PDF.
#[derive(Clone, Debug)]
pub(crate) struct PdfImage {
    pub(crate) bytes: Vec<u8>,
    pub(crate) media_type: &'static str,
    pub(crate) source: String,
}

/// One glyph run on a baseline, with the geometry needed to split table cells.
#[derive(Clone, Debug)]
pub(crate) struct LineRun {
    pub(crate) text: String,
    /// Start of the run in text-space units when the stream positioned it.
    pub(crate) x: Option<f32>,
    /// Declared end of the run in text-space units when the font carries widths.
    pub(crate) end: Option<f32>,
}

/// A visual line rebuilt from glyph runs.
#[derive(Clone, Debug)]
pub(crate) struct ExtractedLine {
    pub(crate) text: String,
    pub(crate) x: Option<f32>,
    pub(crate) y: Option<f32>,
    /// Largest font size on the line, which is the size of its heading glyphs.
    pub(crate) size: f32,
    pub(crate) runs: Vec<LineRun>,
}

/// Ordered page content produced while walking content streams.
#[derive(Debug)]
pub(crate) enum PageItem {
    Line(ExtractedLine),
    /// Named XObject reference from a `Do` operator.
    NamedImage(String),
    /// Inline image operator sequence.
    InlineImage(PdfImage),
}

/// Text diagnostics collected while walking PDF text operators.
#[derive(Default)]
pub(crate) struct ExtractedText {
    pub(crate) spacing_adjustments: bool,
    /// Font resource names whose encoding could not be resolved. Text drawn
    /// with them is omitted and reported as a loss marker.
    pub(crate) unresolved_fonts: std::collections::BTreeSet<String>,
}
