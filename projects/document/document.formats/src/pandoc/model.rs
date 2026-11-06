//! Typed Pandoc AST model.
//!
//! These types mirror the `pandoc-types` constructors one-to-one. They are
//! intentionally independent from `notedown-ir`: the wire model can represent
//! every Pandoc node, while the IR adapter decides which nodes have a semantic
//! projection.

/// First Pandoc API family accepted by the codec.
pub const API_VERSION: [i64; 3] = [1, 23, 1];

/// A decoded Pandoc JSON document.
#[derive(Debug, Clone, PartialEq)]
pub struct PandocDocument {
    /// `pandoc-api-version` triple.
    pub api_version: [i64; 3],
    /// Document metadata in authored key order.
    pub meta: Vec<(String, MetaValue)>,
    /// Top-level block sequence.
    pub blocks: Vec<Block>,
}

impl PandocDocument {
    /// Empty document carrying the accepted API version.
    pub fn new() -> Self {
        Self { api_version: API_VERSION, meta: Vec::new(), blocks: Vec::new() }
    }
}

impl Default for PandocDocument {
    fn default() -> Self {
        Self::new()
    }
}

/// One Pandoc metadata value.
#[derive(Debug, Clone, PartialEq)]
pub enum MetaValue {
    /// Nested metadata map (`MetaMap`).
    Map(Vec<(String, MetaValue)>),
    /// Metadata list (`MetaList`).
    List(Vec<MetaValue>),
    /// Boolean metadata (`MetaBool`).
    Bool(bool),
    /// Plain string metadata (`MetaString`).
    String(String),
    /// Inline metadata (`MetaInlines`).
    Inlines(Vec<Inline>),
    /// Block metadata (`MetaBlocks`).
    Blocks(Vec<Block>),
}

/// Pandoc attribute triple: identifier, classes, ordered key/value pairs.
///
/// Pair order and duplicate keys are preserved; converting to a map would
/// silently drop authored attributes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attr {
    /// Identifier string, empty when absent.
    pub id: String,
    /// Class names in authored order.
    pub classes: Vec<String>,
    /// Ordered key/value pairs, including duplicate keys.
    pub pairs: Vec<(String, String)>,
}

impl Attr {
    /// Empty attribute triple.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether the attribute carries no identifier, class, or pair.
    pub fn is_empty(&self) -> bool {
        self.id.is_empty() && self.classes.is_empty() && self.pairs.is_empty()
    }
}

/// One Pandoc block constructor.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// `Plain`: inline content without paragraph spacing.
    Plain(Vec<Inline>),
    /// `Para`: a paragraph.
    Para(Vec<Inline>),
    /// `LineBlock`: a list of lines, each a separate inline sequence.
    LineBlock(Vec<Vec<Inline>>),
    /// `CodeBlock`: attributes plus verbatim text.
    CodeBlock(Attr, String),
    /// `RawBlock`: format tag plus verbatim payload.
    RawBlock(String, String),
    /// `BlockQuote`: nested blocks.
    BlockQuote(Vec<Block>),
    /// `OrderedList`: numbering parameters plus items.
    OrderedList(OrderedList),
    /// `BulletList`: items.
    BulletList(Vec<Vec<Block>>),
    /// `DefinitionList`: term and its definitions.
    DefinitionList(Vec<(Vec<Inline>, Vec<Vec<Block>>)>),
    /// `Header`: level, attributes, and title inlines.
    Header {
        /// Heading level as authored.
        level: i64,
        /// Heading attributes.
        attr: Attr,
        /// Heading title inlines.
        inlines: Vec<Inline>,
    },
    /// `HorizontalRule`.
    HorizontalRule,
    /// `Table`.
    Table(Table),
    /// `Figure`: attributes, caption, and body blocks.
    Figure {
        /// Figure attributes.
        attr: Attr,
        /// Figure caption.
        caption: Caption,
        /// Figure body blocks.
        blocks: Vec<Block>,
    },
    /// `Div`: attributes and nested blocks.
    Div {
        /// Container attributes.
        attr: Attr,
        /// Container body blocks.
        blocks: Vec<Block>,
    },
}

/// `OrderedList` numbering parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderedList {
    /// First item number.
    pub start: i64,
    /// Numbering style.
    pub style: ListNumberStyle,
    /// Number delimiter.
    pub delimiter: ListNumberDelim,
    /// Items, each a block sequence.
    pub items: Vec<Vec<Block>>,
}

/// Ordered-list numbering style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListNumberStyle {
    /// `DefaultStyle`.
    Default,
    /// `Example`.
    Example,
    /// `Decimal`.
    Decimal,
    /// `LowerAlpha`.
    LowerAlpha,
    /// `UpperAlpha`.
    UpperAlpha,
    /// `LowerRoman`.
    LowerRoman,
    /// `UpperRoman`.
    UpperRoman,
}

/// Ordered-list number delimiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListNumberDelim {
    /// `DefaultDelim`.
    Default,
    /// `Period`.
    Period,
    /// `OneParen`.
    OneParen,
    /// `TwoParens`.
    TwoParens,
}

/// `Table` payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Table attributes.
    pub attr: Attr,
    /// Caption.
    pub caption: Caption,
    /// Column alignments and widths.
    pub colspecs: Vec<ColSpec>,
    /// Header rows.
    pub head: TableHead,
    /// Body sections.
    pub bodies: Vec<TableBody>,
    /// Footer rows.
    pub foot: TableFoot,
}

/// Table caption with an optional short form.
#[derive(Debug, Clone, PartialEq)]
pub struct Caption {
    /// Optional short caption inlines.
    pub short: Option<Vec<Inline>>,
    /// Caption body blocks.
    pub blocks: Vec<Block>,
}

impl Caption {
    /// Empty caption.
    pub fn empty() -> Self {
        Self { short: None, blocks: Vec::new() }
    }

    /// Whether both the short caption and the body are empty.
    pub fn is_empty(&self) -> bool {
        self.short.is_none() && self.blocks.is_empty()
    }
}

/// One table column specification.
#[derive(Debug, Clone, PartialEq)]
pub struct ColSpec {
    /// Column alignment.
    pub alignment: Alignment,
    /// Relative column width, `0.0` when unspecified.
    pub width: f64,
}

/// Table cell alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    /// `AlignLeft`.
    Left,
    /// `AlignRight`.
    Right,
    /// `AlignCenter`.
    Center,
    /// `AlignDefault`.
    Default,
}

/// Table head.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableHead {
    /// Head attributes.
    pub attr: Attr,
    /// Header rows.
    pub rows: Vec<Row>,
}

/// Table body section.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableBody {
    /// Body attributes.
    pub attr: Attr,
    /// Number of leading body columns repeated as row headers.
    pub row_head_columns: i64,
    /// Intermediate header rows.
    pub head: Vec<Row>,
    /// Body rows.
    pub body: Vec<Row>,
}

/// Table foot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableFoot {
    /// Foot attributes.
    pub attr: Attr,
    /// Footer rows.
    pub rows: Vec<Row>,
}

/// One table row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    /// Row attributes.
    pub attr: Attr,
    /// Row cells.
    pub cells: Vec<Cell>,
}

/// One table cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// Cell attributes.
    pub attr: Attr,
    /// Cell alignment.
    pub alignment: Alignment,
    /// Row span.
    pub row_span: i64,
    /// Column span.
    pub col_span: i64,
    /// Cell body blocks.
    pub blocks: Vec<Block>,
}

/// One Pandoc inline constructor.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// `Str`: literal text.
    Str(String),
    /// `Emph`.
    Emph(Vec<Inline>),
    /// `Underline`.
    Underline(Vec<Inline>),
    /// `Strong`.
    Strong(Vec<Inline>),
    /// `Strikeout`.
    Strikeout(Vec<Inline>),
    /// `Superscript`.
    Superscript(Vec<Inline>),
    /// `Subscript`.
    Subscript(Vec<Inline>),
    /// `SmallCaps`.
    SmallCaps(Vec<Inline>),
    /// `Quoted`.
    Quoted {
        /// Quotation mark style.
        kind: QuoteType,
        /// Quoted inlines.
        inlines: Vec<Inline>,
    },
    /// `Cite`.
    Cite {
        /// Citation records.
        citations: Vec<Citation>,
        /// Rendered citation inlines.
        inlines: Vec<Inline>,
    },
    /// `Code`: attributes plus verbatim text.
    Code {
        /// Code attributes.
        attr: Attr,
        /// Verbatim code text.
        text: String,
    },
    /// `Space`.
    Space,
    /// `SoftBreak`.
    SoftBreak,
    /// `LineBreak`.
    LineBreak,
    /// `Math`.
    Math {
        /// Display or inline mode.
        display: MathType,
        /// Authored formula source.
        text: String,
    },
    /// `RawInline`: format tag plus verbatim payload.
    RawInline {
        /// Raw format tag.
        format: String,
        /// Verbatim payload.
        text: String,
    },
    /// `Link`: attributes, display inlines, destination and title.
    Link {
        /// Link attributes.
        attr: Attr,
        /// Visible link inlines.
        inlines: Vec<Inline>,
        /// Destination and title.
        target: Target,
    },
    /// `Image`: attributes, alt inlines, source and title.
    Image {
        /// Image attributes.
        attr: Attr,
        /// Alternative text inlines.
        inlines: Vec<Inline>,
        /// Source and title.
        target: Target,
    },
    /// `Note`: note body blocks.
    Note(Vec<Block>),
    /// `Span`: attributes plus inlines.
    Span {
        /// Span attributes.
        attr: Attr,
        /// Wrapped inlines.
        inlines: Vec<Inline>,
    },
}

/// Quotation mark style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteType {
    /// `SingleQuote`.
    Single,
    /// `DoubleQuote`.
    Double,
}

/// Math rendering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathType {
    /// `DisplayMath`.
    Display,
    /// `InlineMath`.
    Inline,
}

/// Pandoc link/image destination and optional title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    /// URL or path string.
    pub url: String,
    /// Optional title string.
    pub title: String,
}

/// One citation record.
#[derive(Debug, Clone, PartialEq)]
pub struct Citation {
    /// Citation identifier.
    pub id: String,
    /// Prefix inlines.
    pub prefix: Vec<Inline>,
    /// Suffix inlines.
    pub suffix: Vec<Inline>,
    /// Citation mode.
    pub mode: CitationMode,
    /// Note number.
    pub note_num: i64,
    /// Citation hash.
    pub hash: i64,
}

/// Citation rendering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CitationMode {
    /// `NormalCitation`.
    Normal,
    /// `AuthorInText`.
    AuthorInText,
    /// `SuppressAuthor`.
    SuppressAuthor,
}
