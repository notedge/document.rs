//! Render a [`FormulaLayoutResult`] into SVG by walking its [`PaintNode`] tree.
//!
//! Every backend consumes the same immutable [`FormulaLayoutResult`]; this
//! module is the SVG side of that contract.  The paint tree already carries
//! the canvas-space geometry the box-tree renderer used to compute while
//! walking, so the traversal here only maps primitives onto SVG elements and
//! tracks the painted ink for the viewport.

use std::fmt::Write;

use document_compose::{
    Diagnostic, Severity, SourceSpan,
    math::{Fixed, FormulaLayoutResult, PaintNode},
};

/// Output of [`render_formula_result`]: the SVG document plus the warnings
/// collected while walking the paint tree (glyphs the face ships no outline
/// for).
#[derive(Debug, Clone)]
pub struct FormulaSvg {
    pub markup: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Render a fully laid-out formula as an SVG document.
///
/// The canvas is the formula box from [`FormulaLayoutResult::metrics`]:
/// origin at the box top-left, baseline at `y = metrics.height`.  The viewport
/// is the union of that box and the ink actually painted, padded slightly so
/// scaled outlines are never clipped.
///
/// The backend paints outlines only, matching the document path: a glyph the
/// face ships no outline for is skipped and named in a returned
/// `vutex.layout.math.outline_missing` warning, never pulled in through a
/// host-font `<text>` run and never a hard render failure.  Callers that gate
/// on completeness assert the returned diagnostics are empty.
pub fn render_formula_result(result: &FormulaLayoutResult) -> FormulaSvg {
    let metrics = &result.metrics;
    let mut walk = SvgWalk { buffer: String::new(), ink: None, diagnostics: Vec::new() };
    walk.walk(&result.paint, 0.0, 0.0);
    let width = metrics.width.to_f64();
    let height = (metrics.height + metrics.depth).to_f64();
    let diagnostics = std::mem::take(&mut walk.diagnostics);
    FormulaSvg { markup: walk.finish(width, height), diagnostics }
}

/// Axis-aligned ink bounds in canvas pixels, with y growing downward.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    fn include_point(&mut self, x: f64, y: f64) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    fn include_bounds(&mut self, other: &Bounds) {
        self.include_point(other.min_x, other.min_y);
        self.include_point(other.max_x, other.max_y);
    }
}

struct SvgWalk {
    buffer: String,
    /// Bounds of everything painted so far, in canvas pixels.
    ink: Option<Bounds>,
    diagnostics: Vec<Diagnostic>,
}

impl SvgWalk {
    fn walk(&mut self, node: &PaintNode, ox: f64, oy: f64) {
        match node {
            PaintNode::Group { children, translate_x, translate_y } => {
                let nx = ox + translate_x.to_f64();
                let ny = oy + translate_y.to_f64();
                let pre = self.buffer.len();
                let _ = write!(self.buffer, r#"<g transform="translate({}, {})">"#, translate_x.to_f64(), translate_y.to_f64());
                let after_open = self.buffer.len();
                for child in children {
                    self.walk(child, nx, ny);
                }
                if self.buffer.len() == after_open {
                    // The whole subtree painted nothing: drop the group
                    // instead of leaving an empty `<g>` in the document.
                    self.buffer.truncate(pre);
                }
                else {
                    self.buffer.push_str("</g>");
                }
            }
            PaintNode::GlyphRun { glyph_ids: _, chars, family, positions, size, path_data, font_id: _, spans } => {
                let scale = size.to_f64();
                for (i, d_opt) in path_data.iter().enumerate() {
                    let (px, py) = positions.get(i).copied().unwrap_or((Fixed::ZERO, Fixed::ZERO));
                    let (rx, ry) = (px.to_f64(), py.to_f64());
                    let Some(d) = d_opt
                    else {
                        // No outline: the entry is reported whether or not it
                        // paints, matching the document collector this mirrors.
                        let span = spans.get(i).copied().flatten();
                        self.diagnostics.push(outline_missing(chars.get(i).copied().flatten(), family, span));
                        continue;
                    };
                    self.path(d, ox + rx, oy + ry, rx, ry, scale);
                }
            }
            PaintNode::Rect { x, y, width, height, stroke_width, baseline: _ } => {
                let rx = x.to_f64();
                let ry = y.to_f64();
                let rw = width.to_f64();
                let rh = height.to_f64();
                // A stroke is drawn centred on the box edge, so half of it
                // paints outside: the tracked ink must cover the overhang or
                // the viewport clips it.
                let half = (stroke_width.to_f64() / 2.0).max(0.0);
                let rect = Bounds {
                    min_x: ox + rx - half,
                    min_y: oy + ry - half,
                    max_x: ox + rx + rw + half,
                    max_y: oy + ry + rh + half,
                };
                self.ink.get_or_insert(rect).include_bounds(&rect);
                if stroke_width.to_f64() > 0.0 {
                    let _ = write!(
                        self.buffer,
                        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="currentColor" stroke="currentColor" stroke-width="{}" />"#,
                        rx,
                        ry,
                        rw,
                        rh,
                        stroke_width.to_f64(),
                    );
                }
                else {
                    let _ = write!(
                        self.buffer,
                        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="currentColor" />"#,
                        rx, ry, rw, rh
                    );
                }
            }
            PaintNode::VectorPath { d, x, y, scale, bbox: _bbox } => {
                let rx = x.to_f64();
                let ry = y.to_f64();
                let s = scale.to_f64();
                self.path(d, ox + rx, oy + ry, rx, ry, s);
            }
        }
    }

    /// Emit one outline path under `translate(x, y) scale(s, -s)`, mapping the
    /// y-up unit space onto the y-down canvas.  `(ax, ay)` is the absolute
    /// anchor for ink tracking, `(rx, ry)` the group-relative one for markup.
    fn path(&mut self, d: &str, ax: f64, ay: f64, rx: f64, ry: f64, scale: f64) {
        let mut ink = self.ink;
        visit_path_points(d, |px, py| {
            let wx = ax + scale * px;
            let wy = ay - scale * py;
            let bounds = ink.get_or_insert(Bounds { min_x: wx, min_y: wy, max_x: wx, max_y: wy });
            bounds.include_point(wx, wy);
        });
        self.ink = ink;
        let _ = write!(
            self.buffer,
            r#"<path d="{}" transform="translate({}, {}) scale({}, {})" fill="currentColor" />"#,
            d,
            num(rx),
            num(ry),
            num(scale),
            num(-scale),
        );
    }

    /// Close the document, widening the viewport to cover the painted ink.
    fn finish(self, width: f64, height: f64) -> String {
        let mut bounds = Bounds { min_x: 0.0, min_y: 0.0, max_x: width, max_y: height };
        if let Some(ink) = self.ink {
            bounds.include_bounds(&ink);
        }
        let span = (bounds.max_x - bounds.min_x).max(bounds.max_y - bounds.min_y).max(1.0);
        let pad = (span * 0.04).max(0.5);
        let min_x = bounds.min_x - pad;
        let min_y = bounds.min_y - pad;
        let view_w = (bounds.max_x - bounds.min_x) + 2.0 * pad;
        let view_h = (bounds.max_y - bounds.min_y) + 2.0 * pad;
        let mut svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="{} {} {} {}">"#,
            num(view_w),
            num(view_h),
            num(min_x),
            num(min_y),
            num(view_w),
            num(view_h),
        );
        svg.push_str(&self.buffer);
        svg.push_str("</svg>");
        svg
    }
}

/// Structure diagnostic for a glyph the math font ships no outline for.
fn outline_missing(ch: Option<char>, family: &str, span: Option<SourceSpan>) -> Diagnostic {
    let name = match ch {
        Some(ch) => format!("'{ch}' (U+{:04X})", ch as u32),
        None => "without a character".to_string(),
    };
    Diagnostic {
        code: "vutex.layout.math.outline_missing".into(),
        severity: Severity::Warning,
        message: format!(
            "math glyph {name} in family `{family}` has no embedded outline and was not painted; the box keeps its measured advance rather than falling back to a host font"
        ),
        span,
    }
}

/// Format a canvas coordinate for the SVG output.
fn num(value: f64) -> String {
    format!("{value:.4}")
}

/// Visit every on-curve/off-curve point of an absolute SVG path.
///
/// Layout only emits absolute `M`, `L`, `Q`, `C` and `Z`. Control points are
/// visited too, which keeps the tracked ink bounds a superset of the outline.
fn visit_path_points(d: &str, mut visit: impl FnMut(f64, f64)) {
    let mut op = 'M';
    let mut args: Vec<f64> = Vec::new();
    let mut chars = d.chars().peekable();
    loop {
        let Some(c) = chars.peek().copied()
        else {
            break;
        };
        if c.is_ascii_alphabetic() {
            emit_points(op, &args, &mut visit);
            args.clear();
            op = c;
            chars.next();
        }
        else if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
            let mut token = String::new();
            if c == '-' || c == '+' {
                token.push(c);
                chars.next();
            }
            while let Some(&next) = chars.peek() {
                let exponent_sign = matches!(next, '-' | '+') && matches!(token.chars().last(), Some('e' | 'E'));
                if next.is_ascii_digit() || next == '.' || next == 'e' || next == 'E' || exponent_sign {
                    token.push(next);
                    chars.next();
                }
                else {
                    break;
                }
            }
            if let Ok(value) = token.parse::<f64>() {
                args.push(value);
            }
        }
        else {
            chars.next();
        }
    }
    emit_points(op, &args, &mut visit);
}

/// Feed one operator's coordinate pairs to `visit`.
fn emit_points(op: char, args: &[f64], visit: &mut impl FnMut(f64, f64)) {
    let arity = match op {
        'M' | 'L' | 'T' => 2,
        'Q' | 'S' => 4,
        'C' => 6,
        'A' => 7,
        _ => return,
    };
    for chunk in args.chunks_exact(arity) {
        match op {
            'M' | 'L' | 'T' => visit(chunk[0], chunk[1]),
            'Q' | 'S' => {
                visit(chunk[0], chunk[1]);
                visit(chunk[2], chunk[3]);
            }
            'C' => {
                visit(chunk[0], chunk[1]);
                visit(chunk[2], chunk[3]);
                visit(chunk[4], chunk[5]);
            }
            'A' => visit(chunk[5], chunk[6]),
            _ => {}
        }
    }
}
