//! Pandoc JSON to `notedown-ir` import adapter.
//!
//! The adapter treats unmapped constructors as refusals under the default
//! strict policy. [`PandocLossPolicy::AllowLoss`] keeps the content, records a
//! stable loss code, and marks the projection partial. Metadata keys outside
//! the IR's fixed fields always lose per key, because metadata is data, not a
//! construct the IR could expand.

use std::path::Path;

use document_ir::{
    Attributes, Block as IrBlock, BreakKind, DocumentGraph, DocumentId, Inline as IrInline, ListItem, LossMarker, MathDisplay,
    MathFormula, RelationEndpoint, SemanticStatus, SourceKind, SourceRef, TableCell, TableRow,
};

use crate::{FormatError, import::register_image_assets};

use super::{
    codec::{self, FORMAT_ID, PandocBudget},
    model::{
        Alignment, Attr, Block, Caption, Cell, CitationMode, Inline, ListNumberDelim, ListNumberStyle, MathType, MetaValue,
        OrderedList, PandocDocument, QuoteType, Table,
    },
};

/// How the adapter treats constructs the IR cannot represent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PandocLossPolicy {
    /// Reject every unmapped construct with an unsupported error.
    #[default]
    Strict,
    /// Keep the content, record a loss marker, and mark the graph partial.
    AllowLoss,
}

/// Import controls for the Pandoc JSON adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PandocImportOptions {
    /// Construct policy.
    pub policy: PandocLossPolicy,
    /// Wire budgets.
    pub budget: PandocBudget,
}

impl Default for PandocImportOptions {
    fn default() -> Self {
        Self { policy: PandocLossPolicy::default(), budget: PandocBudget::default() }
    }
}

impl PandocImportOptions {
    /// Options that keep content and record losses.
    pub fn allow_loss() -> Self {
        Self { policy: PandocLossPolicy::AllowLoss, budget: PandocBudget::default() }
    }
}

/// Import a Pandoc JSON file, rejecting unmapped constructs.
pub fn import_pandoc_json(path: impl AsRef<Path>) -> Result<DocumentGraph, FormatError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)
        .map_err(|error| FormatError::invalid_input(format!("failed to read {}: {error}", path.display())))?;
    import_pandoc_json_bytes(&path.display().to_string(), &bytes)
}

/// Import Pandoc JSON bytes with the default strict policy.
pub fn import_pandoc_json_bytes(label: &str, bytes: &[u8]) -> Result<DocumentGraph, FormatError> {
    import_pandoc_json_bytes_with(label, bytes, &PandocImportOptions::default())
}

/// Import Pandoc JSON text with the default strict policy.
pub fn import_pandoc_json_text(label: &str, text: &str) -> Result<DocumentGraph, FormatError> {
    import_pandoc_json_bytes(label, text.as_bytes())
}

/// Import Pandoc JSON bytes with explicit policy and budgets.
pub fn import_pandoc_json_bytes_with(
    label: &str,
    bytes: &[u8],
    options: &PandocImportOptions,
) -> Result<DocumentGraph, FormatError> {
    let document = codec::decode(bytes, &options.budget)?;
    Importer::new(label, options.policy).lower(document)
}

struct Importer {
    graph: DocumentGraph,
    policy: PandocLossPolicy,
    losses: Vec<LossMarker>,
    next_note: u32,
}

impl Importer {
    fn new(label: &str, policy: PandocLossPolicy) -> Self {
        Self { graph: DocumentGraph::new(document_id_for(label)), policy, losses: Vec::new(), next_note: 1 }
    }

    fn lower(mut self, document: PandocDocument) -> Result<DocumentGraph, FormatError> {
        self.apply_metadata(&document.meta)?;
        self.lower_sequence(&document.blocks, "/blocks", 0)?;
        for loss in self.losses.drain(..) {
            self.graph.push_loss(loss);
        }
        register_image_assets(&mut self.graph);
        let asset_sources: Vec<String> =
            self.graph.assets.iter().map(|asset| asset.source.clone().unwrap_or_default()).collect();
        for source in asset_sources {
            self.graph.push_loss(LossMarker {
                code: "import.pandoc-json.asset_unresolved".into(),
                message: format!(
                    "image `{source}` is registered as an unresolved asset because Pandoc JSON carries no media bytes and no resolver was supplied"
                ),
                status: SemanticStatus::Unresolved,
            });
        }
        Ok(self.graph)
    }

    fn apply_metadata(&mut self, meta: &[(String, MetaValue)]) -> Result<(), FormatError> {
        let mut title = None;
        let mut language = None;
        let mut authors = Vec::new();
        let mut tags = Vec::new();
        let mut properties = Vec::new();
        for (key, value) in meta {
            let pointer = format!("/meta/{}", escape_pointer_token(key));
            match key.as_str() {
                "title" => title = Some(self.meta_text(value, &pointer)?),
                "author" | "authors" => authors.extend(self.meta_texts(value, &pointer)?),
                "lang" | "language" => language = Some(self.meta_text(value, &pointer)?),
                "keywords" | "tags" => tags.extend(self.meta_texts(value, &pointer)?),
                _ => properties.push(document_ir::MetadataEntry { key: key.clone(), value: self.meta_typed(value, &pointer) }),
            }
        }
        self.graph.set_metadata(document_ir::DocumentMetadata { title, language, authors, tags, properties });
        Ok(())
    }

    /// Lower a Pandoc metadata value into the typed IR payload.
    ///
    /// Pandoc has no numeric metadata constructor, so numbers stay text when
    /// exporting. Inline and block metadata keep their flattened text because
    /// document metadata has no node identity to attach rich content to.
    fn meta_typed(&mut self, value: &MetaValue, pointer: &str) -> document_ir::MetadataValue {
        match value {
            MetaValue::Bool(flag) => document_ir::MetadataValue::Boolean(*flag),
            MetaValue::String(text) => document_ir::MetadataValue::Text(text.clone()),
            MetaValue::Inlines(inlines) => document_ir::MetadataValue::Text(model_inline_text(inlines)),
            MetaValue::Blocks(blocks) => document_ir::MetadataValue::Text(blocks_text(blocks)),
            MetaValue::List(items) => document_ir::MetadataValue::List(
                items.iter().enumerate().map(|(index, item)| self.meta_typed(item, &format!("{pointer}/{index}"))).collect(),
            ),
            MetaValue::Map(entries) => document_ir::MetadataValue::Map(
                entries
                    .iter()
                    .map(|(key, item)| document_ir::MetadataEntry {
                        key: key.clone(),
                        value: self.meta_typed(item, &format!("{pointer}/{}", escape_pointer_token(key))),
                    })
                    .collect(),
            ),
        }
    }

    fn meta_text(&mut self, value: &MetaValue, pointer: &str) -> Result<String, FormatError> {
        match value {
            MetaValue::String(text) => Ok(text.clone()),
            MetaValue::Inlines(inlines) => Ok(model_inline_text(inlines)),
            MetaValue::Blocks(blocks) => Ok(blocks_text(blocks)),
            _ => {
                self.losses.push(LossMarker {
                    code: "import.pandoc-json.meta_shape".into(),
                    message: format!(
                        "metadata at `{pointer}` is not a string, inline or block value, so only its existence is reported"
                    ),
                    status: SemanticStatus::Partial,
                });
                Ok(String::new())
            }
        }
    }

    fn meta_texts(&mut self, value: &MetaValue, pointer: &str) -> Result<Vec<String>, FormatError> {
        match value {
            MetaValue::List(items) => {
                let mut texts = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    texts.push(self.meta_text(item, &format!("{pointer}/{index}"))?);
                }
                Ok(texts)
            }
            other => Ok(vec![self.meta_text(other, pointer)?]),
        }
    }

    /// Lower one block array into a local section-aware sequence.
    ///
    /// The returned ids are the sequence's roots. Sections own their children,
    /// so a heading inside a quote, list item, or table cell never changes the
    /// outer document outline.
    fn lower_sequence(
        &mut self,
        blocks: &[Block],
        array_pointer: &str,
        start: usize,
    ) -> Result<Vec<document_ir::NodeId>, FormatError> {
        let mut roots = Vec::new();
        let mut stack: Vec<(document_ir::NodeId, i64)> = Vec::new();
        for (index, block) in blocks.iter().enumerate().skip(start) {
            let pointer = format!("{array_pointer}/{index}");
            let Block::Header { level, attr, inlines } = block
            else {
                let lowered = self.lower_block(block, &pointer)?;
                let id = self.push_node(lowered, &pointer);
                match stack.last() {
                    Some((parent, _)) => self.append_child(*parent, id, &pointer)?,
                    None => roots.push(id),
                }
                continue;
            };
            while let Some((_, top_level)) = stack.last() {
                if *top_level >= *level {
                    stack.pop();
                }
                else {
                    break;
                }
            }
            let title = self.lower_inlines(inlines, &format!("{pointer}/c/2"))?;
            if !attr.is_empty() {
                self.structural_loss(
                    &pointer,
                    "import.pandoc-json.header_attr",
                    "heading attributes have no IR field, so the identifier, classes and pairs were dropped",
                    SemanticStatus::Partial,
                )?;
            }
            let ir_level = self.header_level(*level, &pointer)?;
            let id = self.push_node(IrBlock::Section { level: ir_level, title, children: Vec::new() }, &pointer);
            match stack.last() {
                Some((parent, _)) => self.append_child(*parent, id, &pointer)?,
                None => roots.push(id),
            }
            stack.push((id, *level));
        }
        Ok(roots)
    }

    fn header_level(&mut self, level: i64, pointer: &str) -> Result<u8, FormatError> {
        if (1..=6).contains(&level) {
            return Ok(level as u8);
        }
        self.structural_loss(
            pointer,
            "import.pandoc-json.header_level",
            &format!("heading level `{level}` is outside 1..=6 and was clamped into the IR range"),
            SemanticStatus::Partial,
        )?;
        Ok(level.clamp(1, 6) as u8)
    }

    fn push_node(&mut self, block: IrBlock, pointer: &str) -> document_ir::NodeId {
        let id = self.graph.push_block(block);
        self.graph.attach_source(SourceRef {
            node: id,
            kind: SourceKind::Synthetic { reason: format!("pandoc-json pointer {pointer}") },
            precision: SemanticStatus::Partial,
        });
        id
    }

    fn append_child(
        &mut self,
        parent: document_ir::NodeId,
        child: document_ir::NodeId,
        pointer: &str,
    ) -> Result<(), FormatError> {
        self.graph.append_child(parent, child).map_err(|error| FormatError::parse(FORMAT_ID, format!("{pointer}: {error}")))
    }

    fn lower_block(&mut self, block: &Block, pointer: &str) -> Result<IrBlock, FormatError> {
        match block {
            Block::Plain(inlines) | Block::Para(inlines) => {
                if let [Inline::Math { display: MathType::Display, text }] = inlines.as_slice() {
                    self.losses.push(LossMarker {
                        code: "import.pandoc-json.math_source_only".into(),
                        message: format!(
                            "math at `{pointer}` keeps its authored source because the IR has no Pandoc math lowering profile"
                        ),
                        status: SemanticStatus::Partial,
                    });
                    return Ok(IrBlock::Math {
                        formula: MathFormula::unsupported(
                            text.clone(),
                            Some("latex".into()),
                            MathDisplay::Display,
                            "pandoc-json",
                        ),
                    });
                }
                Ok(IrBlock::Paragraph { content: self.lower_inlines(inlines, &format!("{pointer}/c"))? })
            }
            Block::LineBlock(lines) => {
                self.structural_loss(
                    pointer,
                    "import.pandoc-json.line_block_flattened",
                    "the IR has no line-block container, so lines are joined with hard breaks",
                    SemanticStatus::Partial,
                )?;
                let mut content = Vec::new();
                for (index, line) in lines.iter().enumerate() {
                    if index > 0 {
                        content.push(IrInline::Break { kind: BreakKind::Hard });
                    }
                    content.extend(self.lower_inlines(line, &format!("{pointer}/c/{index}"))?);
                }
                Ok(IrBlock::Paragraph { content })
            }
            Block::CodeBlock(attr, text) => self.lower_code_block(attr, text, pointer),
            Block::RawBlock(format, text) => {
                if format.eq_ignore_ascii_case("html") {
                    Ok(IrBlock::RawHtml { html: text.clone() })
                }
                else {
                    self.structural_loss(
                        pointer,
                        "import.pandoc-json.raw_block",
                        &format!("raw `{format}` block has no IR semantics and stays an opaque payload"),
                        SemanticStatus::Partial,
                    )?;
                    Ok(IrBlock::Opaque {
                        kind: format!("pandoc.raw.{format}"),
                        payload_hint: text.clone(),
                        status: SemanticStatus::Partial,
                    })
                }
            }
            Block::BlockQuote(blocks) => self.lower_quote(blocks, pointer),
            Block::OrderedList(list) => self.lower_ordered_list(list, pointer),
            Block::BulletList(items) => {
                let mut lowered = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    lowered.push(self.lower_list_item(item, None, &format!("{pointer}/c/{index}"))?);
                }
                Ok(IrBlock::List { ordered: false, items: lowered })
            }
            Block::DefinitionList(entries) => {
                self.structural_loss(
                    pointer,
                    "import.pandoc-json.definition_list_flattened",
                    "the IR has no definition-list constructor, so terms become items and definitions become child blocks",
                    SemanticStatus::Partial,
                )?;
                let mut items = Vec::with_capacity(entries.len());
                for (index, (term, definitions)) in entries.iter().enumerate() {
                    let entry_pointer = format!("{pointer}/c/{index}");
                    let mut children = Vec::new();
                    for (definition_index, definition) in definitions.iter().enumerate() {
                        children.extend(self.lower_sequence(
                            definition,
                            &format!("{entry_pointer}/1/{definition_index}"),
                            0,
                        )?);
                    }
                    items.push(ListItem {
                        content: self.lower_inlines(term, &format!("{entry_pointer}/0"))?,
                        children,
                        number: None,
                        is_task: false,
                        is_checked: None,
                    });
                }
                Ok(IrBlock::List { ordered: false, items })
            }
            Block::Header { .. } => {
                Err(FormatError::parse(FORMAT_ID, format!("{pointer}: header reached a non-sequence lowering path")))
            }
            Block::HorizontalRule => Ok(IrBlock::Opaque {
                kind: "thematic_break".into(),
                payload_hint: "---".into(),
                status: SemanticStatus::Partial,
            }),
            Block::Table(table) => self.lower_table(table, pointer),
            Block::Figure { .. } => {
                self.structural_loss(
                    pointer,
                    "import.pandoc-json.figure_payload",
                    "the IR has no figure constructor, so the full figure payload is kept as an opaque foreign node",
                    SemanticStatus::Partial,
                )?;
                Ok(IrBlock::Opaque {
                    kind: "pandoc.figure".into(),
                    payload_hint: self.block_payload(block, pointer)?,
                    status: SemanticStatus::Partial,
                })
            }
            Block::Div { .. } => {
                self.structural_loss(
                    pointer,
                    "import.pandoc-json.div_payload",
                    "the IR has no generic container, so the full div payload is kept as an opaque foreign node",
                    SemanticStatus::Partial,
                )?;
                Ok(IrBlock::Opaque {
                    kind: "pandoc.div".into(),
                    payload_hint: self.block_payload(block, pointer)?,
                    status: SemanticStatus::Partial,
                })
            }
        }
    }

    fn block_payload(&self, block: &Block, pointer: &str) -> Result<String, FormatError> {
        let value = codec::encode_block_value(block)?;
        serde_json::to_string(&value)
            .map_err(|error| FormatError::parse(FORMAT_ID, format!("{pointer}: cannot encode foreign payload: {error}")))
    }

    fn lower_code_block(&mut self, attr: &Attr, text: &str, pointer: &str) -> Result<IrBlock, FormatError> {
        let mut remaining_classes = attr.classes.clone();
        if let Some(marker) = remaining_classes.first().filter(|class| *class == "diagram" || *class == "chart") {
            let kind = marker.clone();
            remaining_classes.remove(0);
            let language = remaining_classes.first().cloned().unwrap_or_default();
            if !remaining_classes.is_empty() {
                remaining_classes.remove(0);
            }
            if !remaining_classes.is_empty() || !attr.id.is_empty() || !attr.pairs.is_empty() {
                self.structural_loss(
                    pointer,
                    "import.pandoc-json.code_attr",
                    "code attributes beyond the diagram/chart language marker have no IR field",
                    SemanticStatus::Partial,
                )?;
            }
            return Ok(if kind == "diagram" {
                IrBlock::Diagram { language, source: text.to_string() }
            }
            else {
                IrBlock::Chart { language, source: text.to_string() }
            });
        }
        let mut language = None;
        if let Some(position) = remaining_classes.iter().position(|class| class.starts_with("language-")) {
            let class = remaining_classes.remove(position);
            language = class.strip_prefix("language-").map(str::to_string);
        }
        if !remaining_classes.is_empty() || !attr.id.is_empty() || !attr.pairs.is_empty() {
            self.structural_loss(
                pointer,
                "import.pandoc-json.code_attr",
                "code attributes other than the `language-` class have no IR field and were dropped",
                SemanticStatus::Partial,
            )?;
        }
        Ok(IrBlock::Code { language, content: text.to_string() })
    }

    fn lower_quote(&mut self, blocks: &[Block], pointer: &str) -> Result<IrBlock, FormatError> {
        let mut children = Vec::new();
        let start = match blocks.first() {
            Some(Block::Plain(inlines) | Block::Para(inlines)) => {
                let content = self.lower_inlines(inlines, &format!("{pointer}/c/0/c"))?;
                let id = self.push_node(IrBlock::Paragraph { content }, &format!("{pointer}/c/0"));
                children.push(id);
                1
            }
            _ => 0,
        };
        children.extend(self.lower_sequence(blocks, &format!("{pointer}/c"), start)?);
        Ok(IrBlock::Quote { children })
    }

    fn lower_ordered_list(&mut self, list: &OrderedList, pointer: &str) -> Result<IrBlock, FormatError> {
        if list.style != ListNumberStyle::Default && list.style != ListNumberStyle::Decimal {
            self.structural_loss(
                pointer,
                "import.pandoc-json.list_style",
                &format!("ordered-list style `{:?}` has no IR field and was lowered to decimal numbering", list.style),
                SemanticStatus::Partial,
            )?;
        }
        if list.delimiter != ListNumberDelim::Default && list.delimiter != ListNumberDelim::Period {
            self.structural_loss(
                pointer,
                "import.pandoc-json.list_delimiter",
                &format!("ordered-list delimiter `{:?}` has no IR field and was dropped", list.delimiter),
                SemanticStatus::Partial,
            )?;
        }
        let mut items = Vec::with_capacity(list.items.len());
        for (index, item) in list.items.iter().enumerate() {
            let number = u32::try_from(list.start.saturating_add(index as i64)).ok();
            if number.is_none() || number == Some(0) {
                self.structural_loss(
                    pointer,
                    "import.pandoc-json.list_number",
                    "ordered-list numbers outside the IR's positive range were replaced by sequence positions",
                    SemanticStatus::Partial,
                )?;
            }
            let fallback = (index + 1) as u32;
            items.push(self.lower_list_item(
                item,
                Some(number.unwrap_or(fallback).max(1)),
                &format!("{pointer}/c/1/{index}"),
            )?);
        }
        Ok(IrBlock::List { ordered: true, items })
    }

    fn lower_list_item(&mut self, item: &[Block], number: Option<u32>, pointer: &str) -> Result<ListItem, FormatError> {
        let mut content = Vec::new();
        let start = match item.first() {
            Some(Block::Plain(inlines) | Block::Para(inlines)) => {
                content = self.lower_inlines(inlines, &format!("{pointer}/0/c"))?;
                1
            }
            _ => 0,
        };
        let children = self.lower_sequence(item, pointer, start)?;
        Ok(ListItem { content, children, number, is_task: false, is_checked: None })
    }

    fn lower_table(&mut self, table: &Table, pointer: &str) -> Result<IrBlock, FormatError> {
        if !table.attr.is_empty() {
            self.structural_loss(
                pointer,
                "import.pandoc-json.table_attr",
                "table attributes have no IR field and were dropped",
                SemanticStatus::Partial,
            )?;
        }
        if table.caption.short.is_some() {
            self.structural_loss(
                pointer,
                "import.pandoc-json.table_short_caption",
                "the short caption has no IR field and was dropped",
                SemanticStatus::Partial,
            )?;
        }
        if table.colspecs.iter().any(|colspec| colspec.alignment != Alignment::Default || colspec.width != 0.0) {
            self.structural_loss(
                pointer,
                "import.pandoc-json.table_columns",
                "column alignment and width have no IR field and were dropped",
                SemanticStatus::Partial,
            )?;
        }
        if !table.head.attr.is_empty()
            || !table.foot.attr.is_empty()
            || table.bodies.iter().any(|body| !body.attr.is_empty())
            || table.bodies.len() > 1
            || table.bodies.iter().any(|body| body.row_head_columns != 0)
        {
            self.structural_loss(
                pointer,
                "import.pandoc-json.table_structure",
                "table section attributes, split bodies, or row-head columns have no IR field",
                SemanticStatus::Partial,
            )?;
        }
        let caption = self.caption_text(&table.caption, &format!("{pointer}/c/1"))?;
        let mut head = Vec::new();
        for (index, row) in table.head.rows.iter().enumerate() {
            head.push(self.lower_row(row, &format!("{pointer}/c/3/1/{index}"))?);
        }
        let mut body = Vec::new();
        for (body_index, body_section) in table.bodies.iter().enumerate() {
            let body_pointer = format!("{pointer}/c/4/{body_index}");
            for (index, row) in body_section.head.iter().enumerate() {
                head.push(self.lower_row(row, &format!("{body_pointer}/2/{index}"))?);
            }
            for (index, row) in body_section.body.iter().enumerate() {
                body.push(self.lower_row(row, &format!("{body_pointer}/3/{index}"))?);
            }
        }
        let mut foot = Vec::new();
        for (index, row) in table.foot.rows.iter().enumerate() {
            foot.push(self.lower_row(row, &format!("{pointer}/c/5/1/{index}"))?);
        }
        Ok(IrBlock::Table { caption, head, body, foot })
    }

    fn caption_text(&mut self, caption: &Caption, pointer: &str) -> Result<Option<String>, FormatError> {
        if caption.blocks.is_empty() {
            return Ok(None);
        }
        if caption.blocks.len() > 1 || !matches!(caption.blocks[0], Block::Plain(_) | Block::Para(_)) {
            self.structural_loss(
                pointer,
                "import.pandoc-json.table_caption_structure",
                "caption blocks other than a single paragraph were flattened into one string",
                SemanticStatus::Partial,
            )?;
        }
        Ok(Some(blocks_text(&caption.blocks)))
    }

    fn lower_row(&mut self, row: &super::model::Row, pointer: &str) -> Result<TableRow, FormatError> {
        let mut cells = Vec::with_capacity(row.cells.len());
        for (index, cell) in row.cells.iter().enumerate() {
            cells.push(self.lower_cell(cell, &format!("{pointer}/1/{index}"))?);
        }
        Ok(TableRow { cells })
    }

    fn lower_cell(&mut self, cell: &Cell, pointer: &str) -> Result<TableCell, FormatError> {
        if !cell.attr.is_empty() || cell.alignment != Alignment::Default {
            self.structural_loss(
                pointer,
                "import.pandoc-json.cell_attr",
                "cell attributes and alignment have no IR field and were dropped",
                SemanticStatus::Partial,
            )?;
        }
        if cell.row_span < 1 || cell.col_span < 1 {
            self.structural_loss(
                pointer,
                "import.pandoc-json.cell_span",
                "cell spans below one were clamped to one row or column",
                SemanticStatus::Partial,
            )?;
        }
        // The IR cell holds child blocks, so every authored cell block is
        // lowered and linked instead of being flattened into one inline run.
        let children = self.lower_sequence(&cell.blocks, &format!("{pointer}/4"), 0)?;
        Ok(TableCell {
            children,
            colspan: u32::try_from(cell.col_span.max(1)).unwrap_or(1),
            rowspan: u32::try_from(cell.row_span.max(1)).unwrap_or(1),
        })
    }

    fn lower_inlines(&mut self, inlines: &[Inline], array_pointer: &str) -> Result<Vec<IrInline>, FormatError> {
        let mut lowered = Vec::new();
        for (index, inline) in inlines.iter().enumerate() {
            let pointer = format!("{array_pointer}/{index}");
            if let Some(inline) = self.lower_inline(inline, &pointer)? {
                lowered.push(inline);
            }
        }
        Ok(lowered)
    }

    fn lower_inline(&mut self, inline: &Inline, pointer: &str) -> Result<Option<IrInline>, FormatError> {
        match inline {
            Inline::Str(text) => Ok(Some(IrInline::Text { text: text.clone() })),
            Inline::Emph(inlines) => Ok(Some(self.styled("italic", inlines, pointer)?)),
            Inline::Strong(inlines) => Ok(Some(self.styled("bold", inlines, pointer)?)),
            Inline::Strikeout(inlines) => Ok(Some(self.styled("strike", inlines, pointer)?)),
            Inline::Underline(inlines) => Ok(Some(self.styled_with_loss("underline", inlines, pointer)?)),
            Inline::Superscript(inlines) => Ok(Some(self.styled_with_loss("superscript", inlines, pointer)?)),
            Inline::Subscript(inlines) => Ok(Some(self.styled_with_loss("subscript", inlines, pointer)?)),
            Inline::SmallCaps(inlines) => Ok(Some(self.styled_with_loss("smallcaps", inlines, pointer)?)),
            Inline::Quoted { kind, inlines } => {
                let style = match kind {
                    QuoteType::Single => "quote-single",
                    QuoteType::Double => "quote-double",
                };
                Ok(Some(self.styled_with_loss(style, inlines, pointer)?))
            }
            Inline::Cite { citations, inlines } => {
                let key = citations.first().map(|citation| citation.id.clone()).unwrap_or_default();
                if key.trim().is_empty() {
                    self.structural_loss(
                        pointer,
                        "import.pandoc-json.empty_citation",
                        "citation has no key and was dropped",
                        SemanticStatus::Unsupported,
                    )?;
                    return Ok(None);
                }
                let details_lost = citations.len() > 1
                    || citations.iter().any(|citation| {
                        !citation.prefix.is_empty() || !citation.suffix.is_empty() || citation.mode != CitationMode::Normal
                    });
                if details_lost {
                    self.structural_loss(
                        pointer,
                        "import.pandoc-json.citation_details",
                        "citation prefix, suffix, mode and extra keys have no IR field and were dropped",
                        SemanticStatus::Partial,
                    )?;
                }
                Ok(Some(IrInline::Citation { key, display: Some(self.lower_inlines(inlines, &format!("{pointer}/c/1"))?) }))
            }
            Inline::Code { attr, text } => {
                if !attr.is_empty() {
                    self.structural_loss(
                        pointer,
                        "import.pandoc-json.inline_attr",
                        "inline code attributes have no IR field and were dropped",
                        SemanticStatus::Partial,
                    )?;
                }
                Ok(Some(IrInline::InlineCode { text: text.clone() }))
            }
            Inline::Space => Ok(Some(IrInline::Text { text: " ".into() })),
            Inline::SoftBreak => Ok(Some(IrInline::Break { kind: BreakKind::Soft })),
            Inline::LineBreak => Ok(Some(IrInline::Break { kind: BreakKind::Hard })),
            Inline::Math { display, text } => {
                self.losses.push(LossMarker {
                    code: "import.pandoc-json.math_source_only".into(),
                    message: format!(
                        "math at `{pointer}` keeps its authored source because the IR has no Pandoc math lowering profile"
                    ),
                    status: SemanticStatus::Partial,
                });
                let display = match display {
                    MathType::Display => MathDisplay::Display,
                    MathType::Inline => MathDisplay::Inline,
                };
                Ok(Some(IrInline::InlineMath {
                    formula: MathFormula::unsupported(text.clone(), Some("latex".into()), display, "pandoc-json"),
                }))
            }
            Inline::RawInline { format, text } => {
                if format.eq_ignore_ascii_case("html") {
                    Ok(Some(IrInline::RawHtml { html: text.clone() }))
                }
                else {
                    self.structural_loss(
                        pointer,
                        "import.pandoc-json.raw_inline",
                        &format!("raw `{format}` inline has no IR semantics and stays visible source text"),
                        SemanticStatus::Partial,
                    )?;
                    Ok(Some(IrInline::Text { text: text.clone() }))
                }
            }
            Inline::Link { attr, inlines, target } => {
                if !attr.is_empty() {
                    self.structural_loss(
                        pointer,
                        "import.pandoc-json.link_attr",
                        "link attributes have no IR field and were dropped",
                        SemanticStatus::Partial,
                    )?;
                }
                let display = self.lower_inlines(inlines, &format!("{pointer}/c/1"))?;
                let endpoint = if target.url.is_empty() {
                    RelationEndpoint::Unresolved { label: model_inline_text(inlines) }
                }
                else {
                    RelationEndpoint::ExternalUri(target.url.clone())
                };
                Ok(Some(IrInline::Link {
                    target: endpoint,
                    display,
                    title: (!target.title.is_empty()).then(|| target.title.clone()),
                }))
            }
            Inline::Image { attr, inlines, target } => {
                if !attr.is_empty() {
                    self.structural_loss(
                        pointer,
                        "import.pandoc-json.image_attr",
                        "image attributes have no IR field and were dropped",
                        SemanticStatus::Partial,
                    )?;
                }
                Ok(Some(IrInline::Image {
                    source: target.url.clone(),
                    alt: self.lower_inlines(inlines, &format!("{pointer}/c/1"))?,
                    title: (!target.title.is_empty()).then(|| target.title.clone()),
                    asset: None,
                }))
            }
            Inline::Note(blocks) => {
                if blocks.len() == 1 && matches!(blocks[0], Block::Plain(_) | Block::Para(_)) {
                    let (Block::Plain(inlines) | Block::Para(inlines)) = &blocks[0]
                    else {
                        unreachable!()
                    };
                    let label = format!("pandoc-note-{}", self.next_note);
                    self.next_note += 1;
                    return Ok(Some(IrInline::Note {
                        label,
                        display: Some(self.lower_inlines(inlines, &format!("{pointer}/c/0/c"))?),
                    }));
                }
                self.structural_loss(
                    pointer,
                    "import.pandoc-json.note_flattened",
                    "note bodies other than a single paragraph were flattened into inline display content",
                    SemanticStatus::Partial,
                )?;
                let label = format!("pandoc-note-{}", self.next_note);
                self.next_note += 1;
                Ok(Some(IrInline::Note { label, display: Some(self.flatten_blocks(blocks, &format!("{pointer}/c"))?) }))
            }
            Inline::Span { attr, inlines } => Ok(Some(IrInline::Attributed {
                attributes: attributes(attr),
                children: self.lower_inlines(inlines, &format!("{pointer}/c/1"))?,
            })),
        }
    }

    fn styled(&mut self, style: &str, inlines: &[Inline], pointer: &str) -> Result<IrInline, FormatError> {
        Ok(IrInline::Styled { style: style.into(), children: self.lower_inlines(inlines, &format!("{pointer}/c"))? })
    }

    fn styled_with_loss(&mut self, style: &str, inlines: &[Inline], pointer: &str) -> Result<IrInline, FormatError> {
        self.structural_loss(
            pointer,
            "import.pandoc-json.inline_style",
            &format!("inline style `{style}` has no standard IR style name, so it is carried as a named style string"),
            SemanticStatus::Partial,
        )?;
        self.styled(style, inlines, pointer)
    }

    /// Flatten block content that the target IR slot can only express as inlines.
    ///
    /// Callers must already have recorded the structural loss; this helper only
    /// keeps the readable text so a cell, caption, or note never loses its body.
    fn flatten_blocks(&mut self, blocks: &[Block], array_pointer: &str) -> Result<Vec<IrInline>, FormatError> {
        let mut flattened = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            if index > 0 && !flattened.is_empty() {
                flattened.push(IrInline::Break { kind: BreakKind::Hard });
            }
            let pointer = format!("{array_pointer}/{index}");
            match block {
                Block::Plain(inlines) | Block::Para(inlines) => {
                    flattened.extend(self.lower_inlines(inlines, &format!("{pointer}/c"))?);
                }
                Block::Header { inlines, .. } => {
                    flattened.extend(self.lower_inlines(inlines, &format!("{pointer}/c/2"))?);
                }
                Block::LineBlock(lines) => {
                    for (line_index, line) in lines.iter().enumerate() {
                        if line_index > 0 {
                            flattened.push(IrInline::Break { kind: BreakKind::Hard });
                        }
                        flattened.extend(self.lower_inlines(line, &format!("{pointer}/c/{line_index}"))?);
                    }
                }
                Block::CodeBlock(_, text) => flattened.push(IrInline::InlineCode { text: text.clone() }),
                Block::RawBlock(_, text) => flattened.push(IrInline::Text { text: text.clone() }),
                Block::BlockQuote(inner) => {
                    flattened.extend(self.flatten_blocks(inner, &format!("{pointer}/c"))?);
                }
                Block::Figure { blocks: inner, .. } => {
                    flattened.extend(self.flatten_blocks(inner, &format!("{pointer}/c/2"))?);
                }
                Block::Div { blocks: inner, .. } => {
                    flattened.extend(self.flatten_blocks(inner, &format!("{pointer}/c/1"))?);
                }
                Block::OrderedList(list) => {
                    for (item_index, item) in list.items.iter().enumerate() {
                        if item_index > 0 {
                            flattened.push(IrInline::Break { kind: BreakKind::Hard });
                        }
                        flattened.extend(self.flatten_blocks(item, &format!("{pointer}/c/1/{item_index}"))?);
                    }
                }
                Block::BulletList(items) => {
                    for (item_index, item) in items.iter().enumerate() {
                        if item_index > 0 {
                            flattened.push(IrInline::Break { kind: BreakKind::Hard });
                        }
                        flattened.extend(self.flatten_blocks(item, &format!("{pointer}/c/{item_index}"))?);
                    }
                }
                Block::DefinitionList(entries) => {
                    for (entry_index, (term, definitions)) in entries.iter().enumerate() {
                        if entry_index > 0 {
                            flattened.push(IrInline::Break { kind: BreakKind::Hard });
                        }
                        flattened.extend(self.lower_inlines(term, &format!("{pointer}/c/{entry_index}/0"))?);
                        for (definition_index, definition) in definitions.iter().enumerate() {
                            flattened.extend(
                                self.flatten_blocks(definition, &format!("{pointer}/c/{entry_index}/1/{definition_index}"))?,
                            );
                        }
                    }
                }
                Block::Table(table) => {
                    flattened.push(IrInline::Text { text: blocks_text(std::slice::from_ref(block)) });
                    let _ = table;
                }
                Block::HorizontalRule => {}
            }
        }
        Ok(flattened)
    }

    /// Record a structural normalization or reject it under the strict policy.
    fn structural_loss(&mut self, pointer: &str, code: &str, message: &str, status: SemanticStatus) -> Result<(), FormatError> {
        if self.policy == PandocLossPolicy::Strict {
            return Err(FormatError::unsupported(FORMAT_ID, format!("{pointer}: `{code}`: {message}")));
        }
        self.losses.push(LossMarker { code: code.into(), message: message.into(), status });
        Ok(())
    }
}

fn attributes(attr: &Attr) -> Attributes {
    Attributes {
        id: (!attr.id.is_empty()).then(|| attr.id.clone()),
        classes: attr.classes.clone(),
        pairs: attr
            .pairs
            .iter()
            .map(|(key, value)| document_ir::AttributePair { key: key.clone(), value: value.clone() })
            .collect(),
    }
}

fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn document_id_for(label: &str) -> DocumentId {
    let mut hash = 1u64;
    for byte in label.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    DocumentId(hash)
}

/// Plain text projection used only where the target IR slot cannot hold AST.
fn model_inline_text(inlines: &[Inline]) -> String {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(value) | Inline::Code { text: value, .. } => text.push_str(value),
            Inline::Space | Inline::SoftBreak | Inline::LineBreak => text.push(' '),
            Inline::RawInline { text: value, .. } => text.push_str(value),
            Inline::Math { text: value, .. } => text.push_str(value),
            Inline::Emph(inner)
            | Inline::Underline(inner)
            | Inline::Strong(inner)
            | Inline::Strikeout(inner)
            | Inline::Superscript(inner)
            | Inline::Subscript(inner)
            | Inline::SmallCaps(inner)
            | Inline::Quoted { inlines: inner, .. }
            | Inline::Cite { inlines: inner, .. }
            | Inline::Link { inlines: inner, .. }
            | Inline::Image { inlines: inner, .. }
            | Inline::Span { inlines: inner, .. } => text.push_str(&model_inline_text(inner)),
            Inline::Note(blocks) => text.push_str(&blocks_text(blocks)),
        }
    }
    text
}

fn blocks_text(blocks: &[Block]) -> String {
    let mut text = String::new();
    for block in blocks {
        let piece = match block {
            Block::Plain(inlines) | Block::Para(inlines) | Block::Header { inlines, .. } => model_inline_text(inlines),
            Block::LineBlock(lines) => lines.iter().map(|line| model_inline_text(line)).collect::<Vec<_>>().join(" "),
            Block::CodeBlock(_, text) | Block::RawBlock(_, text) => text.clone(),
            Block::BlockQuote(inner) | Block::Figure { blocks: inner, .. } | Block::Div { blocks: inner, .. } => {
                blocks_text(inner)
            }
            Block::OrderedList(list) => list.items.iter().map(|item| blocks_text(item)).collect::<Vec<_>>().join(" "),
            Block::BulletList(items) => items.iter().map(|item| blocks_text(item)).collect::<Vec<_>>().join(" "),
            Block::DefinitionList(entries) => entries
                .iter()
                .map(|(term, definitions)| {
                    let mut entry = model_inline_text(term);
                    for definition in definitions {
                        entry.push(' ');
                        entry.push_str(&blocks_text(definition));
                    }
                    entry
                })
                .collect::<Vec<_>>()
                .join(" "),
            Block::Table(table) => {
                let mut rows = Vec::new();
                for row in &table.head.rows {
                    rows.push(row_text(row));
                }
                for body in &table.bodies {
                    for row in body.head.iter().chain(body.body.iter()) {
                        rows.push(row_text(row));
                    }
                }
                for row in &table.foot.rows {
                    rows.push(row_text(row));
                }
                rows.join(" ")
            }
            Block::HorizontalRule => String::new(),
        };
        if !piece.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&piece);
        }
    }
    text
}

fn row_text(row: &super::model::Row) -> String {
    row.cells.iter().map(|cell| blocks_text(&cell.blocks)).collect::<Vec<_>>().join(" ")
}
