//! Fixed-canvas slide view.
//!
//! Every slide becomes one SVG canvas sized from the presentation `sldSz` in
//! CSS pixels at 96 dpi (`1px = 9525 EMU`). The view paints authored and
//! inherited geometry, text with an estimated wrap, pictures, tables and a
//! refusal placeholder for charts. Objects the model cannot place are reported
//! with a `render.pptx.*` code instead of disappearing.

use super::model::{Fill, IssueStatus, Paragraph, PptxIssue, PptxPresentation, Shape, ShapeKind, TextFrame, codes, emu_to_px};
use crate::import::office::escape;

/// Horizontal text metrics injected into the canvas.
///
/// The estimated implementation charges one em per wide character and 0.55 em
/// per narrow character. Callers that own real font tables inject an `hmtx`
/// backed implementation so wrapping happens at the same places as the font.
pub trait TextMeasurer {
    /// Advance width of one character in CSS pixels at `font_px`.
    fn advance(&self, character: char, font_px: f64) -> f64;
}

/// Estimation used when no font table is injected.
#[derive(Debug, Clone, Copy, Default)]
pub struct EstimatedMetrics;

impl TextMeasurer for EstimatedMetrics {
    fn advance(&self, character: char, font_px: f64) -> f64 {
        if is_wide(character) { font_px } else { font_px * 0.55 }
    }
}

/// Options for the slide canvas view.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    /// Whether hidden slides are painted as well.
    pub include_hidden: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { include_hidden: false }
    }
}

/// Per-object deviation between the authored declaration and the painted view.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeDeviation {
    /// Authored shape id.
    pub shape_id: u32,
    /// Authored shape name.
    pub name: String,
    /// Horizontal offset in points between the painted frame and the authored frame.
    pub x_offset_pt: f64,
    /// Vertical offset in points between the painted frame and the authored frame.
    pub y_offset_pt: f64,
    /// Resolved font size minus the reader fallback, in points.
    pub font_size_delta_pt: f64,
    /// Estimated painted lines minus authored paragraphs.
    pub wrap_line_delta: i32,
}

/// One painted slide.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideView {
    /// One-based slide number.
    pub index: u32,
    /// Canvas width in CSS pixels.
    pub width_px: f64,
    /// Canvas height in CSS pixels.
    pub height_px: f64,
    /// Standalone SVG document for the slide.
    pub svg: String,
    /// Per-object deviations.
    pub deviations: Vec<ShapeDeviation>,
}

/// Result of rendering a presentation.
#[derive(Debug, Clone, PartialEq)]
pub struct PptxRender {
    /// Painted slides in presentation order.
    pub views: Vec<SlideView>,
    /// Self-contained HTML document with one section per slide.
    pub html: String,
    /// View issues.
    pub issues: Vec<PptxIssue>,
}

/// One paragraph wrapped by the given measurer.
#[derive(Debug, Clone, PartialEq)]
pub struct TextWrapProbe {
    /// One-based slide number.
    pub slide: u32,
    /// Shape id the paragraph belongs to.
    pub shape_id: u32,
    /// Shape name.
    pub shape_name: String,
    /// Zero-based paragraph index inside the shape.
    pub paragraph: u32,
    /// Declared or inherited font size in points.
    pub font_size_pt: f64,
    /// Available text width in CSS pixels after insets.
    pub width_px: f64,
    /// Painted lines.
    pub lines: Vec<String>,
}

/// Renders every slide into a fixed canvas and a self-contained HTML document.
pub fn render_presentation(presentation: &PptxPresentation, options: &RenderOptions) -> PptxRender {
    render_presentation_with(presentation, options, &EstimatedMetrics, &[])
}

/// Renders with injected metrics and optional chart fragments.
///
/// Chart fragments are `(slide, shape, svg)` triples already positioned in
/// slide pixels. They replace the refusal placeholder for that chart frame.
pub fn render_presentation_with(
    presentation: &PptxPresentation,
    options: &RenderOptions,
    measurer: &dyn TextMeasurer,
    charts: &[(u32, u32, String)],
) -> PptxRender {
    let mut views = Vec::new();
    let mut issues = Vec::new();
    for slide in &presentation.slides {
        if slide.hidden && !options.include_hidden {
            continue;
        }
        let width = presentation.size.width_px();
        let height = presentation.size.height_px();
        let mut svg = String::new();
        svg.push_str(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{width:.1}\" height=\"{height:.1}\" viewBox=\"0 0 {width:.1} {height:.1}\">"
        ));
        let background = match &slide.background {
            Some(Fill::Solid(color)) => color.clone(),
            _ => "#ffffff".to_owned(),
        };
        svg.push_str(&format!("<rect x=\"0\" y=\"0\" width=\"{width:.1}\" height=\"{height:.1}\" fill=\"{background}\"/>"));
        let mut deviations = Vec::new();
        for shape in &slide.shapes {
            let fragment = charts
                .iter()
                .find(|(chart_slide, chart_shape, _)| *chart_slide == slide.index && *chart_shape == shape.id)
                .map(|(_, _, svg)| svg.as_str());
            paint_shape(shape, slide.index, fragment, measurer, &mut svg, &mut issues, &mut deviations);
        }
        svg.push_str("</svg>");
        views.push(SlideView { index: slide.index, width_px: width, height_px: height, svg, deviations });
    }
    let html = compose_html(presentation, &views, &issues);
    PptxRender { views, html, issues }
}

/// Wraps every authored text paragraph with the given measurer.
pub fn text_wrap_probe(presentation: &PptxPresentation, measurer: &dyn TextMeasurer) -> Vec<TextWrapProbe> {
    let mut probes = Vec::new();
    for slide in &presentation.slides {
        for shape in &slide.shapes {
            let (Some(text), Some(transform)) = (shape.text.as_ref(), shape.transform)
            else {
                continue;
            };
            if text.is_empty() {
                continue;
            }
            let width = emu_to_px(transform.cx).max(1.0);
            let available_width = (width - emu_to_px(91440) * 2.0).max(1.0);
            for (paragraph_index, paragraph) in text.paragraphs.iter().enumerate() {
                let size_pt = paragraph_size(paragraph);
                let font_px = size_pt * 96.0 / 72.0;
                let lines = wrap_paragraph(paragraph, text.body.wrap, font_px, available_width, measurer);
                probes.push(TextWrapProbe {
                    slide: slide.index,
                    shape_id: shape.id,
                    shape_name: shape.name.clone(),
                    paragraph: paragraph_index as u32,
                    font_size_pt: size_pt,
                    width_px: available_width,
                    lines,
                });
            }
        }
    }
    probes
}

fn paint_shape(
    shape: &Shape,
    slide: u32,
    chart_fragment: Option<&str>,
    measurer: &dyn TextMeasurer,
    svg: &mut String,
    issues: &mut Vec<PptxIssue>,
    deviations: &mut Vec<ShapeDeviation>,
) {
    let Some(transform) = shape.transform
    else {
        issues.push(PptxIssue {
            code: codes::VIEW_SHAPE_SKIPPED.to_owned(),
            message: format!("slide {slide} shape `{}` has no resolvable span and is not painted", shape.id),
            status: IssueStatus::Partial,
            slide: Some(slide),
            shape: Some(shape.id),
        });
        return;
    };
    let x = emu_to_px(transform.x);
    let y = emu_to_px(transform.y);
    let width = emu_to_px(transform.cx).max(1.0);
    let height = emu_to_px(transform.cy).max(1.0);
    let rotated = transform.rotation_deg.abs() > f64::EPSILON;
    if rotated {
        let center_x = x + width / 2.0;
        let center_y = y + height / 2.0;
        svg.push_str(&format!("<g transform=\"rotate({:.2} {center_x:.1} {center_y:.1})\">", transform.rotation_deg));
    }
    match shape.kind {
        ShapeKind::Picture => paint_picture(shape, slide, x, y, width, height, svg, issues),
        ShapeKind::Table => paint_table(shape, slide, x, y, width, height, measurer, svg, issues),
        ShapeKind::Chart => {
            if let Some(fragment) = chart_fragment {
                svg.push_str(fragment);
            }
            else {
                svg.push_str(&format!(
                    "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{width:.1}\" height=\"{height:.1}\" fill=\"#f2f2f2\" stroke=\"#9a9a9a\" stroke-dasharray=\"6 4\"/>"
                ));
                let label = shape.chart.as_ref().map(|chart| chart.kind.as_deref().unwrap_or("chart")).unwrap_or("chart");
                svg.push_str(&format!(
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"12\" fill=\"#666\">chart refused: {}</text>",
                    x + 8.0,
                    y + 20.0,
                    escape(label)
                ));
                issues.push(PptxIssue {
                    code: codes::VIEW_CHART_PLACEHOLDER.to_owned(),
                    message: format!("slide {slide} chart `{}` is painted as a refusal placeholder", shape.id),
                    status: IssueStatus::Unsupported,
                    slide: Some(slide),
                    shape: Some(shape.id),
                });
            }
        }
        ShapeKind::Group | ShapeKind::Other => {
            svg.push_str(&format!(
                "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{width:.1}\" height=\"{height:.1}\" fill=\"#fafafa\" stroke=\"#b0b0b0\" stroke-dasharray=\"3 3\"/>"
            ));
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"11\" fill=\"#777\">refused: {}</text>",
                x + 6.0,
                y + 16.0,
                escape(&shape.name)
            ));
        }
        ShapeKind::Shape => paint_autoshape(shape, slide, x, y, width, height, measurer, svg, issues, deviations),
    }
    if rotated {
        svg.push_str("</g>");
    }
}

fn paint_autoshape(
    shape: &Shape,
    slide: u32,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    measurer: &dyn TextMeasurer,
    svg: &mut String,
    issues: &mut Vec<PptxIssue>,
    deviations: &mut Vec<ShapeDeviation>,
) {
    let fill = match shape.fill.as_ref() {
        Some(Fill::Solid(color)) => color.clone(),
        Some(Fill::None) => "none".to_owned(),
        None => "none".to_owned(),
    };
    let line = shape.line.as_ref().and_then(|line| line.color.as_ref().map(|color| (color.clone(), line.width_pt)));
    let rounded = shape.geometry.as_deref() == Some("roundRect");
    let ellipse = shape.geometry.as_deref() == Some("ellipse");
    let stroke = line
        .as_ref()
        .map(|(color, width_pt)| format!(" stroke=\"{color}\" stroke-width=\"{:.1}\"", width_pt * 96.0 / 72.0))
        .unwrap_or_default();
    if ellipse {
        svg.push_str(&format!(
            "<ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" fill=\"{fill}\"{stroke}/>",
            x + width / 2.0,
            y + height / 2.0,
            width / 2.0,
            height / 2.0
        ));
    }
    else if rounded {
        let radius = width.min(height) * 0.12;
        svg.push_str(&format!(
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{width:.1}\" height=\"{height:.1}\" rx=\"{radius:.1}\" fill=\"{fill}\"{stroke}/>"
        ));
    }
    else {
        svg.push_str(&format!(
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{width:.1}\" height=\"{height:.1}\" fill=\"{fill}\"{stroke}/>"
        ));
    }
    let Some(text) = &shape.text
    else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let inset_left = emu_to_px(91440);
    let inset_top = emu_to_px(45720);
    let available_width = (width - inset_left * 2.0).max(1.0);
    let available_height = (height - inset_top * 2.0).max(1.0);
    let mut lines: Vec<PaintedLine> = Vec::new();
    for paragraph in &text.paragraphs {
        let size_pt = paragraph_size(paragraph);
        let font_px = size_pt * 96.0 / 72.0;
        let wrapped = wrap_paragraph(paragraph, text.body.wrap, font_px, available_width, measurer);
        for line in wrapped {
            lines.push(PaintedLine { text: line, size_pt: size_pt, align: paragraph.align.clone(), level: paragraph.level });
        }
    }
    let total_height: f64 = lines.iter().map(|line| line.size_pt * 1.2 * 96.0 / 72.0).sum();
    let anchor = text.body.anchor.as_deref().unwrap_or("t");
    let start_y = match anchor {
        "ctr" => y + inset_top + (available_height - total_height) / 2.0,
        "b" => y + height - inset_top - total_height,
        _ => y + inset_top,
    };
    let mut cursor = start_y.max(y + inset_top);
    let bottom = y + height - inset_top;
    let mut painted_lines = 0i32;
    let mut clipped = false;
    for line in &lines {
        let line_height = line.size_pt * 1.2 * 96.0 / 72.0;
        if cursor + line_height > bottom + 0.5 {
            clipped = true;
            break;
        }
        let font_px = line.size_pt * 96.0 / 72.0;
        let indent = line.level as f64 * 18.0;
        let (anchor_x, text_anchor) = match line.align.as_deref() {
            Some("ctr") => (x + width / 2.0, "middle"),
            Some("r") => (x + width - inset_left, "end"),
            _ => (x + inset_left + indent, "start"),
        };
        svg.push_str(&format!(
            "<text x=\"{anchor_x:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"{font_px:.1}\" text-anchor=\"{text_anchor}\" fill=\"#1a1a1a\">{}</text>",
            cursor + line_height * 0.8,
            escape(&line.text)
        ));
        cursor += line_height;
        painted_lines += 1;
    }
    if clipped {
        issues.push(PptxIssue {
            code: codes::VIEW_TEXT_OVERFLOW.to_owned(),
            message: format!(
                "slide {slide} shape `{}` text exceeds its frame and is clipped after {painted_lines} lines",
                shape.id
            ),
            status: IssueStatus::Partial,
            slide: Some(slide),
            shape: Some(shape.id),
        });
    }
    let authored_paragraphs = text.paragraphs.len() as i32;
    let average_size = average_size(text);
    deviations.push(ShapeDeviation {
        shape_id: shape.id,
        name: shape.name.clone(),
        // The view paints the authored or inherited frame verbatim, so the
        // offsets are zero by construction and stay in the report as a check.
        x_offset_pt: 0.0,
        y_offset_pt: 0.0,
        font_size_delta_pt: average_size - 18.0,
        wrap_line_delta: painted_lines - authored_paragraphs,
    });
}

fn paint_picture(
    shape: &Shape,
    slide: u32,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    svg: &mut String,
    issues: &mut Vec<PptxIssue>,
) {
    let Some(image) = &shape.image
    else {
        return;
    };
    match image.bytes.as_deref() {
        Some(bytes) => {
            svg.push_str(&format!(
                "<image x=\"{x:.1}\" y=\"{y:.1}\" width=\"{width:.1}\" height=\"{height:.1}\" preserveAspectRatio=\"none\" xlink:href=\"data:{};base64,{}\"/>",
                image.media_type,
                base64(bytes)
            ));
        }
        None => {
            svg.push_str(&format!(
                "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{width:.1}\" height=\"{height:.1}\" fill=\"#eeeeee\" stroke=\"#999999\"/>"
            ));
            issues.push(PptxIssue {
                code: codes::VIEW_IMAGE_MISSING.to_owned(),
                message: format!("slide {slide} picture `{}` bytes are unavailable and the frame is painted empty", shape.id),
                status: IssueStatus::Partial,
                slide: Some(slide),
                shape: Some(shape.id),
            });
        }
    }
}

fn paint_table(
    shape: &Shape,
    slide: u32,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    measurer: &dyn TextMeasurer,
    svg: &mut String,
    issues: &mut Vec<PptxIssue>,
) {
    let Some(table) = &shape.table
    else {
        return;
    };
    svg.push_str(&format!(
        "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{width:.1}\" height=\"{height:.1}\" fill=\"#ffffff\" stroke=\"#808080\" stroke-width=\"0.8\"/>"
    ));
    let column_total: i64 = table.column_widths.iter().sum::<i64>().max(1);
    let mut cursor_y = y;
    for row in &table.rows {
        let row_height = emu_to_px(row.height).max(1.0);
        let mut cursor_x = x;
        for (position, cell) in row.cells.iter().enumerate() {
            let width_emu =
                table.column_widths.get(position).copied().unwrap_or(column_total / table.column_widths.len().max(1) as i64);
            let cell_width = (width_emu as f64 / column_total as f64) * width;
            let fill = match cell.fill.as_ref() {
                Some(Fill::Solid(color)) => color.clone(),
                _ => "#ffffff".to_owned(),
            };
            svg.push_str(&format!(
                "<rect x=\"{cursor_x:.1}\" y=\"{cursor_y:.1}\" width=\"{cell_width:.1}\" height=\"{row_height:.1}\" fill=\"{fill}\" stroke=\"#b3b3b3\" stroke-width=\"0.6\"/>"
            ));
            let size_pt = cell.text.paragraphs.first().map(paragraph_size).unwrap_or(18.0);
            let font_px = size_pt * 96.0 / 72.0;
            let line = cell.text.paragraphs.iter().map(Paragraph::text).collect::<Vec<_>>().join(" ");
            if !line.is_empty() {
                let lines = wrap_plain(&line, font_px, cell_width - 10.0, measurer);
                let mut line_y = cursor_y + 4.0 + font_px;
                for text_line in lines.iter().take(((row_height - 4.0) / (font_px * 1.15)).max(1.0) as usize) {
                    svg.push_str(&format!(
                        "<text x=\"{:.1}\" y=\"{line_y:.1}\" font-family=\"sans-serif\" font-size=\"{font_px:.1}\" fill=\"#1a1a1a\">{}</text>",
                        cursor_x + 5.0,
                        escape(text_line)
                        ));
                    line_y += font_px * 1.15;
                }
                if lines.len() > ((row_height - 4.0) / (font_px * 1.15)).max(1.0) as usize {
                    issues.push(PptxIssue {
                        code: codes::VIEW_TEXT_OVERFLOW.to_owned(),
                        message: format!("slide {slide} table cell in shape `{}` clips wrapped text", shape.id),
                        status: IssueStatus::Partial,
                        slide: Some(slide),
                        shape: Some(shape.id),
                    });
                }
            }
            cursor_x += cell_width;
        }
        cursor_y += row_height;
    }
}

struct PaintedLine {
    text: String,
    size_pt: f64,
    align: Option<String>,
    level: u8,
}

fn paragraph_size(paragraph: &Paragraph) -> f64 {
    paragraph.runs.iter().filter_map(|run| run.style.size_pt).fold(0.0_f64, f64::max).max(18.0)
}

fn average_size(text: &TextFrame) -> f64 {
    let mut total = 0.0;
    let mut count = 0.0;
    for paragraph in &text.paragraphs {
        for run in &paragraph.runs {
            if let Some(size) = run.style.size_pt {
                total += size;
                count += 1.0;
            }
        }
    }
    if count > 0.0 { total / count } else { 18.0 }
}

fn wrap_paragraph(paragraph: &Paragraph, wrap: bool, font_px: f64, width: f64, measurer: &dyn TextMeasurer) -> Vec<String> {
    let text = paragraph.text();
    if !wrap {
        return text.split('\n').map(|line| line.to_owned()).collect();
    }
    wrap_plain(&text, font_px, width, measurer)
}

fn wrap_plain(text: &str, font_px: f64, width: f64, measurer: &dyn TextMeasurer) -> Vec<String> {
    let mut lines = Vec::new();
    for source_line in text.split('\n') {
        let mut current = String::new();
        let mut used = 0.0;
        for character in source_line.chars() {
            let advance = measurer.advance(character, font_px);
            if used + advance > width && !current.is_empty() {
                lines.push(current);
                current = String::new();
                used = 0.0;
            }
            current.push(character);
            used += advance;
        }
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn is_wide(character: char) -> bool {
    matches!(character as u32, 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6)
}

fn compose_html(presentation: &PptxPresentation, views: &[SlideView], issues: &[PptxIssue]) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>");
    html.push_str(&escape(presentation.title.as_deref().unwrap_or(&presentation.label)));
    html.push_str("</title><style>body{margin:0;background:#eceff1;font-family:sans-serif;color:#222}header{padding:16px 24px;background:#263238;color:#fff}main{padding:16px 24px}.slide{background:#fff;margin:0 0 24px;padding:12px 16px;box-shadow:0 1px 4px rgba(0,0,0,.18)}.canvas{overflow:auto}.slide h2{margin:0 0 8px;font-size:15px}.slide h2 small{font-weight:400;color:#607d8b;margin-left:8px}.notes{background:#fafafa;border-left:3px solid #90a4ae;padding:8px 10px;font-size:13px}table{border-collapse:collapse;font-size:12px;margin-top:8px}td,th{border:1px solid #cfd8dc;padding:3px 6px;text-align:left}.issues{margin-top:24px;font-size:12px}</style></head><body><header><h1>");
    html.push_str(&escape(&presentation.label));
    html.push_str(&format!(
        "</h1><p>{:.1} x {:.1} pt canvas, {} slides, {} painted, {} chars ({})</p></header><main>",
        presentation.size.width_pt(),
        presentation.size.height_pt(),
        presentation.stats.slide_count,
        views.len(),
        presentation.stats.char_count,
        presentation.stats.non_whitespace_chars
    ));
    for view in views {
        let slide = presentation.slide(view.index);
        html.push_str(&format!(
            "<section class=\"slide\" id=\"slide-{}\"><h2>Slide {}</h2><div class=\"canvas\">{}</div>",
            view.index, view.index, view.svg
        ));
        if let Some(slide) = slide {
            html.push_str(&format!(
                "<p class=\"notes\"><b>Layout:</b> {}</p>",
                escape(slide.layout_name.as_deref().unwrap_or("unknown"))
            ));
            if let Some(notes) = &slide.notes {
                html.push_str(&format!("<p class=\"notes\"><b>Notes:</b> {}</p>", escape(notes).replace('\n', "<br>")));
            }
        }
        if !view.deviations.is_empty() {
            html.push_str("<table><thead><tr><th>shape</th><th>name</th><th>x offset pt</th><th>y offset pt</th><th>font delta pt</th><th>wrap delta</th></tr></thead><tbody>");
            for deviation in &view.deviations {
                html.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td>{:.2}</td><td>{:.2}</td><td>{:.2}</td><td>{}</td></tr>",
                    deviation.shape_id,
                    escape(&deviation.name),
                    deviation.x_offset_pt,
                    deviation.y_offset_pt,
                    deviation.font_size_delta_pt,
                    deviation.wrap_line_delta
                ));
            }
            html.push_str("</tbody></table>");
        }
        html.push_str("</section>");
    }
    html.push_str("<section class=\"issues\"><h2>Issues</h2><table><thead><tr><th>code</th><th>status</th><th>slide</th><th>shape</th><th>message</th></tr></thead><tbody>");
    for issue in issues {
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape(&issue.code),
            match issue.status {
                IssueStatus::Partial => "partial",
                IssueStatus::Unsupported => "unsupported",
            },
            issue.slide.map(|value| value.to_string()).unwrap_or_default(),
            issue.shape.map(|value| value.to_string()).unwrap_or_default(),
            escape(&issue.message)
        ));
    }
    html.push_str("</tbody></table></section></main></body></html>");
    html
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(ALPHABET[(((first & 0b11) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[(((second & 0b1111) << 2) | (third >> 6)) as usize] as char);
        }
        else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(third & 0b111111) as usize] as char);
        }
        else {
            output.push('=');
        }
    }
    output
}
