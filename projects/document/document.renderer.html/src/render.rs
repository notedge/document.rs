//! HTML output for fixed-geometry snapshots and semantic reflow.

use document_compose::{Diagnostic, Severity, math::FontStyle};
use document_layout::math::layout_formula_for;
use document_presentation::{
    display::{DisplayGraphic, DisplayItem, DisplayPath, DisplayPresentation, DisplayTextRun},
    flow::{FlowBlock, FlowPresentation, FlowRun, FlowTableRow},
};
use document_renderer::is_safe_href;
use document_renderer_svg::math::{FormulaSvg, GlyphDefs, render_formula_result_with_glyph_defs};
use document_text::math::MathFontFamily;

/// MATH family and body size a reflow render typesets formulas with.
///
/// Formula layout needs the face the host installed and the body size the
/// surrounding text is set at, because the engine resolves every metric
/// relative to that em. A render without this context keeps authored source
/// text visible instead of typesetting.
#[derive(Debug, Clone, Copy)]
pub struct FlowMathContext {
    /// OpenType MATH family the host activated.
    pub family: MathFontFamily,
    /// Body size in points the formula scales from.
    pub base_size: f64,
}

/// Reflow HTML plus the diagnostics formula layout raised.
#[derive(Debug)]
pub struct FlowHtml {
    /// Complete HTML document.
    pub markup: String,
    /// Warnings from formulas that kept their source text, plus the outline
    /// warnings the SVG math backend reports.
    pub diagnostics: Vec<Diagnostic>,
}

/// Snapshot HTML maps fixed layout geometry into absolute-positioned markup.
///
/// Every page becomes a `data-vutex-page` section with point-sized geometry;
/// every primitive becomes a `data-vutex-item` element carrying the same text.
pub fn render_html_snapshot(presentation: &DisplayPresentation) -> Result<String, Diagnostic> {
    let mut out = String::new();
    out.push_str(
        "<!doctype html>\n<html lang=\"und\">\n<head>\n<meta charset=\"utf-8\">\n<title>VuTeX snapshot</title>\n<style>\n\
body{margin:0;padding:24px;background:#f5f5f5}\
.vutex-page{position:relative;margin:0 auto 24px;background:#fff;box-shadow:0 0 4px rgba(0,0,0,.15)}\
.vutex-text{position:absolute;white-space:pre;line-height:1}\
.vutex-rule{position:absolute;background:#111}\
.vutex-link{text-decoration:underline;text-decoration-thickness:.5pt}\
.vutex-image{position:absolute;box-sizing:border-box;border:1px dashed #999;color:#666;font:8pt sans-serif}\
.vutex-graphic{position:absolute;overflow:hidden}\
</style>\n</head>\n<body>\n",
    );
    for (index, page) in presentation.pages.iter().enumerate() {
        out.push_str(&format!(
            "<section class=\"vutex-page\" data-vutex-page=\"{}\" style=\"width:{}pt;height:{}pt\">\n",
            index + 1,
            num(page.width),
            num(page.height),
        ));
        for item in &page.items {
            write_item(&mut out, item)?;
        }
        out.push_str("</section>\n");
    }
    out.push_str("</body>\n</html>\n");
    Ok(out)
}

/// Reflow HTML maps semantic flow presentation into responsive markup.
///
/// Blocks carry `data-vutex-block` roles so hosts can restyle them, and all
/// text stays in the document flow for accessibility and text extraction.
/// Formulas keep their authored source text because no MATH context is given;
/// [`render_html_reflow_with_math`] typesets them instead.
pub fn render_html_reflow(presentation: &FlowPresentation) -> Result<String, Diagnostic> {
    Ok(render_flow(presentation, None)?.markup)
}

/// Reflow HTML that typesets formulas into inline SVG.
///
/// Inline formulas become sized inline boxes on the surrounding baseline, and
/// display formulas become their own `data-vutex-block="math"` line. A formula
/// the engine cannot lay out keeps its source text and adds a warning, so the
/// output never silently loses content. The returned [`FlowHtml`] carries those
/// warnings for the caller's report.
pub fn render_html_reflow_with_math(presentation: &FlowPresentation, math: &FlowMathContext) -> Result<FlowHtml, Diagnostic> {
    render_flow(presentation, Some(*math))
}

fn render_flow(presentation: &FlowPresentation, math: Option<FlowMathContext>) -> Result<FlowHtml, Diagnostic> {
    if !is_valid_tag(&presentation.root_tag) {
        return Err(Diagnostic {
            code: "vutex.render.html.reflow.invalid-root-tag".into(),
            severity: Severity::Error,
            message: format!("flow root tag `{}` is not a plain element name", presentation.root_tag),
            span: None,
        });
    }
    let mut out = String::new();
    out.push_str(
        "<!doctype html>\n<html lang=\"und\">\n<head>\n<meta charset=\"utf-8\">\n<title>VuTeX document</title>\n<style>.vutex-link{color:#2B5B84;text-decoration:underline;text-decoration-thickness:.5pt}.vutex-math svg{display:block}</style>\n</head>\n<body>\n",
    );
    let mut blocks = String::new();
    let mut diagnostics = Vec::new();
    let mut glyphs = GlyphDefs::new();
    for block in &presentation.blocks {
        write_flow_block(&mut blocks, block, math, &mut glyphs, &mut diagnostics);
    }
    // Every formula references shared outlines, so the definitions must live
    // in the same document. The container paints nothing but stays resolvable.
    if !glyphs.is_empty() {
        out.push_str(&format!(
            "<svg class=\"vutex-math-defs\" aria-hidden=\"true\" style=\"position:absolute;width:0;height:0;overflow:hidden\"><defs>{}</defs></svg>\n",
            glyphs.defs(),
        ));
    }
    out.push_str(&format!("<{} class=\"vutex-flow\" data-vutex=\"flow\">\n", presentation.root_tag));
    out.push_str(&blocks);
    out.push_str(&format!("</{}>\n</body>\n</html>\n", presentation.root_tag));
    Ok(FlowHtml { markup: out, diagnostics })
}

fn write_item(out: &mut String, item: &DisplayItem) -> Result<(), Diagnostic> {
    match item {
        DisplayItem::TextRun(run) => {
            out.push_str(&format!(
                "<div class=\"vutex-text\" data-vutex-item=\"text\" style=\"left:{}pt;top:{}pt;font-size:{}pt;font-family:{}{}{}{}\">",
                num(run.x),
                num(run.y - 0.8 * run.size),
                num(run.size),
                escape_attribute(&run.font_family),
                style_declarations(run.style),
                run.color.map(|color| format!(";color:{color}")).unwrap_or_default(),
                if run.numeric { ";font-variant-numeric:tabular-nums" } else { "" },
            ));
            write_snapshot_text(out, run);
            out.push_str("</div>\n");
        }
        DisplayItem::Rule(rule) => {
            let (left, top, width, height) = if (rule.y2 - rule.y1).abs() <= f64::EPSILON {
                let left = rule.x1.min(rule.x2);
                (left, rule.y1 - rule.thickness / 2.0, (rule.x2 - rule.x1).abs(), rule.thickness)
            }
            else {
                (rule.x1 - rule.thickness / 2.0, rule.y1.min(rule.y2), rule.thickness, (rule.y2 - rule.y1).abs())
            };
            out.push_str(&format!(
                "<div class=\"vutex-rule\" data-vutex-item=\"rule\" style=\"left:{}pt;top:{}pt;width:{}pt;height:{}pt{}\"></div>\n",
                num(left),
                num(top),
                num(width),
                num(height),
                rule.color.map(|color| format!(";background:{color}")).unwrap_or_default(),
            ));
        }
        DisplayItem::Rect(rect) => {
            let mut paint = String::new();
            if let Some(fill) = rect.fill {
                paint.push_str(&format!("background:{fill}"));
            }
            else if rect.filled {
                paint.push_str("background:#f2f2f2");
            }
            if let Some(stroke) = rect.stroke {
                paint.push_str(&format!(";border:{}pt solid {stroke}", num(rect.stroke_width)));
            }
            else if !rect.filled {
                paint.push_str("border:0.5pt solid #999");
            }
            out.push_str(&format!(
                "<div class=\"vutex-rect\" data-vutex-item=\"rect\" style=\"left:{}pt;top:{}pt;width:{}pt;height:{}pt;{}\"></div>\n",
                num(rect.x),
                num(rect.y),
                num(rect.width),
                num(rect.height),
                paint,
            ));
        }
        DisplayItem::Image(image) => {
            out.push_str(&format!(
                "<div class=\"vutex-image\" data-vutex-item=\"image\" role=\"img\" aria-label=\"{}\" style=\"left:{}pt;top:{}pt;width:{}pt;height:{}pt\">{}</div>\n",
                escape_attribute(&image.alt),
                num(image.x),
                num(image.y),
                num(image.width),
                num(image.height),
                escape_text(&image.alt),
            ));
        }
        DisplayItem::Graphic(graphic) => write_graphic(out, graphic)?,
        DisplayItem::Path(path) => write_path(out, path),
    }
    Ok(())
}

/// Embed one placed diagram scene as an inline SVG inside its layout box.
///
/// The scene serializes as a standalone `<svg>` document carrying its own
/// intrinsic size and `viewBox`; stretching it to the positioned box scales
/// that viewBox onto the layout geometry — the same placement the SVG backend
/// achieves with a translated, scaled group.  A scene that cannot serialize
/// fails the render instead of leaving a blank hole, matching the SVG
/// backend's policy.
fn write_graphic(out: &mut String, graphic: &DisplayGraphic) -> Result<(), Diagnostic> {
    let intrinsic = graphic.scene.size;
    if intrinsic.width <= 0.0 || intrinsic.height <= 0.0 {
        return Err(Diagnostic {
            code: "vutex.render.html.diagram-invalid".into(),
            severity: Severity::Error,
            message: format!("diagram `{}` has a non-positive canvas size", graphic.alt),
            span: None,
        });
    }
    let scene = graphic.scene.to_svg().map_err(|error| Diagnostic {
        code: "vutex.render.html.diagram-invalid".into(),
        severity: Severity::Error,
        message: format!("diagram `{}` is not serializable: {error}", graphic.alt),
        span: None,
    })?;
    let embedded = scene.replacen("<svg ", "<svg style=\"width:100%;height:100%;display:block\" ", 1);
    out.push_str(&format!(
        "<div class=\"vutex-graphic\" data-vutex-item=\"graphic\" data-diagram=\"{}\" role=\"img\" aria-label=\"{}\" style=\"left:{}pt;top:{}pt;width:{}pt;height:{}pt\">{}\n</div>\n",
        escape_attribute(&graphic.alt),
        escape_attribute(&graphic.alt),
        num(graphic.x),
        num(graphic.y),
        num(graphic.width),
        num(graphic.height),
        embedded,
    ));
    Ok(())
}

/// Write the text of a fixed snapshot run, wrapping it in its anchor.
///
/// The anchor sits inside the positioned text element so snapshot geometry is
/// unchanged. A destination that fails [`is_safe_href`] is dropped and the text
/// still renders as plain content; the link title is only written onto an
/// anchor that exists.
fn write_snapshot_text(out: &mut String, run: &DisplayTextRun) {
    let anchor = run.href.as_deref().filter(|href| is_safe_href(href));
    if let Some(href) = anchor {
        write_anchor_open(out, href, run.title.as_deref(), run.color);
    }
    out.push_str(&escape_text(&run.text));
    if anchor.is_some() {
        out.push_str("</a>");
    }
}

fn write_flow_block(
    out: &mut String,
    block: &FlowBlock,
    math: Option<FlowMathContext>,
    glyphs: &mut GlyphDefs,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match block {
        FlowBlock::Heading { level, runs } => {
            let level = (*level).clamp(1, 6);
            out.push_str(&format!("<h{level} data-vutex-block=\"heading\">"));
            for run in runs {
                write_flow_run(out, run, math, glyphs, diagnostics);
            }
            out.push_str(&format!("</h{level}>\n"));
        }
        FlowBlock::Paragraph { runs } => {
            out.push_str("<p data-vutex-block=\"paragraph\">");
            for run in runs {
                write_flow_run(out, run, math, glyphs, diagnostics);
            }
            out.push_str("</p>\n");
        }
        FlowBlock::List { ordered, items } => {
            let tag = if *ordered { "ol" } else { "ul" };
            out.push_str(&format!("<{tag} data-vutex-block=\"list\""));
            // The first authored number becomes the list start, so later items
            // only need a `value` where the authored run breaks.
            let first_number = items.first().and_then(|item| item.number);
            if *ordered
                && let Some(number) = first_number
                && number != 1
            {
                out.push_str(&format!(" start=\"{number}\""));
            }
            out.push_str(">\n");
            let mut count = first_number.unwrap_or(1);
            for item in items {
                out.push_str("<li");
                if *ordered {
                    if let Some(number) = item.number {
                        if number != count {
                            out.push_str(&format!(" value=\"{number}\""));
                        }
                        count = number.saturating_add(1);
                    }
                    else {
                        count = count.saturating_add(1);
                    }
                }
                out.push('>');
                for run in &item.runs {
                    write_flow_run(out, run, math, glyphs, diagnostics);
                }
                for child in &item.children {
                    write_flow_block(out, child, math, glyphs, diagnostics);
                }
                out.push_str("</li>\n");
            }
            out.push_str(&format!("</{tag}>\n"));
        }
        FlowBlock::CodeBlock { language, text } => {
            out.push_str("<pre data-vutex-block=\"code\"><code");
            if let Some(language) = language {
                out.push_str(&format!(" class=\"language-{}\"", escape_attribute(language)));
            }
            out.push_str(&format!(">{}</code></pre>\n", escape_text(text)));
        }
        FlowBlock::Math { formula } => match math.and_then(|context| formula_svg(formula, context, glyphs, diagnostics))
        {
            Some(svg) => write_display_math(out, &formula.source, &svg),
            // Without a MATH context, or when the engine rejects the formula,
            // the authored source stays visible as a code run: the same output
            // the reflow path produced before formulas were typeset.
            None => write_math_source_block(out, &formula.source),
        },
        FlowBlock::Quote { runs } => {
            out.push_str("<blockquote data-vutex-block=\"quote\"><p>");
            for run in runs {
                write_flow_run(out, run, math, glyphs, diagnostics);
            }
            out.push_str("</p></blockquote>\n");
        }
        FlowBlock::Table { caption, rows } => {
            out.push_str("<table data-vutex-block=\"table\">\n");
            if let Some(caption) = caption {
                out.push_str(&format!("<caption>{}</caption>\n", escape_text(caption)));
            }
            write_table_rows(out, "thead", math, glyphs, diagnostics, rows.iter().filter(|row| row.is_header));
            write_table_rows(out, "tbody", math, glyphs, diagnostics, rows.iter().filter(|row| !row.is_header));
            out.push_str("</table>\n");
        }
        FlowBlock::Image { alt, source } => {
            out.push_str(&format!(
                "<figure data-vutex-block=\"image\"><img src=\"{}\" alt=\"{}\">",
                escape_attribute(source),
                escape_attribute(alt),
            ));
            if !alt.is_empty() {
                out.push_str(&format!("<figcaption>{}</figcaption>", escape_text(alt)));
            }
            out.push_str("</figure>\n");
        }
        FlowBlock::Rule => out.push_str("<hr data-vutex-block=\"rule\">\n"),
    }
}

/// Typeset one formula into SVG, or record why it kept its source text.
///
/// Layout diagnostics and outline warnings travel back to the caller's report.
/// A rejected formula is a warning, not a failure: the source still reaches the
/// page, so no content disappears.
fn formula_svg(
    formula: &document_ir::MathFormula,
    math: FlowMathContext,
    glyphs: &mut GlyphDefs,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<FormulaSvg> {
    match layout_formula_for(formula, math.base_size, math.family) {
        Ok((result, _root, mut layout_diagnostics)) => {
            diagnostics.append(&mut layout_diagnostics);
            let svg = render_formula_result_with_glyph_defs(&result, glyphs);
            diagnostics.extend(svg.diagnostics.iter().cloned());
            Some(svg)
        }
        Err(error) => {
            diagnostics.push(Diagnostic {
                code: "vutex.render.html.math.not-typeset".into(),
                severity: Severity::Warning,
                message: format!("formula `{}` keeps its source text: {error}", formula.source),
                span: None,
            });
            None
        }
    }
}

/// Embed one typeset formula as an inline SVG box sitting on the text baseline.
///
/// The wrapper carries the formula's point size, and the viewport hangs
/// `height - baseline` below the baseline so surrounding text lines up with the
/// formula axis instead of its descender.
fn write_inline_math(out: &mut String, source: &str, svg: &FormulaSvg) {
    out.push_str(&format!(
        "<span class=\"vutex-math\" data-vutex-item=\"math\" role=\"img\" aria-label=\"{}\" style=\"display:inline-block;width:{}pt;height:{}pt;vertical-align:-{}pt\">",
        escape_attribute(source),
        num(svg.width),
        num(svg.height),
        num(svg.height - svg.baseline),
    ));
    out.push_str(&svg.markup.replacen("<svg ", "<svg style=\"width:100%;height:100%;display:block\" ", 1));
    out.push_str("</span>");
}

/// Embed one typeset display formula as its own line.
fn write_display_math(out: &mut String, source: &str, svg: &FormulaSvg) {
    out.push_str(&format!(
        "<div class=\"vutex-math\" data-vutex-block=\"math\" role=\"img\" aria-label=\"{}\">",
        escape_attribute(source),
    ));
    out.push_str(&svg.markup.replacen(
        "<svg ",
        &format!("<svg style=\"width:{}pt;height:{}pt\" ", num(svg.width), num(svg.height)),
        1,
    ));
    out.push_str("</div>\n");
}

/// Print a display formula as verbatim source when it is not typeset.
fn write_math_source_block(out: &mut String, source: &str) {
    out.push_str(&format!("<pre data-vutex-block=\"code\"><code>{}</code></pre>\n", escape_text(source)));
}

/// Write one flow run, wrapping it in the element that carries its emphasis.
///
/// A run destination nests outside the emphasis element, so authored bold text
/// inside a link renders as `<a href="…"><strong>…</strong></a>`. Runs without a
/// destination keep the plain output, and the link title only appears on an
/// anchor that exists. A math run typesets instead of printing source text.
fn write_flow_run(
    out: &mut String,
    run: &FlowRun,
    math: Option<FlowMathContext>,
    glyphs: &mut GlyphDefs,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let anchor = run.href.as_deref().filter(|href| is_safe_href(href));
    if let Some(href) = anchor {
        write_anchor_open(out, href, run.title.as_deref(), None);
    }
    if let Some(formula) = &run.math {
        match math.and_then(|context| formula_svg(formula, context, glyphs, diagnostics)) {
            Some(svg) => write_inline_math(out, &formula.source, &svg),
            None => out.push_str(&escape_text(&formula.source)),
        }
    }
    else {
        let tag = match run.style {
            FontStyle::Bold => Some("strong"),
            FontStyle::Italic => Some("em"),
            FontStyle::Monospace => Some("code"),
            FontStyle::Strike => Some("del"),
            _ => None,
        };
        match tag {
            Some(tag) => out.push_str(&format!("<{tag}>{}</{tag}>", escape_text(&run.text))),
            None => out.push_str(&escape_text(&run.text)),
        }
    }
    if anchor.is_some() {
        out.push_str("</a>");
    }
}

/// Open an anchor, adding the escaped tooltip only when the run carries one.
///
/// The class carries the underline, and the theme ink travels as an inline
/// colour so the browser default blue never reaches a VuTeX page.
fn write_anchor_open(out: &mut String, href: &str, title: Option<&str>, color: Option<&'static str>) {
    out.push_str(&format!("<a class=\"vutex-link\" href=\"{}\"", escape_attribute(href)));
    if let Some(title) = title {
        out.push_str(&format!(" title=\"{}\"", escape_attribute(title)));
    }
    if let Some(color) = color {
        out.push_str(&format!(" style=\"color:{color}\""));
    }
    out.push('>');
}

/// Inline CSS declarations for a styled run inside the fixed snapshot layout.
///
/// Snapshot geometry stays untouched; emphasis is layered on top so the
/// absolute positioning is byte-identical to the unstyled output.
fn style_declarations(style: FontStyle) -> &'static str {
    match style {
        FontStyle::Bold => ";font-weight:700",
        FontStyle::Italic => ";font-style:italic",
        _ => "",
    }
}

/// Render one vector glyph outline as an absolutely-positioned inline `<svg>`.
///
/// The path origin sits on the text baseline with y growing downward, so the
/// inline viewport is placed `height` points above the baseline (a negative
/// viewBox origin) and reaches `depth` points below it. The `<path>` `d`
/// attribute is copied verbatim; browsers natively understand SVG path
/// operators so no conversion is needed.
fn write_path(out: &mut String, path: &DisplayPath) {
    if path.d.is_empty() {
        return;
    }
    let vh = path.height + path.depth;
    out.push_str(&format!(
        "<svg class=\"vutex-path\" data-vutex-item=\"path\" style=\"position:absolute;left:{}pt;top:{}pt;width:{}pt;height:{}pt\" viewBox=\"0 {} {} {}\"><path d=\"{}\" fill=\"#111111\"/></svg>\n",
        num(path.x),
        num(path.y - path.height),
        num(path.width.max(0.001)),
        num(vh.max(0.001)),
        num(-path.height),
        num(path.width.max(0.001)),
        num(vh.max(0.001)),
        escape_attribute(&path.d),
    ));
}

fn write_table_rows<'a>(
    out: &mut String,
    section: &str,
    math: Option<FlowMathContext>,
    glyphs: &mut GlyphDefs,
    diagnostics: &mut Vec<Diagnostic>,
    rows: impl Iterator<Item = &'a FlowTableRow>,
) {
    let mut wrote_section = false;
    for row in rows {
        if !wrote_section {
            out.push_str(&format!("<{section}>\n"));
            wrote_section = true;
        }
        out.push_str("<tr>");
        let cell_tag = if row.is_header { "th" } else { "td" };
        for cell in &row.cells {
            out.push_str(&format!("<{cell_tag}>"));
            for run in cell {
                write_flow_run(out, run, math, glyphs, diagnostics);
            }
            out.push_str(&format!("</{cell_tag}>"));
        }
        out.push_str("</tr>\n");
    }
    if wrote_section {
        out.push_str(&format!("</{section}>\n"));
    }
}

fn is_valid_tag(tag: &str) -> bool {
    !tag.is_empty() && tag.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn num(value: f64) -> String {
    format!("{value:.3}")
}

fn escape_text(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_attribute(value: &str) -> String {
    escape_text(value).replace('"', "&quot;").replace('\'', "&#39;")
}
