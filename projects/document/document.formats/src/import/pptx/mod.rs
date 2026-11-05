//! PresentationML PPTX import, IR projection and fixed-canvas slide view.
//!
//! The reader produces a format-native `PptxPresentation` that keeps geometry
//! in English Metric Units, resolves placeholder inheritance from the slide
//! layout and master chain and refuses charts, SmartArt, OLE objects, media and
//! group shapes with `import.pptx.*` codes. `lower` projects the visible text,
//! tables and pictures into the existing `notedown-ir`. `render` paints one SVG
//! canvas per slide at 96 dpi and reports object-level deviations.

pub mod lower;
pub mod model;
pub mod reader;
pub mod render;

pub use lower::{GraphOptions, import_pptx_graph, import_pptx_graph_bytes, presentation_to_graph};
pub use model::{
    BodyProperties, ChartRef, ChartSeries, Fill, IssueStatus, Line, Paragraph, PlaceholderKind, PlaceholderRef, PptxIssue,
    PptxPresentation, PptxStats, PptxTable, PptxTableCell, PptxTableRow, Run, Shape, ShapeKind, Slide, SlideImage, SlideSize,
    TextFrame, TextStyle, Transform, emu_to_pt, emu_to_px,
};
pub use reader::{import_pptx, import_pptx_bytes};
pub use render::{
    EstimatedMetrics, PptxRender, RenderOptions, ShapeDeviation, SlideView, TextMeasurer, TextWrapProbe, render_presentation,
    render_presentation_with, text_wrap_probe,
};
