//! `notedown-ir` to Pandoc JSON export adapter.
//!
//! The exporter walks the section-owned graph in authored order, emits only
//! constructors the IR actually carries, and returns a loss report for every
//! normalization it had to apply.

use std::collections::HashSet;

use document_ir::{
    AssetId, Block as IrBlock, BreakKind, DocumentGraph, Inline as IrInline, LossMarker, MathDisplay, NodeId, RelationEndpoint,
    SemanticStatus,
};

use crate::FormatError;

use super::{
    codec::{self, FORMAT_ID, PandocBudget},
    model::{
        API_VERSION, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, Inline, ListNumberDelim, ListNumberStyle,
        MathType, MetaValue, OrderedList, PandocDocument, QuoteType, Row, Table, TableBody, TableFoot, TableHead, Target,
    },
};

/// Result of a Pandoc JSON export.
#[derive(Debug, Clone, PartialEq)]
pub struct PandocExport {
    /// Encoded document model.
    pub document: PandocDocument,
    /// Normalizations applied while converting the IR.
    pub losses: Vec<LossMarker>,
}

impl PandocExport {
    /// Serialize the document with the default budget.
    pub fn to_json_text(&self) -> Result<String, FormatError> {
        codec::encode(&self.document, &PandocBudget::default())
    }

    /// Serialize the document with an explicit budget.
    pub fn to_json_text_with(&self, budget: &PandocBudget) -> Result<String, FormatError> {
        codec::encode(&self.document, budget)
    }
}

/// Export one document graph as Pandoc JSON.
pub fn export_pandoc_json(graph: &DocumentGraph) -> Result<PandocExport, FormatError> {
    let mut exporter = Exporter { graph, losses: Vec::new() };
    let document =
        PandocDocument { api_version: API_VERSION, meta: exporter.encode_metadata(), blocks: exporter.export_top_level()? };
    Ok(PandocExport { document, losses: exporter.losses })
}

struct Exporter<'a> {
    graph: &'a DocumentGraph,
    losses: Vec<LossMarker>,
}

impl Exporter<'_> {
    fn loss(&mut self, code: &str, message: String, status: SemanticStatus) {
        self.losses.push(LossMarker { code: code.into(), message, status });
    }

    fn encode_metadata(&mut self) -> Vec<(String, MetaValue)> {
        let mut meta = Vec::new();
        if let Some(title) = &self.graph.metadata.title {
            meta.push(("title".into(), MetaValue::Inlines(vec![Inline::Str(title.clone())])));
        }
        if let Some(language) = &self.graph.metadata.language {
            meta.push(("lang".into(), MetaValue::String(language.clone())));
        }
        if !self.graph.metadata.authors.is_empty() {
            meta.push((
                "author".into(),
                MetaValue::List(self.graph.metadata.authors.iter().cloned().map(MetaValue::String).collect()),
            ));
        }
        if !self.graph.metadata.tags.is_empty() {
            meta.push((
                "tags".into(),
                MetaValue::List(self.graph.metadata.tags.iter().cloned().map(MetaValue::String).collect()),
            ));
        }
        for entry in &self.graph.metadata.properties {
            let value = self.encode_metadata_value(&entry.value);
            meta.push((entry.key.clone(), value));
        }
        meta
    }

    /// Encode one typed metadata value.
    ///
    /// Pandoc JSON has no numeric metadata constructor, so integers and floats
    /// are written as `MetaString` and reported as lossy.
    fn encode_metadata_value(&mut self, value: &document_ir::MetadataValue) -> MetaValue {
        match value {
            document_ir::MetadataValue::Text(text) => MetaValue::String(text.clone()),
            document_ir::MetadataValue::Boolean(flag) => MetaValue::Bool(*flag),
            document_ir::MetadataValue::Integer(number) => {
                self.loss(
                    "export.pandoc-json.metadata_number",
                    format!("Pandoc JSON has no numeric metadata constructor, wrote `{number}` as MetaString"),
                    SemanticStatus::Partial,
                );
                MetaValue::String(number.to_string())
            }
            document_ir::MetadataValue::Float(number) => {
                self.loss(
                    "export.pandoc-json.metadata_number",
                    format!("Pandoc JSON has no numeric metadata constructor, wrote `{number}` as MetaString"),
                    SemanticStatus::Partial,
                );
                MetaValue::String(number.to_string())
            }
            document_ir::MetadataValue::List(items) => {
                MetaValue::List(items.iter().map(|item| self.encode_metadata_value(item)).collect())
            }
            document_ir::MetadataValue::Map(entries) => MetaValue::Map(
                entries.iter().map(|entry| (entry.key.clone(), self.encode_metadata_value(&entry.value))).collect(),
            ),
        }
    }

    fn export_top_level(&mut self) -> Result<Vec<Block>, FormatError> {
        let referenced = referenced_children(self.graph);
        let top_level: Vec<NodeId> =
            self.graph.blocks.iter().map(|node| node.id).filter(|id| !referenced.contains(id)).collect();
        self.export_sequence(&top_level)
    }

    fn export_sequence(&mut self, ids: &[NodeId]) -> Result<Vec<Block>, FormatError> {
        let mut blocks = Vec::with_capacity(ids.len());
        for id in ids {
            blocks.extend(self.export_node(*id)?);
        }
        Ok(blocks)
    }

    fn export_node(&mut self, id: NodeId) -> Result<Vec<Block>, FormatError> {
        let Some(node) = self.graph.block(id)
        else {
            self.loss(
                "export.pandoc-json.dangling_node",
                format!("node {} is missing from the graph", id.0),
                SemanticStatus::Unsupported,
            );
            return Ok(Vec::new());
        };
        let block = match &node.block {
            IrBlock::Section { level, title, children } => {
                let mut blocks =
                    vec![Block::Header { level: i64::from(*level), attr: Attr::empty(), inlines: self.export_inlines(title)? }];
                blocks.extend(self.export_sequence(children)?);
                return Ok(blocks);
            }
            IrBlock::Paragraph { content } => Block::Para(self.export_inlines(content)?),
            IrBlock::List { ordered, items } => {
                let mut pandoc_items = Vec::with_capacity(items.len());
                for item in items {
                    let mut blocks = Vec::new();
                    if !item.content.is_empty() {
                        blocks.push(Block::Plain(self.export_inlines(&item.content)?));
                    }
                    blocks.extend(self.export_sequence(&item.children)?);
                    pandoc_items.push(blocks);
                }
                if *ordered {
                    let start = items.iter().find_map(|item| item.number).map(i64::from).unwrap_or(1);
                    // Pandoc stores one starting number per list, so authored
                    // per-item numbers only survive when they are consecutive.
                    for (index, item) in items.iter().enumerate() {
                        if let Some(number) = item.number {
                            let expected = start + index as i64;
                            if i64::from(number) != expected {
                                self.loss(
                                    "export.pandoc-json.list_number",
                                    format!("list item {index} authored number {number} becomes {expected}"),
                                    SemanticStatus::Partial,
                                );
                            }
                        }
                    }
                    Block::OrderedList(OrderedList {
                        start,
                        style: ListNumberStyle::Decimal,
                        delimiter: ListNumberDelim::Period,
                        items: pandoc_items,
                    })
                }
                else {
                    Block::BulletList(pandoc_items)
                }
            }
            IrBlock::Table { caption, head, body, foot } => Block::Table(self.export_table(caption, head, body, foot)?),
            IrBlock::Quote { children } => Block::BlockQuote(self.export_sequence(children)?),
            IrBlock::Code { language, content } => {
                let mut classes = Vec::new();
                if let Some(language) = language {
                    classes.push(format!("language-{language}"));
                }
                Block::CodeBlock(Attr { id: String::new(), classes, pairs: Vec::new() }, content.clone())
            }
            IrBlock::Math { formula } => Block::Para(vec![Inline::Math {
                display: match formula.display {
                    MathDisplay::Display => MathType::Display,
                    MathDisplay::Inline => MathType::Inline,
                },
                text: formula.source.clone(),
            }]),
            IrBlock::Diagram { language, source } => Block::CodeBlock(
                Attr { id: String::new(), classes: vec!["diagram".into(), language.clone()], pairs: Vec::new() },
                source.clone(),
            ),
            IrBlock::Chart { language, source } => Block::CodeBlock(
                Attr { id: String::new(), classes: vec!["chart".into(), language.clone()], pairs: Vec::new() },
                source.clone(),
            ),
            IrBlock::Opaque { kind, payload_hint, .. } => self.export_opaque(kind, payload_hint)?,
            IrBlock::RawHtml { html } => Block::RawBlock("html".into(), html.clone()),
        };
        Ok(vec![block])
    }

    fn export_opaque(&mut self, kind: &str, payload_hint: &str) -> Result<Block, FormatError> {
        if kind == "thematic_break" {
            return Ok(Block::HorizontalRule);
        }
        if kind == "pandoc.div" || kind == "pandoc.figure" {
            let value: serde_json::Value = serde_json::from_str(payload_hint).map_err(|error| {
                FormatError::parse(FORMAT_ID, format!("opaque `{kind}` payload is not valid JSON: {error}"))
            })?;
            return codec::decode_block_value(&value);
        }
        if let Some(format) = kind.strip_prefix("pandoc.raw.") {
            return Ok(Block::RawBlock(format.into(), payload_hint.into()));
        }
        self.loss(
            "export.pandoc-json.opaque_raw",
            format!("opaque node `{kind}` has no Pandoc constructor and is written as a `notedown.{kind}` raw block"),
            SemanticStatus::Partial,
        );
        Ok(Block::RawBlock(format!("notedown.{kind}"), payload_hint.into()))
    }

    fn export_table(
        &mut self,
        caption: &Option<String>,
        head: &[document_ir::TableRow],
        body: &[document_ir::TableRow],
        foot: &[document_ir::TableRow],
    ) -> Result<Table, FormatError> {
        let caption = match caption {
            Some(text) => Caption { short: None, blocks: vec![Block::Plain(vec![Inline::Str(text.clone())])] },
            None => Caption::empty(),
        };
        let columns = head
            .iter()
            .chain(body)
            .chain(foot)
            .map(|row| row.cells.iter().map(|cell| usize::try_from(cell.colspan.max(1)).unwrap_or(1)).sum::<usize>())
            .max()
            .unwrap_or(0);
        let colspecs = (0..columns).map(|_| ColSpec { alignment: super::model::Alignment::Default, width: 0.0 }).collect();
        let head_rows = head.iter().map(|row| self.export_row(row)).collect::<Result<Vec<_>, _>>()?;
        let body_rows = body.iter().map(|row| self.export_row(row)).collect::<Result<Vec<_>, _>>()?;
        let foot_rows = foot.iter().map(|row| self.export_row(row)).collect::<Result<Vec<_>, _>>()?;
        Ok(Table {
            attr: Attr::empty(),
            caption,
            colspecs,
            head: TableHead { attr: Attr::empty(), rows: head_rows },
            bodies: vec![TableBody { attr: Attr::empty(), row_head_columns: 0, head: Vec::new(), body: body_rows }],
            foot: TableFoot { attr: Attr::empty(), rows: foot_rows },
        })
    }

    fn export_row(&mut self, row: &document_ir::TableRow) -> Result<Row, FormatError> {
        let mut cells = Vec::with_capacity(row.cells.len());
        for cell in &row.cells {
            let mut blocks = Vec::new();
            for child in &cell.children {
                blocks.extend(self.export_node(*child)?);
            }
            cells.push(Cell {
                attr: Attr::empty(),
                alignment: super::model::Alignment::Default,
                row_span: i64::from(cell.rowspan.max(1)),
                col_span: i64::from(cell.colspan.max(1)),
                blocks,
            });
        }
        Ok(Row { attr: Attr::empty(), cells })
    }

    fn export_inlines(&mut self, inlines: &[IrInline]) -> Result<Vec<Inline>, FormatError> {
        let mut exported = Vec::with_capacity(inlines.len());
        for inline in inlines {
            exported.push(self.export_inline(inline)?);
        }
        Ok(exported)
    }

    fn export_inline(&mut self, inline: &IrInline) -> Result<Inline, FormatError> {
        match inline {
            IrInline::Text { text } => Ok(Inline::Str(text.clone())),
            IrInline::Styled { style, children } => {
                let children = self.export_inlines(children)?;
                match style.as_str() {
                    "italic" | "emph" => Ok(Inline::Emph(children)),
                    "bold" | "strong" => Ok(Inline::Strong(children)),
                    "strike" | "strikeout" => Ok(Inline::Strikeout(children)),
                    "underline" => Ok(Inline::Underline(children)),
                    "superscript" => Ok(Inline::Superscript(children)),
                    "subscript" => Ok(Inline::Subscript(children)),
                    "smallcaps" => Ok(Inline::SmallCaps(children)),
                    "quote-single" => Ok(Inline::Quoted { kind: QuoteType::Single, inlines: children }),
                    "quote-double" => Ok(Inline::Quoted { kind: QuoteType::Double, inlines: children }),
                    other => {
                        Err(FormatError::unsupported(FORMAT_ID, format!("inline style `{other}` has no Pandoc constructor")))
                    }
                }
            }
            IrInline::InlineCode { text } => Ok(Inline::Code { attr: Attr::empty(), text: text.clone() }),
            IrInline::InlineMath { formula } => Ok(Inline::Math {
                display: match formula.display {
                    MathDisplay::Display => MathType::Display,
                    MathDisplay::Inline => MathType::Inline,
                },
                text: formula.source.clone(),
            }),
            IrInline::Reference { target, display } => {
                self.loss(
                    "export.pandoc-json.reference_anchor",
                    format!("intra-document reference to node {} is written as a `#node-{}` link", target.0, target.0),
                    SemanticStatus::Partial,
                );
                Ok(Inline::Link {
                    attr: Attr::empty(),
                    inlines: vec![Inline::Str(display.clone())],
                    target: Target { url: format!("#node-{}", target.0), title: String::new() },
                })
            }
            IrInline::Link { target, display, title } => {
                let inlines = self.export_inlines(display)?;
                let (url, loss) = match target {
                    RelationEndpoint::ExternalUri(url) => (url.clone(), false),
                    RelationEndpoint::Node(id) => (format!("#node-{}", id.0), true),
                    RelationEndpoint::Document(id) => (format!("#document-{}", id.0), true),
                    RelationEndpoint::Asset(id) => (self.asset_source(*id), true),
                    RelationEndpoint::Unresolved { .. } => (String::new(), true),
                };
                if loss {
                    self.loss(
                        "export.pandoc-json.link_target",
                        "a non-external link target was written as a fragment or empty URL".into(),
                        SemanticStatus::Partial,
                    );
                }
                Ok(Inline::Link {
                    attr: Attr::empty(),
                    inlines,
                    target: Target { url, title: title.clone().unwrap_or_default() },
                })
            }
            IrInline::Image { source, alt, title, asset } => {
                let url =
                    if source.is_empty() { asset.map(|id| self.asset_source(id)).unwrap_or_default() } else { source.clone() };
                if url.is_empty() {
                    self.loss(
                        "export.pandoc-json.image_source_missing",
                        "an image has no source or asset location and was written with an empty URL".into(),
                        SemanticStatus::Unresolved,
                    );
                }
                Ok(Inline::Image {
                    attr: Attr::empty(),
                    inlines: self.export_inlines(alt)?,
                    target: Target { url, title: title.clone().unwrap_or_default() },
                })
            }
            IrInline::Note { label, display } => {
                let Some(display) = display.as_ref().filter(|display| !display.is_empty())
                else {
                    self.loss(
                        "export.pandoc-json.note_content_unavailable",
                        format!("note `{label}` has no display content and was written with an empty body"),
                        SemanticStatus::Partial,
                    );
                    return Ok(Inline::Note(Vec::new()));
                };
                Ok(Inline::Note(vec![Block::Para(self.export_inlines(display)?)]))
            }
            IrInline::Citation { key, display } => {
                let inlines = match display {
                    Some(display) if !display.is_empty() => self.export_inlines(display)?,
                    _ => {
                        self.loss(
                            "export.pandoc-json.citation_display_synthesized",
                            format!("citation `{key}` has no display content and the key was written as visible text"),
                            SemanticStatus::Partial,
                        );
                        vec![Inline::Str(key.clone())]
                    }
                };
                Ok(Inline::Cite {
                    citations: vec![Citation {
                        id: key.clone(),
                        prefix: Vec::new(),
                        suffix: Vec::new(),
                        mode: CitationMode::Normal,
                        note_num: 0,
                        hash: 0,
                    }],
                    inlines,
                })
            }
            IrInline::Break { kind } => Ok(match kind {
                BreakKind::Soft => Inline::SoftBreak,
                BreakKind::Hard => Inline::LineBreak,
            }),
            IrInline::Attributed { attributes, children } => {
                Ok(Inline::Span { attr: attr_from_attributes(attributes), inlines: self.export_inlines(children)? })
            }
            IrInline::RawHtml { html } => Ok(Inline::RawInline { format: "html".into(), text: html.clone() }),
        }
    }

    fn asset_source(&self, id: AssetId) -> String {
        self.graph.assets.iter().find(|asset| asset.id == id).and_then(|asset| asset.source.clone()).unwrap_or_default()
    }
}

fn attr_from_attributes(attributes: &document_ir::Attributes) -> Attr {
    Attr {
        id: attributes.id.clone().unwrap_or_default(),
        classes: attributes.classes.clone(),
        pairs: attributes.pairs.iter().map(|pair| (pair.key.clone(), pair.value.clone())).collect(),
    }
}

fn referenced_children(graph: &DocumentGraph) -> HashSet<NodeId> {
    let mut referenced = HashSet::new();
    for node in &graph.blocks {
        match &node.block {
            IrBlock::Section { children, .. } => referenced.extend(children.iter().copied()),
            IrBlock::Quote { children, .. } => referenced.extend(children.iter().copied()),
            IrBlock::List { items, .. } => {
                for item in items {
                    referenced.extend(item.children.iter().copied());
                }
            }
            IrBlock::Table { head, body, foot, .. } => {
                for cell in head.iter().chain(body).chain(foot).flat_map(|row| row.cells.iter()) {
                    referenced.extend(cell.children.iter().copied());
                }
            }
            _ => {}
        }
    }
    referenced
}
