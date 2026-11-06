//! Page geometry, body metrics, and the measurable style tokens behind them.

/// Where the running header text comes from.
///
/// The theme paints chrome from page two on. A template can override the source
/// without touching the theme: `Title` keeps the document-derived header,
/// `None` drops the header while keeping the folio, and `Literal` paints fixed
/// text the template owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeHeader {
    /// Document title, falling back to the first level-1 section title.
    Title,
    /// No running header.
    None,
    /// Fixed text supplied by a template.
    Literal(&'static str),
}

/// Template overrides for the page chrome the theme enables.
///
/// The theme decides whether chrome exists and how it looks. This hint only
/// chooses which pages carry it, where the header text comes from, and whether
/// a folio is painted, so a template never needs to restate theme tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChromeHint {
    /// Header text source.
    pub header: ChromeHeader,
    /// Whether pages carry a folio.
    pub folio: bool,
    /// 1-based page index where chrome starts; earlier pages stay bare.
    pub start_page: usize,
}

impl ChromeHint {
    /// Theme behaviour: document title and folio from page two on.
    pub const fn themed() -> Self {
        Self { header: ChromeHeader::Title, folio: true, start_page: 2 }
    }
}

impl Default for ChromeHint {
    fn default() -> Self {
        Self::themed()
    }
}

use document_text::math::MathFontFamily;

/// Horizontal alignment a theme asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Flush with the content column left edge.
    Left,
    /// Centred inside the content column.
    Center,
    /// Flush with the content column right edge.
    Right,
}

/// Measurable style tokens the layout pass paints from.
///
/// Every distance is in points and every colour is a `#RRGGBB` string, so a
/// theme can be asserted as numbers instead of compared as a screenshot.
/// [`StyleTokens::legacy`] reproduces the pre-theme defaults exactly, which
/// keeps [`LayoutHint::a4`] and every existing reference page unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StyleTokens {
    /// Vertical space after a paragraph, in points.
    pub paragraph_gap: f64,
    /// First-line paragraph indent, in points.
    pub paragraph_indent: f64,
    /// Heading size as a multiple of the body size, H1 to H4.
    pub heading_scales: [f64; 4],
    /// Space before a heading, in points, H1 to H4.
    pub heading_gap_before: [f64; 4],
    /// Space after a heading, in points, H1 to H4.
    pub heading_gap_after: [f64; 4],
    /// Whether heading runs are painted bold.
    pub heading_bold: bool,
    /// Thickness of the rule under an H1, in points, zero for none.
    pub heading_rule: f64,
    /// Colour of the rule under an H1.
    pub heading_rule_color: &'static str,
    /// Width of the H1 rule in points, zero for the full content column.
    pub heading_rule_width: f64,
    /// Horizontal alignment of heading text.
    pub heading_align: Align,
    /// Thickness of the table header rule, in points.
    pub table_header_rule: f64,
    /// Thickness of a table row rule, in points, zero for none.
    pub table_row_rule: f64,
    /// Thickness of the closing table rule, in points, zero for none.
    pub table_final_rule: f64,
    /// Colour of the table header and closing rules.
    pub table_rule_strong_color: &'static str,
    /// Colour of the table row rules.
    pub table_rule_color: &'static str,
    /// Header row fill, or `None` for paper white.
    pub table_header_tint: Option<&'static str>,
    /// Horizontal cell padding, in points.
    pub table_pad_h: f64,
    /// Vertical cell padding, in points.
    pub table_pad_v: f64,
    /// Code text size in points.
    pub code_size: f64,
    /// Code line box as a multiple of the code size.
    pub code_line_height: f64,
    /// Code inner padding, in points.
    pub code_padding: f64,
    /// Space above and below a code block, in points.
    pub code_margin: f64,
    /// Code block fill colour.
    pub code_fill: &'static str,
    /// Code block border width in points, zero for none.
    pub code_border: f64,
    /// Code block border colour.
    pub code_border_color: &'static str,
    /// Quote bar width in points, zero for no bar.
    pub quote_rule: f64,
    /// Quote bar indent, in points.
    pub quote_indent: f64,
    /// Quote text colour.
    pub quote_text_color: &'static str,
    /// Quote bar colour.
    pub quote_rule_color: &'static str,
    /// Unordered list marker glyph.
    pub list_marker: &'static str,
    /// Space between the marker and the item text, in points.
    pub list_marker_gap: f64,
    /// Space between list items, in points.
    pub list_item_gap: f64,
    /// Space before a list item, in points.
    pub list_gap_before: f64,
    /// Left indent of a list, in points.
    pub list_indent: f64,
    /// Primary ink colour.
    pub ink: &'static str,
    /// Secondary ink colour for quotes and captions.
    pub ink_secondary: &'static str,
    /// Muted ink colour for chrome.
    pub ink_muted: &'static str,
    /// Faint ink colour for disabled or decorative marks.
    pub ink_faint: &'static str,
    /// Accent colour used by links.
    pub accent: &'static str,
    /// Strong rule colour.
    pub rule_strong: &'static str,
    /// Regular rule colour.
    pub rule: &'static str,
    /// Faint rule colour.
    pub rule_faint: &'static str,
    /// Light tint behind code and header cells.
    pub tint: &'static str,
    /// Space before a footnote body, in points.
    pub footnote_gap: f64,
    /// Footnote separator thickness, in points, zero for none.
    pub footnote_rule: f64,
    /// Image maximum width in points, zero for the full content column.
    pub image_max_width: f64,
    /// Whether pages carry a running header and folio.
    pub chrome: bool,
    /// Running header text size in points.
    pub header_size: f64,
    /// Running header colour.
    pub header_color: &'static str,
    /// Running header alignment.
    pub header_align: Align,
    /// Folio text size in points.
    pub folio_size: f64,
    /// Folio colour.
    pub folio_color: &'static str,
    /// Distance from the page top to the running header baseline.
    pub chrome_top: f64,
    /// Distance from the page bottom to the folio baseline.
    pub chrome_bottom: f64,
}

impl StyleTokens {
    /// Default theme tokens: the zero-configuration face of every document.
    ///
    /// Values follow the modern column of the theme baseline: 8 pt paragraph
    /// rhythm, a 700 heading ladder with a light rule under the H1, a three-rule
    /// table, bordered code blocks, one muted accent for links, and a running
    /// header plus folio from page two on.
    pub const fn modern() -> Self {
        Self {
            paragraph_gap: 8.0,
            paragraph_indent: 0.0,
            heading_scales: [1.60, 1.35, 1.18, 1.10],
            heading_gap_before: [0.0, 24.0, 16.0, 12.0],
            heading_gap_after: [20.0, 8.0, 4.0, 4.0],
            heading_bold: true,
            heading_rule: 0.75,
            heading_rule_color: "#C7C7C7",
            heading_rule_width: 0.0,
            heading_align: Align::Left,
            table_header_rule: 0.75,
            table_row_rule: 0.5,
            table_final_rule: 0.75,
            table_rule_strong_color: "#C7C7C7",
            table_rule_color: "#EEEEEC",
            table_header_tint: None,
            table_pad_h: 8.0,
            table_pad_v: 4.0,
            code_size: 10.0,
            code_line_height: 1.45,
            code_padding: 12.0,
            code_margin: 12.0,
            code_fill: "#F7F7F4",
            code_border: 0.5,
            code_border_color: "#E3E3E0",
            quote_rule: 2.5,
            quote_indent: 12.0,
            quote_text_color: "#4A4A4A",
            quote_rule_color: "#C7C7C7",
            list_marker: "–",
            list_marker_gap: 8.0,
            list_item_gap: 4.0,
            list_gap_before: 4.0,
            list_indent: 20.0,
            ink: "#1A1A1A",
            ink_secondary: "#4A4A4A",
            ink_muted: "#737373",
            ink_faint: "#9A9A9A",
            accent: "#2B5B84",
            rule_strong: "#C7C7C7",
            rule: "#E3E3E0",
            rule_faint: "#EEEEEC",
            tint: "#F7F7F4",
            footnote_gap: 12.0,
            footnote_rule: 0.5,
            image_max_width: 0.0,
            chrome: true,
            header_size: 8.5,
            header_color: "#737373",
            header_align: Align::Right,
            folio_size: 9.0,
            folio_color: "#737373",
            chrome_top: 40.0,
            chrome_bottom: 48.0,
        }
    }

    /// Editorial theme tokens: magazine and column voice, explicit choice only.
    ///
    /// A larger H1 with a short brick-red rule, warm tints behind table headers
    /// and code, and a terracotta accent. Never selected by default.
    pub const fn editorial() -> Self {
        Self {
            paragraph_gap: 8.0,
            paragraph_indent: 0.0,
            heading_scales: [2.00, 1.50, 1.25, 1.12],
            heading_gap_before: [0.0, 24.0, 16.0, 12.0],
            heading_gap_after: [20.0, 8.0, 4.0, 4.0],
            heading_bold: true,
            heading_rule: 2.0,
            heading_rule_color: "#B3402F",
            heading_rule_width: 56.0,
            heading_align: Align::Left,
            table_header_rule: 0.75,
            table_row_rule: 0.5,
            table_final_rule: 0.75,
            table_rule_strong_color: "#C9C2B6",
            table_rule_color: "#EDE8DF",
            table_header_tint: Some("#F5F1E9"),
            table_pad_h: 8.0,
            table_pad_v: 4.0,
            code_size: 10.0,
            code_line_height: 1.45,
            code_padding: 12.0,
            code_margin: 12.0,
            code_fill: "#F5F1E9",
            code_border: 0.5,
            code_border_color: "#E0DACF",
            quote_rule: 2.5,
            quote_indent: 12.0,
            quote_text_color: "#45403A",
            quote_rule_color: "#C9C2B6",
            list_marker: "–",
            list_marker_gap: 8.0,
            list_item_gap: 4.0,
            list_gap_before: 4.0,
            list_indent: 20.0,
            ink: "#1C1A18",
            ink_secondary: "#45403A",
            ink_muted: "#7C756C",
            ink_faint: "#A79F94",
            accent: "#B3402F",
            rule_strong: "#C9C2B6",
            rule: "#E0DACF",
            rule_faint: "#EDE8DF",
            tint: "#F5F1E9",
            footnote_gap: 12.0,
            footnote_rule: 0.5,
            image_max_width: 0.0,
            chrome: true,
            header_size: 8.5,
            header_color: "#7C756C",
            header_align: Align::Left,
            folio_size: 9.0,
            folio_color: "#7C756C",
            chrome_top: 40.0,
            chrome_bottom: 48.0,
        }
    }

    /// Classic theme tokens: Chinese book conventions, explicit choice only.
    ///
    /// Centre-aligned headings over a hairline, first-line indent instead of
    /// paragraph gaps, a bare three-rule table with no row rules, no coloured
    /// accent, and grey ink only. Never selected by default.
    pub const fn classic() -> Self {
        Self {
            paragraph_gap: 0.0,
            paragraph_indent: 21.0,
            heading_scales: [1.60, 1.30, 1.20, 1.12],
            heading_gap_before: [0.0, 24.0, 16.0, 12.0],
            heading_gap_after: [20.0, 8.0, 4.0, 4.0],
            heading_bold: true,
            heading_rule: 0.5,
            heading_rule_color: "#444444",
            heading_rule_width: 0.0,
            heading_align: Align::Center,
            table_header_rule: 0.75,
            table_row_rule: 0.0,
            table_final_rule: 0.75,
            table_rule_strong_color: "#444444",
            table_rule_color: "#444444",
            table_header_tint: None,
            table_pad_h: 8.0,
            table_pad_v: 4.0,
            code_size: 10.0,
            code_line_height: 1.45,
            code_padding: 12.0,
            code_margin: 12.0,
            code_fill: "#F4F4F2",
            code_border: 0.0,
            code_border_color: "#999999",
            quote_rule: 0.0,
            quote_indent: 21.0,
            quote_text_color: "#2B2B2B",
            quote_rule_color: "#444444",
            list_marker: "–",
            list_marker_gap: 8.0,
            list_item_gap: 4.0,
            list_gap_before: 4.0,
            list_indent: 21.0,
            ink: "#111111",
            ink_secondary: "#2B2B2B",
            ink_muted: "#555555",
            ink_faint: "#8A8A8A",
            accent: "#111111",
            rule_strong: "#444444",
            rule: "#BBBBBB",
            rule_faint: "#DDDDDD",
            tint: "#F4F4F2",
            footnote_gap: 12.0,
            footnote_rule: 0.5,
            image_max_width: 0.0,
            chrome: true,
            header_size: 8.5,
            header_color: "#555555",
            header_align: Align::Center,
            folio_size: 9.0,
            folio_color: "#555555",
            chrome_top: 40.0,
            chrome_bottom: 48.0,
        }
    }

    /// Legacy tokens: the pre-theme defaults, now the explicit traditional
    /// theme. Kept byte-identical so pages laid out before themes keep their
    /// pagination.
    pub const fn legacy() -> Self {
        Self {
            paragraph_gap: 0.0,
            paragraph_indent: 0.0,
            heading_scales: [1.8, 1.45, 1.25, 1.1],
            heading_gap_before: [11.0, 11.0, 8.8, 8.8],
            heading_gap_after: [4.4, 4.4, 4.4, 4.4],
            heading_bold: false,
            heading_rule: 0.0,
            heading_rule_color: "#111111",
            heading_rule_width: 0.0,
            heading_align: Align::Left,
            table_header_rule: 1.0,
            table_row_rule: 0.4,
            table_final_rule: 0.4,
            table_rule_strong_color: "#111111",
            table_rule_color: "#111111",
            table_header_tint: None,
            table_pad_h: 3.85,
            table_pad_v: 3.85,
            code_size: 10.12,
            code_line_height: 1.425,
            code_padding: 6.07,
            code_margin: 4.05,
            code_fill: "#F2F2F2",
            code_border: 0.0,
            code_border_color: "#999999",
            quote_rule: 2.0,
            quote_indent: 16.0,
            quote_text_color: "#111111",
            quote_rule_color: "#111111",
            list_marker: "•",
            list_marker_gap: 6.0,
            list_item_gap: 2.2,
            list_gap_before: 1.65,
            list_indent: 18.0,
            ink: "#111111",
            ink_secondary: "#111111",
            ink_muted: "#111111",
            ink_faint: "#111111",
            accent: "#111111",
            rule_strong: "#111111",
            rule: "#111111",
            rule_faint: "#111111",
            tint: "#F2F2F2",
            footnote_gap: 8.8,
            footnote_rule: 0.0,
            image_max_width: 240.0,
            chrome: false,
            header_size: 8.5,
            header_color: "#111111",
            header_align: Align::Right,
            folio_size: 9.0,
            folio_color: "#111111",
            chrome_top: 40.0,
            chrome_bottom: 48.0,
        }
    }
}

impl Default for StyleTokens {
    fn default() -> Self {
        Self::modern()
    }
}

/// A4 portrait page width in points (210 mm).
pub const A4_WIDTH: f64 = 595.276;
/// A4 portrait page height in points (297 mm).
pub const A4_HEIGHT: f64 = 841.89;
/// Default inward margin in points (1 inch = 72 pt).
pub const DEFAULT_MARGIN: f64 = 72.0;

/// Inward page margins in points, measured from each page edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margin {
    /// Top margin in points.
    pub top: f64,
    /// Right margin in points.
    pub right: f64,
    /// Bottom margin in points.
    pub bottom: f64,
    /// Left margin in points.
    pub left: f64,
}

impl Margin {
    /// Same margin on all four sides.
    pub const fn uniform(value: f64) -> Self {
        Self { top: value, right: value, bottom: value, left: value }
    }
}

/// Layout intent expressed by source languages or style rules.
///
/// The hint fixes page geometry and body metrics; the document layout pass
/// fills pages inside the margins. [`Default`] is the modern baseline: A4
/// portrait, 88/100/96/100 pt margins, 11 pt body text at a 1.72 line height,
/// and [`StyleTokens::modern`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutHint {
    /// Page width in points.
    pub page_width: f64,
    /// Page height in points.
    pub page_height: f64,
    /// Inward page margins.
    pub margin: Margin,
    /// Body text size in points; headings scale from it.
    pub base_font_size: f64,
    /// Line box height as a multiple of the font size.
    pub line_height: f64,
    /// Measurable style tokens painted by the layout pass.
    pub style: StyleTokens,
    /// OpenType MATH family every formula in the pass lays out with.
    ///
    /// `None` keeps the process-wide host default installed through
    /// `document_text::math::install_math_font`, resolved by
    /// [`MathFontFamily::builtin_default`]. A host that lets a document pick a
    /// math face passes the family explicitly, so the choice travels with the
    /// layout instead of with process startup order.
    pub math_font: Option<MathFontFamily>,
    /// Page chrome overrides a template asks for.
    pub chrome: ChromeHint,
    /// Heading levels (H1 to H4) that start a new page.
    ///
    /// A template uses this to make level-1 sections open new pages. The
    /// force-break never adds a blank page before the first heading on a page
    /// that is already empty, so a document that starts with an H1 keeps its
    /// first page.
    pub section_breaks: [bool; 4],
}

impl Default for LayoutHint {
    fn default() -> Self {
        Self::a4()
    }
}

impl LayoutHint {
    /// A4 portrait page at the modern baseline: 88/100/96/100 pt margins,
    /// 11 pt body text, a 1.72 line height, and modern style tokens.
    ///
    /// This is the zero-configuration face of a document. The legacy layout
    /// defaults survive as [`StyleTokens::legacy`], the explicit traditional
    /// theme, which hosts pair with 72 pt margins and a tighter line height.
    pub const fn a4() -> Self {
        Self {
            page_width: A4_WIDTH,
            page_height: A4_HEIGHT,
            margin: Margin { top: 88.0, right: 100.0, bottom: 96.0, left: 100.0 },
            base_font_size: 11.0,
            line_height: 1.72,
            style: StyleTokens::modern(),
            math_font: None,
            chrome: ChromeHint::themed(),
            section_breaks: [false; 4],
        }
    }

    /// Same hint with a different page size, keeping margins and body metrics.
    pub const fn with_page_size(mut self, width: f64, height: f64) -> Self {
        self.page_width = width;
        self.page_height = height;
        self
    }

    /// Same hint with different inward margins.
    pub const fn with_margin(mut self, margin: Margin) -> Self {
        self.margin = margin;
        self
    }

    /// Usable width between the left and right margins, never negative.
    pub fn content_width(&self) -> f64 {
        (self.page_width - self.margin.left - self.margin.right).max(0.0)
    }

    /// Usable height between the top and bottom margins, never negative.
    pub fn content_height(&self) -> f64 {
        (self.page_height - self.margin.top - self.margin.bottom).max(0.0)
    }

    /// Bottom edge of the content area measured from the page top, in points.
    pub fn content_bottom(&self) -> f64 {
        self.page_height - self.margin.bottom
    }
}
