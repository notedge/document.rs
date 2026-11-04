#![doc = include_str!("readme.md")]

//! TikZ 前端：`tikzpicture` 源直接降到 [`DisplayScene`]。
//!
//! 接受的子集见 crate 文档。子集外的命令、选项或字符一律以
//! `davinci.diagram.unsupported` 与行内字节范围拒绝，不猜几何、不降级成近似图形。

use plotter_diagram::DiagramError;
use plotter_scene::{
    Color, DisplayScene, NodeKind, PathCommand, PathData, Point, SceneNode, ShapeStyle, Size, TextAnchor, TextStyle, Transform,
};

/// Every lowered TikZ picture is registered under this alternative text.
pub const TIKZ_ALT: &str = "TikZ picture";

/// Scene units per centimetre.
const CM: f64 = 72.0 / 2.54;
/// Scene units per millimetre.
const MM: f64 = 72.0 / 25.4;
/// Scene units per typographic point.
const PT: f64 = 1.0;

/// Margin added around the fitted picture bounds, in scene units.
const PICTURE_MARGIN: f64 = 2.0;
/// Default node label font size, in scene units.
const DEFAULT_FONT_SIZE: f64 = 12.0;
/// Default stroke width, in scene units.
const DEFAULT_LINE_WIDTH: f64 = 0.4;
/// `thin` stroke width, in scene units.
const THIN_WIDTH: f64 = 0.4;
/// `thick` stroke width, in scene units.
const THICK_WIDTH: f64 = 0.8;
/// `very thick` stroke width, in scene units.
const VERY_THICK_WIDTH: f64 = 1.2;
/// Length of one painted dash piece, in scene units.
const DASH_ON: f64 = 4.0;
/// Length of one gap between dash pieces, in scene units.
const DASH_OFF: f64 = 3.0;
/// Gap between a node point and an anchored label, in scene units.
const NODE_GAP: f64 = 2.0;
/// Line segments used to sample a circle before dashing it.
const CIRCLE_SEGMENTS: usize = 72;
/// Control-point offset that turns a quarter ellipse into a cubic Bézier.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// Lower one TikZ picture source into a display scene.
///
/// The source may be a `tikzpicture` environment or its bare body. The picture
/// is parsed, its geometry is converted to scene points, and the canvas is
/// fitted to the painted extent plus [`PICTURE_MARGIN`]. A source outside the
/// supported subset returns `davinci.diagram.unsupported` with an inline span
/// rather than inventing geometry.
pub fn tikz_to_scene(source: &str) -> Result<DisplayScene, DiagramError> {
    scene_from_source(source).map_err(|error| error.into_diagram_error(source))
}

/// A lowering failure carrying a byte offset into the picture source.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TikzError {
    offset: usize,
    detail: String,
}

impl TikzError {
    /// A malformed-source error at a byte offset.
    fn syntax(offset: usize, detail: impl Into<String>) -> Self {
        Self { offset, detail: detail.into() }
    }

    /// An unsupported-construct error at a byte offset.
    fn unsupported(offset: usize, detail: impl Into<String>) -> Self {
        Self { offset, detail: detail.into() }
    }

    /// Convert to the shared frontend error, keeping the authored line in the span.
    fn into_diagram_error(self, source: &str) -> DiagramError {
        let offset = self.offset.min(source.len());
        let end = source[offset..].find('\n').map(|delta| offset + delta).unwrap_or(source.len());
        DiagramError::unsupported(self.detail).with_span(offset..end)
    }
}

/// Tokenise and parse one picture, then fit its painted bounds into a canvas.
fn scene_from_source(source: &str) -> Result<DisplayScene, TikzError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser { tokens, cursor: 0, scale: 1.0, nodes: Vec::new() };
    parser.parse_picture()?;
    fit_scene(parser.nodes)
}

/// Wrap the painted nodes in a root group translated so the picture bounds
/// start at the margin, and size the canvas to the fitted extent.
fn fit_scene(nodes: Vec<SceneNode>) -> Result<DisplayScene, TikzError> {
    if nodes.is_empty() {
        return Err(TikzError::unsupported(0, "the TikZ picture paints no geometry"));
    }
    let Some(bounds) = SceneNode::group(nodes.clone()).bounds()
    else {
        return Err(TikzError::unsupported(0, "the TikZ picture paints no geometry"));
    };
    let width = bounds.width() + PICTURE_MARGIN * 2.0;
    let height = bounds.height() + PICTURE_MARGIN * 2.0;
    let root = SceneNode::group(nodes)
        .with_transform(Transform::translate(PICTURE_MARGIN - bounds.min.x, PICTURE_MARGIN - bounds.min.y));
    let mut scene = DisplayScene::new(Size::new(width, height));
    scene.push(root);
    Ok(scene)
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

/// One lexed TikZ token with its byte offset in the picture source.
#[derive(Debug, Clone, PartialEq)]
struct Token {
    kind: TokenKind,
    offset: usize,
}

/// Token classes the supported grammar needs.
#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    /// A backslash command name such as `draw`.
    Command(String),
    /// A braced run, used for environment names and node labels.
    Brace(String),
    /// A bare word such as a colour name or a unit.
    Word(String),
    /// A numeric literal.
    Number(f64),
    /// `[`
    BracketOpen,
    /// `]`
    BracketClose,
    /// `(`
    ParenOpen,
    /// `)`
    ParenClose,
    /// `,`
    Comma,
    /// `=`
    Equals,
    /// `;`
    Semicolon,
    /// `--`
    DashDash,
}

/// Split a picture source into tokens, rejecting characters outside the subset.
fn tokenize(source: &str) -> Result<Vec<Token>, TikzError> {
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while let Some(ch) = source[index..].chars().next() {
        let offset = index;
        match ch {
            _ if ch.is_whitespace() => index += ch.len_utf8(),
            '%' => {
                index += 1;
                while let Some(next) = source[index..].chars().next() {
                    index += next.len_utf8();
                    if next == '\n' {
                        break;
                    }
                }
            }
            '\\' => {
                let mut cursor = index + 1;
                let mut name = String::new();
                while let Some(next) = source[cursor..].chars().next() {
                    if next.is_ascii_alphabetic() {
                        name.push(next);
                        cursor += next.len_utf8();
                    }
                    else {
                        break;
                    }
                }
                if name.is_empty() {
                    return Err(TikzError::syntax(offset, "expected a command name after `\\`"));
                }
                tokens.push(Token { kind: TokenKind::Command(name), offset });
                index = cursor;
            }
            '{' => {
                let mut cursor = index + 1;
                let mut depth = 1usize;
                let mut text = String::new();
                loop {
                    let Some(next) = source[cursor..].chars().next()
                    else {
                        return Err(TikzError::syntax(offset, "unbalanced `{` reached the end of the picture"));
                    };
                    cursor += next.len_utf8();
                    match next {
                        '{' => {
                            depth += 1;
                            text.push(next);
                        }
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            text.push(next);
                        }
                        _ => text.push(next),
                    }
                }
                tokens.push(Token { kind: TokenKind::Brace(text.trim().to_string()), offset });
                index = cursor;
            }
            '}' => return Err(TikzError::syntax(offset, "unmatched `}`")),
            '[' => {
                tokens.push(Token { kind: TokenKind::BracketOpen, offset });
                index += 1;
            }
            ']' => {
                tokens.push(Token { kind: TokenKind::BracketClose, offset });
                index += 1;
            }
            '(' => {
                tokens.push(Token { kind: TokenKind::ParenOpen, offset });
                index += 1;
            }
            ')' => {
                tokens.push(Token { kind: TokenKind::ParenClose, offset });
                index += 1;
            }
            ',' => {
                tokens.push(Token { kind: TokenKind::Comma, offset });
                index += 1;
            }
            '=' => {
                tokens.push(Token { kind: TokenKind::Equals, offset });
                index += 1;
            }
            ';' => {
                tokens.push(Token { kind: TokenKind::Semicolon, offset });
                index += 1;
            }
            '-' => {
                if source[index..].starts_with("--") {
                    tokens.push(Token { kind: TokenKind::DashDash, offset });
                    index += 2;
                }
                else if source[index + 1..].chars().next().is_some_and(|next| next.is_ascii_digit() || next == '.') {
                    let (value, cursor) = scan_number(source, index)?;
                    tokens.push(Token { kind: TokenKind::Number(value), offset });
                    index = cursor;
                }
                else {
                    return Err(TikzError::syntax(offset, "unexpected `-`, expected `--` or a negative number"));
                }
            }
            '.' => {
                if source[index + 1..].chars().next().is_some_and(|next| next.is_ascii_digit()) {
                    let (value, cursor) = scan_number(source, index)?;
                    tokens.push(Token { kind: TokenKind::Number(value), offset });
                    index = cursor;
                }
                else {
                    return Err(TikzError::syntax(offset, "unexpected `.` in a TikZ picture"));
                }
            }
            _ if ch.is_ascii_digit() => {
                let (value, cursor) = scan_number(source, index)?;
                tokens.push(Token { kind: TokenKind::Number(value), offset });
                index = cursor;
            }
            _ if ch.is_ascii_alphabetic() => {
                let mut cursor = index + ch.len_utf8();
                let mut word = String::from(ch);
                while let Some(next) = source[cursor..].chars().next() {
                    if next.is_ascii_alphabetic() {
                        word.push(next);
                        cursor += next.len_utf8();
                    }
                    else {
                        break;
                    }
                }
                tokens.push(Token { kind: TokenKind::Word(word), offset });
                index = cursor;
            }
            _ => return Err(TikzError::syntax(offset, format!("unsupported character `{ch}` in a TikZ picture"))),
        }
    }
    Ok(tokens)
}

/// Scan an optionally negative decimal number starting at `start`.
fn scan_number(source: &str, start: usize) -> Result<(f64, usize), TikzError> {
    let mut cursor = start;
    if source[cursor..].starts_with('-') {
        cursor += 1;
    }
    let mut seen_dot = false;
    let mut digits = 0usize;
    while let Some(ch) = source[cursor..].chars().next() {
        if ch.is_ascii_digit() {
            digits += 1;
            cursor += 1;
        }
        else if ch == '.' && !seen_dot {
            seen_dot = true;
            cursor += 1;
        }
        else {
            break;
        }
    }
    if digits == 0 {
        return Err(TikzError::syntax(start, "expected a number"));
    }
    let text = &source[start..cursor];
    let value = text.parse::<f64>().map_err(|_| TikzError::syntax(start, format!("invalid number `{text}`")))?;
    Ok((value, cursor))
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Picture options gathered from one `[...]` list.
struct OptionSpec {
    key: String,
    value: Option<OptionValue>,
    offset: usize,
}

/// Value carried by one parsed option.
enum OptionValue {
    /// A bare word such as a colour name.
    Word(String),
    /// A number with an optional unit suffix.
    Number(f64, Option<String>),
}

/// One drawing command.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    /// Stroke only.
    Draw,
    /// Fill only.
    Fill,
    /// Fill and stroke.
    FillDraw,
}

/// State waiting for the next coordinate after `--` or `rectangle`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// `--`: the next coordinate continues the current polyline.
    Line,
    /// `rectangle`: the next coordinate is the opposite corner.
    Rectangle,
}

/// One closed or open outline collected from a path expression.
enum Contour {
    /// Straight segments through the listed points.
    Polyline { points: Vec<Point>, closed: bool },
    /// A circle sampled as four cubic segments or as dashes.
    Circle { center: Point, radius: f64 },
}

/// Anchoring of a node label relative to its point.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Anchor {
    /// Centre the label on the point.
    Center,
    /// Place the label above the point.
    Above,
    /// Place the label below the point.
    Below,
    /// Place the label to the left of the point.
    Left,
    /// Place the label to the right of the point.
    Right,
}

/// Paint attributes accumulated from one option list.
#[derive(Clone, Copy)]
struct Style {
    color: Color,
    draw: Option<Color>,
    fill: Option<Color>,
    text: Option<Color>,
    line_width: f64,
    dashed: bool,
    opacity: f64,
    fill_opacity: f64,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            draw: None,
            fill: None,
            text: None,
            line_width: DEFAULT_LINE_WIDTH,
            dashed: false,
            opacity: 1.0,
            fill_opacity: 1.0,
        }
    }
}

impl Style {
    /// Resolve the paint attributes of one drawing command.
    fn shape_style(self, command: Command) -> ShapeStyle {
        let color = self.color;
        let (fill, stroke) = match command {
            Command::Draw => (self.fill, Some(self.draw.unwrap_or(color))),
            Command::Fill => (Some(self.fill.unwrap_or(color)), None),
            Command::FillDraw => (Some(self.fill.unwrap_or(color)), Some(self.draw.unwrap_or(color))),
        };
        let fill_only = stroke.is_none();
        // A fill-only path can carry fill opacity in the shape alpha, which both
        // backends honour. A filled and stroked path has one alpha for the whole
        // shape, so the fill opacity rides on the fill colour instead.
        let fill = fill.map(|colour| if fill_only { colour } else { apply_alpha(colour, self.fill_opacity) });
        let opacity = if fill_only { self.opacity * self.fill_opacity } else { self.opacity };
        ShapeStyle { fill, stroke, stroke_width: self.line_width, opacity: opacity.clamp(0.0, 1.0) }
    }
}

/// Node-specific options layered on top of the enclosing path style.
#[derive(Clone, Copy)]
struct NodeOptions {
    anchor: Anchor,
    color: Option<Color>,
    text: Option<Color>,
    scale: f64,
    opacity: Option<f64>,
}

impl Default for NodeOptions {
    fn default() -> Self {
        Self { anchor: Anchor::Center, color: None, text: None, scale: 1.0, opacity: None }
    }
}

/// Recursive-descent parser over the token stream.
struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    scale: f64,
    nodes: Vec<SceneNode>,
}

impl Parser {
    /// Parse one picture, accepting an environment wrapper or a bare body.
    fn parse_picture(&mut self) -> Result<(), TikzError> {
        if self.check_command("begin") {
            self.cursor += 1;
            let offset = self.offset();
            match self.next_kind() {
                Some(TokenKind::Brace(name)) if name.trim().eq_ignore_ascii_case("tikzpicture") => {}
                Some(TokenKind::Brace(name)) => {
                    return Err(TikzError::unsupported(offset, format!("unsupported TeX environment `{name}`")));
                }
                _ => return Err(TikzError::syntax(offset, "expected `{tikzpicture}` after `\\begin`")),
            }
            self.parse_picture_options()?;
            loop {
                match self.peek_kind() {
                    None => return Err(TikzError::syntax(self.offset(), "unterminated `tikzpicture` environment")),
                    Some(TokenKind::Command(name)) if name == "end" => {
                        self.cursor += 1;
                        let offset = self.offset();
                        match self.next_kind() {
                            Some(TokenKind::Brace(name)) if name.trim().eq_ignore_ascii_case("tikzpicture") => break,
                            _ => return Err(TikzError::syntax(offset, "expected `{tikzpicture}` after `\\end`")),
                        }
                    }
                    _ => self.parse_statement()?,
                }
            }
            if let Some(kind) = self.peek_kind() {
                return Err(TikzError::unsupported(self.offset(), format!("content after `\\end{{tikzpicture}}`: {kind:?}")));
            }
        }
        else {
            while self.peek_kind().is_some() {
                if self.check_command("end") {
                    return Err(TikzError::syntax(self.offset(), "unexpected `\\end` without `\\begin`"));
                }
                self.parse_statement()?;
            }
        }
        Ok(())
    }

    /// Apply picture-wide options. Only `scale` is part of the subset.
    fn parse_picture_options(&mut self) -> Result<(), TikzError> {
        if !matches!(self.peek_kind(), Some(TokenKind::BracketOpen)) {
            return Ok(());
        }
        for option in self.parse_options()? {
            match (option.key.as_str(), option.value.as_ref()) {
                ("scale", Some(OptionValue::Number(value, None))) if *value > 0.0 => self.scale = *value,
                _ => {
                    return Err(TikzError::unsupported(option.offset, format!("unsupported picture option `{}`", option.key)));
                }
            }
        }
        Ok(())
    }

    /// Parse one drawing command and its path expression.
    fn parse_statement(&mut self) -> Result<(), TikzError> {
        let offset = self.offset();
        let Some(TokenKind::Command(name)) = self.next_kind()
        else {
            return Err(TikzError::syntax(offset, "expected a path command such as `\\draw`"));
        };
        let command = match name.as_str() {
            "draw" => Command::Draw,
            "fill" => Command::Fill,
            "filldraw" => Command::FillDraw,
            other => {
                return Err(TikzError::unsupported(offset, format!("unsupported TikZ command `\\{other}`")));
            }
        };
        let style = self.parse_path_options()?;
        let nodes = self.parse_path(command, style)?;
        self.nodes.extend(nodes);
        Ok(())
    }

    /// Parse the option list that follows a drawing command.
    fn parse_path_options(&mut self) -> Result<Style, TikzError> {
        let mut style = Style::default();
        if matches!(self.peek_kind(), Some(TokenKind::BracketOpen)) {
            for option in self.parse_options()? {
                apply_shape_option(&mut style, &option)?;
            }
        }
        Ok(style)
    }

    /// Parse a `[...]` list into independent key/value options.
    fn parse_options(&mut self) -> Result<Vec<OptionSpec>, TikzError> {
        self.cursor += 1;
        let mut options = Vec::new();
        loop {
            match self.peek_kind() {
                None => return Err(TikzError::syntax(self.offset(), "unterminated option list")),
                Some(TokenKind::BracketClose) => {
                    self.cursor += 1;
                    break;
                }
                _ => {}
            }
            let offset = self.offset();
            let mut words = Vec::new();
            while let Some(TokenKind::Word(word)) = self.peek_kind() {
                words.push(word.clone());
                self.cursor += 1;
            }
            if words.is_empty() {
                return Err(TikzError::syntax(offset, "expected an option name"));
            }
            let key = words.join(" ");
            let value = if matches!(self.peek_kind(), Some(TokenKind::Equals)) {
                self.cursor += 1;
                Some(self.parse_option_value()?)
            }
            else {
                None
            };
            options.push(OptionSpec { key, value, offset });
            match self.peek_kind() {
                Some(TokenKind::Comma) => self.cursor += 1,
                Some(TokenKind::BracketClose) => {
                    self.cursor += 1;
                    break;
                }
                _ => return Err(TikzError::syntax(self.offset(), "expected `,` or `]` after an option")),
            }
        }
        Ok(options)
    }

    /// Parse one option value: a word, or a number with an optional unit.
    fn parse_option_value(&mut self) -> Result<OptionValue, TikzError> {
        let offset = self.offset();
        match self.next_kind() {
            Some(TokenKind::Word(word)) => Ok(OptionValue::Word(word)),
            Some(TokenKind::Number(value)) => {
                let unit = match self.peek_kind() {
                    Some(TokenKind::Word(word)) if is_length_unit(word) => {
                        let word = word.clone();
                        self.cursor += 1;
                        Some(word)
                    }
                    _ => None,
                };
                Ok(OptionValue::Number(value, unit))
            }
            _ => Err(TikzError::syntax(offset, "expected an option value")),
        }
    }

    /// Parse a drawing path up to its terminating `;`.
    fn parse_path(&mut self, command: Command, style: Style) -> Result<Vec<SceneNode>, TikzError> {
        let mut contours = Vec::new();
        let mut texts = Vec::new();
        let mut current: Option<Point> = None;
        let mut pending: Option<Pending> = None;
        // Index of the polyline that `--` still extends. A leading single-point
        // contour is tracked too, so a shape operator can replace it instead of
        // painting a stray move-to.
        let mut open: Option<usize> = None;
        loop {
            let offset = self.offset();
            match self.peek_kind().cloned() {
                None => return Err(TikzError::syntax(offset, "path command is missing its `;`")),
                Some(TokenKind::Semicolon) => {
                    self.cursor += 1;
                    break;
                }
                Some(TokenKind::DashDash) => {
                    self.cursor += 1;
                    if current.is_none() {
                        return Err(TikzError::syntax(offset, "`--` needs a preceding coordinate"));
                    }
                    if pending.is_some() {
                        return Err(TikzError::syntax(offset, "consecutive path operators"));
                    }
                    pending = Some(Pending::Line);
                }
                Some(TokenKind::ParenOpen) => {
                    let point = self.parse_coordinate()?;
                    match pending.take() {
                        None => {
                            if current.is_some() {
                                return Err(TikzError::syntax(offset, "a coordinate needs `--` or a shape operator before it"));
                            }
                            contours.push(Contour::Polyline { points: vec![point], closed: false });
                            open = Some(contours.len() - 1);
                            current = Some(point);
                        }
                        Some(Pending::Line) => {
                            let start = current.unwrap_or(point);
                            match open {
                                Some(index) => {
                                    if let Some(Contour::Polyline { points, .. }) = contours.get_mut(index) {
                                        points.push(point);
                                    }
                                }
                                None => {
                                    contours.push(Contour::Polyline { points: vec![start, point], closed: false });
                                    open = Some(contours.len() - 1);
                                }
                            }
                            current = Some(point);
                        }
                        Some(Pending::Rectangle) => {
                            let start = current.unwrap_or(point);
                            drop_leading_point(&mut contours, open.take(), start);
                            contours.push(Contour::Polyline {
                                points: vec![start, Point::new(point.x, start.y), point, Point::new(start.x, point.y)],
                                closed: true,
                            });
                            current = Some(point);
                        }
                    }
                }
                Some(TokenKind::Word(word)) if word.eq_ignore_ascii_case("rectangle") => {
                    self.cursor += 1;
                    if current.is_none() {
                        return Err(TikzError::syntax(offset, "`rectangle` needs a preceding coordinate"));
                    }
                    if pending.is_some() {
                        return Err(TikzError::syntax(offset, "consecutive path operators"));
                    }
                    pending = Some(Pending::Rectangle);
                }
                Some(TokenKind::Word(word)) if word.eq_ignore_ascii_case("circle") => {
                    self.cursor += 1;
                    let center = current.ok_or_else(|| TikzError::syntax(offset, "`circle` needs a centre coordinate"))?;
                    if pending.is_some() {
                        return Err(TikzError::syntax(offset, "consecutive path operators"));
                    }
                    let radius = self.parse_circle_radius()?;
                    drop_leading_point(&mut contours, open.take(), center);
                    contours.push(Contour::Circle { center, radius });
                }
                Some(TokenKind::Word(word)) if word.eq_ignore_ascii_case("node") => {
                    self.cursor += 1;
                    let point = current.ok_or_else(|| TikzError::syntax(offset, "`node` needs a current point"))?;
                    texts.push(self.parse_node(point, style)?);
                }
                Some(other) => {
                    return Err(TikzError::syntax(offset, format!("unexpected {other:?} in a path expression")));
                }
            }
        }
        if pending.is_some() {
            return Err(TikzError::syntax(self.offset(), "path operator without an endpoint"));
        }
        let mut nodes = Vec::new();
        if !contours.is_empty() {
            nodes
                .push(SceneNode::new(NodeKind::Path { path: build_path(&contours, style), style: style.shape_style(command) }));
        }
        nodes.extend(texts);
        Ok(nodes)
    }

    /// Parse `(x,y)` with bare centimetres and `cm`/`mm`/`pt` suffixes.
    fn parse_coordinate(&mut self) -> Result<Point, TikzError> {
        self.cursor += 1;
        let x = self.parse_length("a coordinate")?;
        self.expect_kind(TokenKind::Comma, "expected `,` between the two coordinate values")?;
        let y = self.parse_length("a coordinate")?;
        self.expect_kind(TokenKind::ParenClose, "expected `)` closing the coordinate")?;
        Ok(Point::new(x, y))
    }

    /// Parse one coordinate or radius length and convert it to scene units.
    fn parse_length(&mut self, context: &str) -> Result<f64, TikzError> {
        let offset = self.offset();
        let Some(TokenKind::Number(value)) = self.next_kind()
        else {
            return Err(TikzError::syntax(offset, format!("expected {context}")));
        };
        let unit = match self.peek_kind() {
            Some(TokenKind::Word(word)) if is_length_unit(word) => {
                let word = word.clone();
                self.cursor += 1;
                Some(word)
            }
            _ => None,
        };
        Ok(points_for(value, unit.as_deref(), CM, self.scale))
    }

    /// Parse `(r)` after `circle`.
    fn parse_circle_radius(&mut self) -> Result<f64, TikzError> {
        let offset = self.offset();
        self.expect_kind(TokenKind::ParenOpen, "expected `(` before a circle radius")?;
        let radius = self.parse_length("a circle radius")?;
        self.expect_kind(TokenKind::ParenClose, "expected `)` after the circle radius")?;
        if !(radius > 0.0) {
            return Err(TikzError::syntax(offset, "circle radius must be positive"));
        }
        Ok(radius)
    }

    /// Parse `node[options] {text}` at `point`.
    fn parse_node(&mut self, point: Point, path_style: Style) -> Result<SceneNode, TikzError> {
        let options = self.parse_node_options()?;
        let offset = self.offset();
        let Some(TokenKind::Brace(text)) = self.next_kind()
        else {
            return Err(TikzError::syntax(offset, "node is missing its `{text}` label"));
        };
        if text.is_empty() {
            return Err(TikzError::syntax(offset, "node label is empty"));
        }
        let font_size = DEFAULT_FONT_SIZE * options.scale;
        let (position, text_anchor) = node_placement(point, options.anchor, font_size);
        let fill = options.text.or(options.color).or(path_style.text).unwrap_or(path_style.color);
        let style = TextStyle {
            fill: Some(fill),
            opacity: options.opacity.unwrap_or(path_style.opacity).clamp(0.0, 1.0),
            font_size,
            text_anchor,
            ..TextStyle::default()
        };
        Ok(SceneNode::new(NodeKind::Text { position, text, style }))
    }

    /// Parse the option list attached to a node label.
    fn parse_node_options(&mut self) -> Result<NodeOptions, TikzError> {
        let mut options = NodeOptions::default();
        if matches!(self.peek_kind(), Some(TokenKind::BracketOpen)) {
            for option in self.parse_options()? {
                apply_node_option(&mut options, &option)?;
            }
        }
        Ok(options)
    }

    /// The kind of the next token without consuming it.
    fn peek_kind(&self) -> Option<&TokenKind> {
        self.tokens.get(self.cursor).map(|token| &token.kind)
    }

    /// Consume and return the next token kind.
    fn next_kind(&mut self) -> Option<TokenKind> {
        let token = self.tokens.get(self.cursor)?;
        let kind = token.kind.clone();
        self.cursor += 1;
        Some(kind)
    }

    /// Byte offset of the next token, or of the last token at end of input.
    fn offset(&self) -> usize {
        self.tokens.get(self.cursor).or_else(|| self.tokens.last()).map(|token| token.offset).unwrap_or(0)
    }

    /// Consume one token of `expected` kind, or report `detail`.
    fn expect_kind(&mut self, expected: TokenKind, detail: &str) -> Result<(), TikzError> {
        let offset = self.offset();
        match self.next_kind() {
            Some(kind) if kind == expected => Ok(()),
            _ => Err(TikzError::syntax(offset, detail)),
        }
    }

    /// Whether the next token is the given command name.
    fn check_command(&self, expected: &str) -> bool {
        matches!(self.peek_kind(), Some(TokenKind::Command(name)) if name == expected)
    }
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Apply one option to a path style.
fn apply_shape_option(style: &mut Style, option: &OptionSpec) -> Result<(), TikzError> {
    match (option.key.as_str(), option.value.as_ref()) {
        ("dashed", None) => style.dashed = true,
        ("thin", None) => style.line_width = THIN_WIDTH,
        ("thick", None) => style.line_width = THICK_WIDTH,
        ("very thick", None) => style.line_width = VERY_THICK_WIDTH,
        ("fill", None) => style.fill = Some(style.color),
        ("line width", Some(OptionValue::Number(value, unit))) => {
            style.line_width = points_for(*value, unit.as_deref(), PT, 1.0);
            if !(style.line_width > 0.0) {
                return Err(TikzError::syntax(option.offset, "line width must be positive"));
            }
        }
        ("color", Some(OptionValue::Word(name))) => style.color = named_color(name, option.offset)?,
        ("draw", Some(OptionValue::Word(name))) => style.draw = Some(named_color(name, option.offset)?),
        ("fill", Some(OptionValue::Word(name))) => style.fill = Some(named_color(name, option.offset)?),
        ("text", Some(OptionValue::Word(name))) => style.text = Some(named_color(name, option.offset)?),
        ("opacity", Some(OptionValue::Number(value, None))) => style.opacity = unit_opacity(*value, option.offset)?,
        ("fill opacity", Some(OptionValue::Number(value, None))) => {
            style.fill_opacity = unit_opacity(*value, option.offset)?;
        }
        (key, None) => style.color = named_color(key, option.offset)?,
        _ => {
            return Err(TikzError::unsupported(option.offset, format!("unsupported TikZ option `{}`", option.key)));
        }
    }
    Ok(())
}

/// Apply one option to node label options.
fn apply_node_option(options: &mut NodeOptions, option: &OptionSpec) -> Result<(), TikzError> {
    match (option.key.as_str(), option.value.as_ref()) {
        ("above", None) => options.anchor = Anchor::Above,
        ("below", None) => options.anchor = Anchor::Below,
        ("left", None) => options.anchor = Anchor::Left,
        ("right", None) => options.anchor = Anchor::Right,
        ("color", Some(OptionValue::Word(name))) => options.color = Some(named_color(name, option.offset)?),
        ("text", Some(OptionValue::Word(name))) => options.text = Some(named_color(name, option.offset)?),
        ("scale", Some(OptionValue::Number(value, None))) => {
            if !(*value > 0.0) {
                return Err(TikzError::syntax(option.offset, "node scale must be positive"));
            }
            options.scale = *value;
        }
        ("opacity", Some(OptionValue::Number(value, None))) => {
            options.opacity = Some(unit_opacity(*value, option.offset)?);
        }
        (key, None) => options.color = Some(named_color(key, option.offset)?),
        _ => {
            return Err(TikzError::unsupported(option.offset, format!("unsupported node option `{}`", option.key)));
        }
    }
    Ok(())
}

/// Resolve the supported colour names.
fn named_color(name: &str, offset: usize) -> Result<Color, TikzError> {
    match name.trim().to_ascii_lowercase().as_str() {
        "black" => Ok(Color::BLACK),
        "red" => Ok(Color::rgb(255, 0, 0)),
        "blue" => Ok(Color::rgb(0, 0, 255)),
        "green" => Ok(Color::rgb(0, 255, 0)),
        other => Err(TikzError::unsupported(offset, format!("unknown TikZ colour `{other}`"))),
    }
}

/// Validate an opacity factor and keep it inside the scene's supported range.
fn unit_opacity(value: f64, offset: usize) -> Result<f64, TikzError> {
    if !(0.0..=1.0).contains(&value) {
        return Err(TikzError::syntax(offset, format!("opacity `{value}` must be within 0..=1")));
    }
    Ok(value)
}

/// Whether a word names a supported length unit.
fn is_length_unit(word: &str) -> bool {
    matches!(word.trim().to_ascii_lowercase().as_str(), "cm" | "mm" | "pt")
}

/// Convert one authored length to scene units.
///
/// Bare numbers keep `default_factor`, which is centimetres for coordinates and
/// points for line widths, matching the two TikZ dimensions this subset reads.
fn points_for(value: f64, unit: Option<&str>, default_factor: f64, scale: f64) -> f64 {
    let factor = match unit.map(str::trim) {
        Some(unit) if unit.eq_ignore_ascii_case("cm") => CM,
        Some(unit) if unit.eq_ignore_ascii_case("mm") => MM,
        Some(unit) if unit.eq_ignore_ascii_case("pt") => PT,
        _ => default_factor,
    };
    value * factor * scale
}

/// Multiply a colour's alpha by `factor`, rounding to the nearest channel value.
fn apply_alpha(color: Color, factor: f64) -> Color {
    let alpha = (f64::from(color.alpha()) * factor).round().clamp(0.0, 255.0) as u8;
    color.with_alpha(alpha)
}

// ---------------------------------------------------------------------------
// Scene geometry
// ---------------------------------------------------------------------------

/// Place a node label so its text box touches `point` on the anchored side.
///
/// The baseline offsets follow the scene crate's text bounds approximation,
/// which reserves `0.8` of the font size above and `0.2` below the baseline.
fn node_placement(point: Point, anchor: Anchor, font_size: f64) -> (Point, TextAnchor) {
    match anchor {
        Anchor::Center => (Point::new(point.x, point.y + font_size * 0.3), TextAnchor::Middle),
        Anchor::Above => (Point::new(point.x, point.y - font_size * 0.2), TextAnchor::Middle),
        Anchor::Below => (Point::new(point.x, point.y + font_size * 0.8), TextAnchor::Middle),
        Anchor::Left => (Point::new(point.x - NODE_GAP, point.y + font_size * 0.3), TextAnchor::End),
        Anchor::Right => (Point::new(point.x + NODE_GAP, point.y + font_size * 0.3), TextAnchor::Start),
    }
}

/// Drop the leading single-point contour when a shape operator replaces it.
fn drop_leading_point(contours: &mut Vec<Contour>, open: Option<usize>, start: Point) {
    let Some(index) = open
    else {
        return;
    };
    let replaceable = matches!(contours.get(index), Some(Contour::Polyline { points, closed: false }) if points.len() == 1 && points[0] == start);
    if replaceable {
        contours.pop();
    }
}

/// Turn the collected contours into one path, dashing them when asked.
fn build_path(contours: &[Contour], style: Style) -> PathData {
    let mut path = PathData::new();
    for contour in contours {
        match contour {
            Contour::Circle { center, radius } if style.dashed => {
                let points = circle_points(*center, *radius);
                append_dashed_contour(&mut path, &points, true);
            }
            Contour::Circle { center, radius } => append_circle(&mut path, *center, *radius),
            Contour::Polyline { points, closed } if style.dashed => append_dashed_contour(&mut path, points, *closed),
            Contour::Polyline { points, closed } => {
                let Some(first) = points.first()
                else {
                    continue;
                };
                path.commands.push(PathCommand::MoveTo(*first));
                for point in &points[1..] {
                    path.commands.push(PathCommand::LineTo(*point));
                }
                if *closed {
                    path.commands.push(PathCommand::Close);
                }
            }
        }
    }
    path
}

/// Append a circle as a move plus four cubic quarter arcs.
fn append_circle(path: &mut PathData, center: Point, radius: f64) {
    let (cx, cy) = (center.x, center.y);
    let k = radius * KAPPA;
    path.commands.push(PathCommand::MoveTo(Point::new(cx + radius, cy)));
    path.commands.push(PathCommand::CubicTo {
        control1: Point::new(cx + radius, cy + k),
        control2: Point::new(cx + k, cy + radius),
        to: Point::new(cx, cy + radius),
    });
    path.commands.push(PathCommand::CubicTo {
        control1: Point::new(cx - k, cy + radius),
        control2: Point::new(cx - radius, cy + k),
        to: Point::new(cx - radius, cy),
    });
    path.commands.push(PathCommand::CubicTo {
        control1: Point::new(cx - radius, cy - k),
        control2: Point::new(cx - k, cy - radius),
        to: Point::new(cx, cy - radius),
    });
    path.commands.push(PathCommand::CubicTo {
        control1: Point::new(cx + k, cy - radius),
        control2: Point::new(cx + radius, cy - k),
        to: Point::new(cx + radius, cy),
    });
    path.commands.push(PathCommand::Close);
}

/// Sample a circle as a closed polyline so it can be dashed.
fn circle_points(center: Point, radius: f64) -> Vec<Point> {
    (0..=CIRCLE_SEGMENTS)
        .map(|index| {
            let angle = std::f64::consts::TAU * index as f64 / CIRCLE_SEGMENTS as f64;
            Point::new(center.x + radius * angle.cos(), center.y + radius * angle.sin())
        })
        .collect()
}

/// Append a contour as alternating dash pieces in one subpath sequence.
fn append_dashed_contour(path: &mut PathData, points: &[Point], closed: bool) {
    if points.len() < 2 {
        return;
    }
    let mut points = points.to_vec();
    if closed {
        points.push(points[0]);
    }
    let mut on = true;
    let mut remaining = DASH_ON;
    let mut pen_down = false;
    for pair in points.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let length = start.distance(end);
        if length <= f64::EPSILON {
            continue;
        }
        let direction = ((end.x - start.x) / length, (end.y - start.y) / length);
        let mut consumed = 0.0f64;
        while length - consumed > f64::EPSILON {
            if on && !pen_down {
                path.commands
                    .push(PathCommand::MoveTo(Point::new(start.x + direction.0 * consumed, start.y + direction.1 * consumed)));
                pen_down = true;
            }
            let step = remaining.min(length - consumed);
            consumed += step;
            remaining -= step;
            if on {
                path.commands
                    .push(PathCommand::LineTo(Point::new(start.x + direction.0 * consumed, start.y + direction.1 * consumed)));
            }
            if remaining <= f64::EPSILON {
                pen_down = false;
                on = !on;
                remaining = if on { DASH_ON } else { DASH_OFF };
            }
        }
    }
}
