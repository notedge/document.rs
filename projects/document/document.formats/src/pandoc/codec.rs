//! Strict Pandoc JSON wire codec.
//!
//! Decoding rejects unknown constructors, illegal arity, unknown top-level
//! fields and unsupported API versions with a JSON pointer. Nothing is
//! silently dropped, guessed or coerced into plain text.

use serde_json::{Map, Value};

use crate::FormatError;

use super::model::{
    API_VERSION, Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, Inline, ListNumberDelim,
    ListNumberStyle, MathType, MetaValue, OrderedList, PandocDocument, QuoteType, Row, Table, TableBody, TableFoot, TableHead,
    Target,
};

/// Format identifier used in diagnostics and the capability table.
pub const FORMAT_ID: &str = "pandoc-json";

/// Input budgets for decode and encode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PandocBudget {
    /// Maximum input or output bytes.
    pub max_bytes: usize,
    /// Maximum nested value depth.
    pub max_depth: usize,
    /// Maximum number of decoded or encoded nodes.
    pub max_nodes: usize,
}

impl Default for PandocBudget {
    fn default() -> Self {
        Self { max_bytes: 64 * 1024 * 1024, max_depth: 64, max_nodes: 2_000_000 }
    }
}

/// Decode Pandoc JSON bytes into the typed model.
pub fn decode(bytes: &[u8], budget: &PandocBudget) -> Result<PandocDocument, FormatError> {
    if bytes.len() > budget.max_bytes {
        return Err(FormatError::budget(
            FORMAT_ID,
            format!("input is {} bytes but the limit is {} bytes", bytes.len(), budget.max_bytes),
        ));
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| FormatError::parse(FORMAT_ID, format!("invalid JSON: {error}")))?;
    let mut decoder = Decoder { budget: *budget, nodes: 0 };
    decoder.decode_document(&value)
}

/// Encode the typed model into Pandoc JSON text.
pub fn encode(document: &PandocDocument, budget: &PandocBudget) -> Result<String, FormatError> {
    let mut encoder = Encoder { budget: *budget, nodes: 0 };
    let value = encoder.encode_document(document)?;
    let text = serde_json::to_string(&value)
        .map_err(|error| FormatError::parse(FORMAT_ID, format!("failed to serialize JSON: {error}")))?;
    if text.len() > budget.max_bytes {
        return Err(FormatError::budget(
            FORMAT_ID,
            format!("output is {} bytes but the limit is {} bytes", text.len(), budget.max_bytes),
        ));
    }
    Ok(text)
}

/// Encode one block as the JSON value Pandoc uses inside its parent payload.
pub(crate) fn encode_block_value(block: &Block) -> Result<Value, FormatError> {
    Encoder { budget: PandocBudget::default(), nodes: 0 }.encode_block(block, 0)
}

/// Decode one block JSON value without a surrounding document.
pub(crate) fn decode_block_value(value: &Value) -> Result<Block, FormatError> {
    Decoder { budget: PandocBudget::default(), nodes: 0 }.decode_block(value, "", 0)
}

fn decode_error(pointer: &str, reason: impl Into<String>) -> FormatError {
    FormatError::parse(FORMAT_ID, format!("{pointer}: {}", reason.into()))
}

fn join_pointer(base: &str, key: &str) -> String {
    format!("{base}/{key}")
}

fn index_pointer(base: &str, index: usize) -> String {
    format!("{base}/{index}")
}

fn as_object<'a>(value: &'a Value, pointer: &str) -> Result<&'a Map<String, Value>, FormatError> {
    value.as_object().ok_or_else(|| decode_error(pointer, "expected a JSON object"))
}

fn as_array<'a>(value: &'a Value, pointer: &str) -> Result<&'a Vec<Value>, FormatError> {
    value.as_array().ok_or_else(|| decode_error(pointer, "expected a JSON array"))
}

fn as_string<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, FormatError> {
    value.as_str().ok_or_else(|| decode_error(pointer, "expected a JSON string"))
}

fn as_i64(value: &Value, pointer: &str) -> Result<i64, FormatError> {
    value.as_i64().ok_or_else(|| decode_error(pointer, "expected an integer"))
}

fn as_f64(value: &Value, pointer: &str) -> Result<f64, FormatError> {
    value.as_f64().ok_or_else(|| decode_error(pointer, "expected a number"))
}

fn exact_arity<'a>(value: &'a Value, expected: usize, pointer: &str) -> Result<&'a [Value], FormatError> {
    let items = as_array(value, pointer)?;
    if items.len() != expected {
        return Err(decode_error(pointer, format!("expected arity {expected}, found {}", items.len())));
    }
    Ok(items)
}

fn constructor<'a>(object: &'a Map<String, Value>, pointer: &str) -> Result<&'a str, FormatError> {
    let tag = object.get("t").ok_or_else(|| decode_error(pointer, "missing constructor tag `t`"))?;
    as_string(tag, &join_pointer(pointer, "t"))
}

fn reject_extra_fields(object: &Map<String, Value>, pointer: &str) -> Result<(), FormatError> {
    for key in object.keys() {
        if key != "t" && key != "c" {
            return Err(decode_error(pointer, format!("unknown tagged-node field `{key}`")));
        }
    }
    Ok(())
}

fn payload<'a>(object: &'a Map<String, Value>, pointer: &str) -> Result<&'a Value, FormatError> {
    object.get("c").ok_or_else(|| decode_error(pointer, "missing payload field `c`"))
}

fn nullary(object: &Map<String, Value>, pointer: &str) -> Result<(), FormatError> {
    match object.get("c") {
        None | Some(Value::Null) => Ok(()),
        Some(_) => Err(decode_error(pointer, "constructor carries no payload, so `c` must be absent or null")),
    }
}

struct Decoder {
    budget: PandocBudget,
    nodes: usize,
}

impl Decoder {
    fn count_node(&mut self, pointer: &str) -> Result<(), FormatError> {
        self.nodes += 1;
        if self.nodes > self.budget.max_nodes {
            return Err(FormatError::budget(
                FORMAT_ID,
                format!("{pointer}: node count exceeds the limit of {}", self.budget.max_nodes),
            ));
        }
        Ok(())
    }

    fn check_depth(&self, depth: usize, pointer: &str) -> Result<(), FormatError> {
        if depth > self.budget.max_depth {
            return Err(FormatError::budget(
                FORMAT_ID,
                format!("{pointer}: nesting depth exceeds the limit of {}", self.budget.max_depth),
            ));
        }
        Ok(())
    }

    fn decode_document(&mut self, value: &Value) -> Result<PandocDocument, FormatError> {
        self.check_depth(0, "")?;
        let object = as_object(value, "")?;
        for key in object.keys() {
            if !matches!(key.as_str(), "pandoc-api-version" | "meta" | "blocks") {
                return Err(decode_error("", format!("unknown top-level field `{key}`")));
            }
        }
        let version = object.get("pandoc-api-version").ok_or_else(|| decode_error("", "missing `pandoc-api-version`"))?;
        let version_pointer = "/pandoc-api-version";
        let parts = exact_arity(version, 3, version_pointer)?;
        let mut decoded_version = [0_i64; 3];
        for (index, part) in parts.iter().enumerate() {
            decoded_version[index] = as_i64(part, &index_pointer(version_pointer, index))?;
        }
        if decoded_version != API_VERSION {
            return Err(decode_error(
                version_pointer,
                format!("unsupported `pandoc-api-version` {decoded_version:?}, accepted {API_VERSION:?}"),
            ));
        }
        let meta_value = object.get("meta").ok_or_else(|| decode_error("", "missing `meta`"))?;
        let meta = self.decode_meta_map(meta_value, "/meta", 1)?;
        let blocks_value = object.get("blocks").ok_or_else(|| decode_error("", "missing `blocks`"))?;
        let block_values = as_array(blocks_value, "/blocks")?;
        let mut blocks = Vec::with_capacity(block_values.len());
        for (index, block) in block_values.iter().enumerate() {
            blocks.push(self.decode_block(block, &index_pointer("/blocks", index), 1)?);
        }
        Ok(PandocDocument { api_version: decoded_version, meta, blocks })
    }

    fn decode_meta_map(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<Vec<(String, MetaValue)>, FormatError> {
        self.check_depth(depth, pointer)?;
        let object = as_object(value, pointer)?;
        let mut entries = Vec::with_capacity(object.len());
        for (key, entry) in object {
            entries.push((key.clone(), self.decode_meta(entry, &join_pointer(pointer, key), depth + 1)?));
        }
        Ok(entries)
    }

    fn decode_meta(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<MetaValue, FormatError> {
        self.count_node(pointer)?;
        self.check_depth(depth, pointer)?;
        let object = as_object(value, pointer)?;
        reject_extra_fields(object, pointer)?;
        let tag = constructor(object, pointer)?;
        let c_pointer = join_pointer(pointer, "c");
        match tag {
            "MetaMap" => {
                let value = payload(object, pointer)?;
                Ok(MetaValue::Map(self.decode_meta_map(value, &c_pointer, depth + 1)?))
            }
            "MetaList" => {
                let items = as_array(payload(object, pointer)?, &c_pointer)?;
                let mut decoded = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    decoded.push(self.decode_meta(item, &index_pointer(&c_pointer, index), depth + 1)?);
                }
                Ok(MetaValue::List(decoded))
            }
            "MetaBool" => Ok(MetaValue::Bool(
                payload(object, pointer)?.as_bool().ok_or_else(|| decode_error(&c_pointer, "expected a boolean"))?,
            )),
            "MetaString" => Ok(MetaValue::String(as_string(payload(object, pointer)?, &c_pointer)?.to_string())),
            "MetaInlines" => {
                let inlines = self.decode_inlines(payload(object, pointer)?, &c_pointer, depth + 1)?;
                Ok(MetaValue::Inlines(inlines))
            }
            "MetaBlocks" => {
                let blocks = self.decode_blocks(payload(object, pointer)?, &c_pointer, depth + 1)?;
                Ok(MetaValue::Blocks(blocks))
            }
            other => Err(decode_error(pointer, format!("unknown metadata constructor `{other}`"))),
        }
    }

    fn decode_blocks(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<Vec<Block>, FormatError> {
        self.check_depth(depth, pointer)?;
        let items = as_array(value, pointer)?;
        let mut blocks = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            blocks.push(self.decode_block(item, &index_pointer(pointer, index), depth + 1)?);
        }
        Ok(blocks)
    }

    fn decode_inlines(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<Vec<Inline>, FormatError> {
        self.check_depth(depth, pointer)?;
        let items = as_array(value, pointer)?;
        let mut inlines = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            inlines.push(self.decode_inline(item, &index_pointer(pointer, index), depth + 1)?);
        }
        Ok(inlines)
    }

    fn decode_block(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<Block, FormatError> {
        self.count_node(pointer)?;
        self.check_depth(depth, pointer)?;
        let object = as_object(value, pointer)?;
        reject_extra_fields(object, pointer)?;
        let tag = constructor(object, pointer)?;
        let c_pointer = join_pointer(pointer, "c");
        match tag {
            "Plain" => Ok(Block::Plain(self.decode_inlines(payload(object, pointer)?, &c_pointer, depth + 1)?)),
            "Para" => Ok(Block::Para(self.decode_inlines(payload(object, pointer)?, &c_pointer, depth + 1)?)),
            "LineBlock" => {
                let lines = as_array(payload(object, pointer)?, &c_pointer)?;
                let mut decoded = Vec::with_capacity(lines.len());
                for (index, line) in lines.iter().enumerate() {
                    decoded.push(self.decode_inlines(line, &index_pointer(&c_pointer, index), depth + 1)?);
                }
                Ok(Block::LineBlock(decoded))
            }
            "CodeBlock" => {
                let parts = exact_arity(payload(object, pointer)?, 2, &c_pointer)?;
                let attr = self.decode_attr(&parts[0], &index_pointer(&c_pointer, 0))?;
                let text = as_string(&parts[1], &index_pointer(&c_pointer, 1))?.to_string();
                Ok(Block::CodeBlock(attr, text))
            }
            "RawBlock" => {
                let parts = exact_arity(payload(object, pointer)?, 2, &c_pointer)?;
                Ok(Block::RawBlock(
                    as_string(&parts[0], &index_pointer(&c_pointer, 0))?.to_string(),
                    as_string(&parts[1], &index_pointer(&c_pointer, 1))?.to_string(),
                ))
            }
            "BlockQuote" => Ok(Block::BlockQuote(self.decode_blocks(payload(object, pointer)?, &c_pointer, depth + 1)?)),
            "OrderedList" => {
                let parts = exact_arity(payload(object, pointer)?, 2, &c_pointer)?;
                let parameters = exact_arity(&parts[0], 3, &index_pointer(&c_pointer, 0))?;
                let items_pointer = index_pointer(&c_pointer, 1);
                let items_value = as_array(&parts[1], &items_pointer)?;
                let mut items = Vec::with_capacity(items_value.len());
                for (index, item) in items_value.iter().enumerate() {
                    items.push(self.decode_blocks(item, &index_pointer(&items_pointer, index), depth + 1)?);
                }
                Ok(Block::OrderedList(OrderedList {
                    start: as_i64(&parameters[0], &index_pointer(&index_pointer(&c_pointer, 0), 0))?,
                    style: decode_number_style(
                        as_string(&parameters[1], &index_pointer(&index_pointer(&c_pointer, 0), 1))?,
                        &index_pointer(&index_pointer(&c_pointer, 0), 1),
                    )?,
                    delimiter: decode_number_delim(
                        as_string(&parameters[2], &index_pointer(&index_pointer(&c_pointer, 0), 2))?,
                        &index_pointer(&index_pointer(&c_pointer, 0), 2),
                    )?,
                    items,
                }))
            }
            "BulletList" => {
                let items_value = as_array(payload(object, pointer)?, &c_pointer)?;
                let mut items = Vec::with_capacity(items_value.len());
                for (index, item) in items_value.iter().enumerate() {
                    items.push(self.decode_blocks(item, &index_pointer(&c_pointer, index), depth + 1)?);
                }
                Ok(Block::BulletList(items))
            }
            "DefinitionList" => {
                let entries = as_array(payload(object, pointer)?, &c_pointer)?;
                let mut decoded = Vec::with_capacity(entries.len());
                for (index, entry) in entries.iter().enumerate() {
                    let entry_pointer = index_pointer(&c_pointer, index);
                    let parts = exact_arity(entry, 2, &entry_pointer)?;
                    let term = self.decode_inlines(&parts[0], &index_pointer(&entry_pointer, 0), depth + 1)?;
                    let definitions_value = as_array(&parts[1], &index_pointer(&entry_pointer, 1))?;
                    let definitions_pointer = index_pointer(&entry_pointer, 1);
                    let mut definitions = Vec::with_capacity(definitions_value.len());
                    for (definition_index, definition) in definitions_value.iter().enumerate() {
                        definitions.push(self.decode_blocks(
                            definition,
                            &index_pointer(&definitions_pointer, definition_index),
                            depth + 1,
                        )?);
                    }
                    decoded.push((term, definitions));
                }
                Ok(Block::DefinitionList(decoded))
            }
            "Header" => {
                let parts = exact_arity(payload(object, pointer)?, 3, &c_pointer)?;
                Ok(Block::Header {
                    level: as_i64(&parts[0], &index_pointer(&c_pointer, 0))?,
                    attr: self.decode_attr(&parts[1], &index_pointer(&c_pointer, 1))?,
                    inlines: self.decode_inlines(&parts[2], &index_pointer(&c_pointer, 2), depth + 1)?,
                })
            }
            "HorizontalRule" => {
                nullary(object, pointer)?;
                Ok(Block::HorizontalRule)
            }
            "Table" => {
                let parts = exact_arity(payload(object, pointer)?, 6, &c_pointer)?;
                Ok(Block::Table(self.decode_table(parts, &c_pointer, depth)?))
            }
            "Figure" => {
                let parts = exact_arity(payload(object, pointer)?, 3, &c_pointer)?;
                Ok(Block::Figure {
                    attr: self.decode_attr(&parts[0], &index_pointer(&c_pointer, 0))?,
                    caption: self.decode_caption(&parts[1], &index_pointer(&c_pointer, 1), depth + 1)?,
                    blocks: self.decode_blocks(&parts[2], &index_pointer(&c_pointer, 2), depth + 1)?,
                })
            }
            "Div" => {
                let parts = exact_arity(payload(object, pointer)?, 2, &c_pointer)?;
                Ok(Block::Div {
                    attr: self.decode_attr(&parts[0], &index_pointer(&c_pointer, 0))?,
                    blocks: self.decode_blocks(&parts[1], &index_pointer(&c_pointer, 1), depth + 1)?,
                })
            }
            other => Err(decode_error(pointer, format!("unknown block constructor `{other}`"))),
        }
    }

    fn decode_table(&mut self, parts: &[Value], c_pointer: &str, depth: usize) -> Result<Table, FormatError> {
        let attr = self.decode_attr(&parts[0], &index_pointer(c_pointer, 0))?;
        let caption = self.decode_caption(&parts[1], &index_pointer(c_pointer, 1), depth + 1)?;
        let colspecs_pointer = index_pointer(c_pointer, 2);
        let colspecs_value = as_array(&parts[2], &colspecs_pointer)?;
        let mut colspecs = Vec::with_capacity(colspecs_value.len());
        for (index, colspec) in colspecs_value.iter().enumerate() {
            let item_pointer = index_pointer(&colspecs_pointer, index);
            let pair = exact_arity(colspec, 2, &item_pointer)?;
            colspecs.push(ColSpec {
                alignment: decode_alignment(
                    as_string(&pair[0], &index_pointer(&item_pointer, 0))?,
                    &index_pointer(&item_pointer, 0),
                )?,
                width: as_f64(&pair[1], &index_pointer(&item_pointer, 1))?,
            });
        }
        let head = self.decode_table_head(&parts[3], &index_pointer(c_pointer, 3), depth + 1)?;
        let bodies_pointer = index_pointer(c_pointer, 4);
        let bodies_value = as_array(&parts[4], &bodies_pointer)?;
        let mut bodies = Vec::with_capacity(bodies_value.len());
        for (index, body) in bodies_value.iter().enumerate() {
            bodies.push(self.decode_table_body(body, &index_pointer(&bodies_pointer, index), depth + 1)?);
        }
        let foot = self.decode_table_foot(&parts[5], &index_pointer(c_pointer, 5), depth + 1)?;
        Ok(Table { attr, caption, colspecs, head, bodies, foot })
    }

    fn decode_caption(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<Caption, FormatError> {
        let parts = exact_arity(value, 2, pointer)?;
        let short = match &parts[0] {
            Value::Null => None,
            other => Some(self.decode_inlines(other, &index_pointer(pointer, 0), depth + 1)?),
        };
        let blocks = self.decode_blocks(&parts[1], &index_pointer(pointer, 1), depth + 1)?;
        Ok(Caption { short, blocks })
    }

    fn decode_table_head(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<TableHead, FormatError> {
        let parts = exact_arity(value, 2, pointer)?;
        let attr = self.decode_attr(&parts[0], &index_pointer(pointer, 0))?;
        let rows = self.decode_rows(&parts[1], &index_pointer(pointer, 1), depth + 1)?;
        Ok(TableHead { attr, rows })
    }

    fn decode_table_body(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<TableBody, FormatError> {
        let parts = exact_arity(value, 4, pointer)?;
        Ok(TableBody {
            attr: self.decode_attr(&parts[0], &index_pointer(pointer, 0))?,
            row_head_columns: as_i64(&parts[1], &index_pointer(pointer, 1))?,
            head: self.decode_rows(&parts[2], &index_pointer(pointer, 2), depth + 1)?,
            body: self.decode_rows(&parts[3], &index_pointer(pointer, 3), depth + 1)?,
        })
    }

    fn decode_table_foot(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<TableFoot, FormatError> {
        let parts = exact_arity(value, 2, pointer)?;
        Ok(TableFoot {
            attr: self.decode_attr(&parts[0], &index_pointer(pointer, 0))?,
            rows: self.decode_rows(&parts[1], &index_pointer(pointer, 1), depth + 1)?,
        })
    }

    fn decode_rows(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<Vec<Row>, FormatError> {
        let items = as_array(value, pointer)?;
        let mut rows = Vec::with_capacity(items.len());
        for (index, row) in items.iter().enumerate() {
            let row_pointer = index_pointer(pointer, index);
            let parts = exact_arity(row, 2, &row_pointer)?;
            let cells_pointer = index_pointer(&row_pointer, 1);
            let cells_value = as_array(&parts[1], &cells_pointer)?;
            let mut cells = Vec::with_capacity(cells_value.len());
            for (cell_index, cell) in cells_value.iter().enumerate() {
                cells.push(self.decode_cell(cell, &index_pointer(&cells_pointer, cell_index), depth + 1)?);
            }
            rows.push(Row { attr: self.decode_attr(&parts[0], &index_pointer(&row_pointer, 0))?, cells });
        }
        Ok(rows)
    }

    fn decode_cell(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<Cell, FormatError> {
        let parts = exact_arity(value, 5, pointer)?;
        Ok(Cell {
            attr: self.decode_attr(&parts[0], &index_pointer(pointer, 0))?,
            alignment: decode_alignment(as_string(&parts[1], &index_pointer(pointer, 1))?, &index_pointer(pointer, 1))?,
            row_span: as_i64(&parts[2], &index_pointer(pointer, 2))?,
            col_span: as_i64(&parts[3], &index_pointer(pointer, 3))?,
            blocks: self.decode_blocks(&parts[4], &index_pointer(pointer, 4), depth + 1)?,
        })
    }

    fn decode_attr(&mut self, value: &Value, pointer: &str) -> Result<Attr, FormatError> {
        let parts = exact_arity(value, 3, pointer)?;
        let classes_pointer = index_pointer(pointer, 1);
        let classes_value = as_array(&parts[1], &classes_pointer)?;
        let mut classes = Vec::with_capacity(classes_value.len());
        for (index, class) in classes_value.iter().enumerate() {
            classes.push(as_string(class, &index_pointer(&classes_pointer, index))?.to_string());
        }
        let pairs_pointer = index_pointer(pointer, 2);
        let pairs_value = as_array(&parts[2], &pairs_pointer)?;
        let mut pairs = Vec::with_capacity(pairs_value.len());
        for (index, pair) in pairs_value.iter().enumerate() {
            let pair_pointer = index_pointer(&pairs_pointer, index);
            let pair = exact_arity(pair, 2, &pair_pointer)?;
            pairs.push((
                as_string(&pair[0], &index_pointer(&pair_pointer, 0))?.to_string(),
                as_string(&pair[1], &index_pointer(&pair_pointer, 1))?.to_string(),
            ));
        }
        Ok(Attr { id: as_string(&parts[0], &index_pointer(pointer, 0))?.to_string(), classes, pairs })
    }

    fn decode_inline(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<Inline, FormatError> {
        self.count_node(pointer)?;
        self.check_depth(depth, pointer)?;
        let object = as_object(value, pointer)?;
        reject_extra_fields(object, pointer)?;
        let tag = constructor(object, pointer)?;
        let c_pointer = join_pointer(pointer, "c");
        match tag {
            "Str" => Ok(Inline::Str(as_string(payload(object, pointer)?, &c_pointer)?.to_string())),
            "Emph" => Ok(Inline::Emph(self.decode_inlines(payload(object, pointer)?, &c_pointer, depth + 1)?)),
            "Underline" => Ok(Inline::Underline(self.decode_inlines(payload(object, pointer)?, &c_pointer, depth + 1)?)),
            "Strong" => Ok(Inline::Strong(self.decode_inlines(payload(object, pointer)?, &c_pointer, depth + 1)?)),
            "Strikeout" => Ok(Inline::Strikeout(self.decode_inlines(payload(object, pointer)?, &c_pointer, depth + 1)?)),
            "Superscript" => Ok(Inline::Superscript(self.decode_inlines(payload(object, pointer)?, &c_pointer, depth + 1)?)),
            "Subscript" => Ok(Inline::Subscript(self.decode_inlines(payload(object, pointer)?, &c_pointer, depth + 1)?)),
            "SmallCaps" => Ok(Inline::SmallCaps(self.decode_inlines(payload(object, pointer)?, &c_pointer, depth + 1)?)),
            "Quoted" => {
                let parts = exact_arity(payload(object, pointer)?, 2, &c_pointer)?;
                Ok(Inline::Quoted {
                    kind: decode_quote_type(
                        as_string(&parts[0], &index_pointer(&c_pointer, 0))?,
                        &index_pointer(&c_pointer, 0),
                    )?,
                    inlines: self.decode_inlines(&parts[1], &index_pointer(&c_pointer, 1), depth + 1)?,
                })
            }
            "Cite" => {
                let parts = exact_arity(payload(object, pointer)?, 2, &c_pointer)?;
                let citations_pointer = index_pointer(&c_pointer, 0);
                let citations_value = as_array(&parts[0], &citations_pointer)?;
                let mut citations = Vec::with_capacity(citations_value.len());
                for (index, citation) in citations_value.iter().enumerate() {
                    citations.push(self.decode_citation(citation, &index_pointer(&citations_pointer, index), depth + 1)?);
                }
                Ok(Inline::Cite {
                    citations,
                    inlines: self.decode_inlines(&parts[1], &index_pointer(&c_pointer, 1), depth + 1)?,
                })
            }
            "Code" => {
                let parts = exact_arity(payload(object, pointer)?, 2, &c_pointer)?;
                Ok(Inline::Code {
                    attr: self.decode_attr(&parts[0], &index_pointer(&c_pointer, 0))?,
                    text: as_string(&parts[1], &index_pointer(&c_pointer, 1))?.to_string(),
                })
            }
            "Space" => {
                nullary(object, pointer)?;
                Ok(Inline::Space)
            }
            "SoftBreak" => {
                nullary(object, pointer)?;
                Ok(Inline::SoftBreak)
            }
            "LineBreak" => {
                nullary(object, pointer)?;
                Ok(Inline::LineBreak)
            }
            "Math" => {
                let parts = exact_arity(payload(object, pointer)?, 2, &c_pointer)?;
                Ok(Inline::Math {
                    display: decode_math_type(
                        as_string(&parts[0], &index_pointer(&c_pointer, 0))?,
                        &index_pointer(&c_pointer, 0),
                    )?,
                    text: as_string(&parts[1], &index_pointer(&c_pointer, 1))?.to_string(),
                })
            }
            "RawInline" => {
                let parts = exact_arity(payload(object, pointer)?, 2, &c_pointer)?;
                Ok(Inline::RawInline {
                    format: as_string(&parts[0], &index_pointer(&c_pointer, 0))?.to_string(),
                    text: as_string(&parts[1], &index_pointer(&c_pointer, 1))?.to_string(),
                })
            }
            "Link" | "Image" => {
                let parts = exact_arity(payload(object, pointer)?, 3, &c_pointer)?;
                let attr = self.decode_attr(&parts[0], &index_pointer(&c_pointer, 0))?;
                let inlines = self.decode_inlines(&parts[1], &index_pointer(&c_pointer, 1), depth + 1)?;
                let target_pointer = index_pointer(&c_pointer, 2);
                let target_parts = exact_arity(&parts[2], 2, &target_pointer)?;
                let target = Target {
                    url: as_string(&target_parts[0], &index_pointer(&target_pointer, 0))?.to_string(),
                    title: as_string(&target_parts[1], &index_pointer(&target_pointer, 1))?.to_string(),
                };
                if tag == "Link" {
                    Ok(Inline::Link { attr, inlines, target })
                }
                else {
                    Ok(Inline::Image { attr, inlines, target })
                }
            }
            "Note" => Ok(Inline::Note(self.decode_blocks(payload(object, pointer)?, &c_pointer, depth + 1)?)),
            "Span" => {
                let parts = exact_arity(payload(object, pointer)?, 2, &c_pointer)?;
                Ok(Inline::Span {
                    attr: self.decode_attr(&parts[0], &index_pointer(&c_pointer, 0))?,
                    inlines: self.decode_inlines(&parts[1], &index_pointer(&c_pointer, 1), depth + 1)?,
                })
            }
            other => Err(decode_error(pointer, format!("unknown inline constructor `{other}`"))),
        }
    }

    fn decode_citation(&mut self, value: &Value, pointer: &str, depth: usize) -> Result<Citation, FormatError> {
        self.check_depth(depth, pointer)?;
        let object = as_object(value, pointer)?;
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "citationId" | "citationPrefix" | "citationSuffix" | "citationMode" | "citationNoteNum" | "citationHash"
            ) {
                return Err(decode_error(pointer, format!("unknown citation field `{key}`")));
            }
        }
        let id = object.get("citationId").ok_or_else(|| decode_error(pointer, "missing `citationId`"))?;
        let prefix = object.get("citationPrefix").ok_or_else(|| decode_error(pointer, "missing `citationPrefix`"))?;
        let suffix = object.get("citationSuffix").ok_or_else(|| decode_error(pointer, "missing `citationSuffix`"))?;
        let mode = object.get("citationMode").ok_or_else(|| decode_error(pointer, "missing `citationMode`"))?;
        let note_num = object.get("citationNoteNum").ok_or_else(|| decode_error(pointer, "missing `citationNoteNum`"))?;
        let hash = object.get("citationHash").ok_or_else(|| decode_error(pointer, "missing `citationHash`"))?;
        Ok(Citation {
            id: as_string(id, &join_pointer(pointer, "citationId"))?.to_string(),
            prefix: self.decode_inlines(prefix, &join_pointer(pointer, "citationPrefix"), depth + 1)?,
            suffix: self.decode_inlines(suffix, &join_pointer(pointer, "citationSuffix"), depth + 1)?,
            mode: decode_citation_mode(
                as_string(mode, &join_pointer(pointer, "citationMode"))?,
                &join_pointer(pointer, "citationMode"),
            )?,
            note_num: as_i64(note_num, &join_pointer(pointer, "citationNoteNum"))?,
            hash: as_i64(hash, &join_pointer(pointer, "citationHash"))?,
        })
    }
}

fn decode_alignment(value: &str, pointer: &str) -> Result<Alignment, FormatError> {
    match value {
        "AlignLeft" => Ok(Alignment::Left),
        "AlignRight" => Ok(Alignment::Right),
        "AlignCenter" => Ok(Alignment::Center),
        "AlignDefault" => Ok(Alignment::Default),
        other => Err(decode_error(pointer, format!("unknown alignment `{other}`"))),
    }
}

fn decode_number_style(value: &str, pointer: &str) -> Result<ListNumberStyle, FormatError> {
    match value {
        "DefaultStyle" => Ok(ListNumberStyle::Default),
        "Example" => Ok(ListNumberStyle::Example),
        "Decimal" => Ok(ListNumberStyle::Decimal),
        "LowerAlpha" => Ok(ListNumberStyle::LowerAlpha),
        "UpperAlpha" => Ok(ListNumberStyle::UpperAlpha),
        "LowerRoman" => Ok(ListNumberStyle::LowerRoman),
        "UpperRoman" => Ok(ListNumberStyle::UpperRoman),
        other => Err(decode_error(pointer, format!("unknown list number style `{other}`"))),
    }
}

fn decode_number_delim(value: &str, pointer: &str) -> Result<ListNumberDelim, FormatError> {
    match value {
        "DefaultDelim" => Ok(ListNumberDelim::Default),
        "Period" => Ok(ListNumberDelim::Period),
        "OneParen" => Ok(ListNumberDelim::OneParen),
        "TwoParens" => Ok(ListNumberDelim::TwoParens),
        other => Err(decode_error(pointer, format!("unknown list number delimiter `{other}`"))),
    }
}

fn decode_quote_type(value: &str, pointer: &str) -> Result<QuoteType, FormatError> {
    match value {
        "SingleQuote" => Ok(QuoteType::Single),
        "DoubleQuote" => Ok(QuoteType::Double),
        other => Err(decode_error(pointer, format!("unknown quote type `{other}`"))),
    }
}

fn decode_math_type(value: &str, pointer: &str) -> Result<MathType, FormatError> {
    match value {
        "DisplayMath" => Ok(MathType::Display),
        "InlineMath" => Ok(MathType::Inline),
        other => Err(decode_error(pointer, format!("unknown math type `{other}`"))),
    }
}

fn decode_citation_mode(value: &str, pointer: &str) -> Result<CitationMode, FormatError> {
    match value {
        "NormalCitation" => Ok(CitationMode::Normal),
        "AuthorInText" => Ok(CitationMode::AuthorInText),
        "SuppressAuthor" => Ok(CitationMode::SuppressAuthor),
        other => Err(decode_error(pointer, format!("unknown citation mode `{other}`"))),
    }
}

struct Encoder {
    budget: PandocBudget,
    nodes: usize,
}

impl Encoder {
    fn count_node(&mut self) -> Result<(), FormatError> {
        self.nodes += 1;
        if self.nodes > self.budget.max_nodes {
            return Err(FormatError::budget(FORMAT_ID, format!("node count exceeds the limit of {}", self.budget.max_nodes)));
        }
        Ok(())
    }

    /// Enforce the same conservative nesting measure the decoder uses.
    ///
    /// Each container level counts both its array and its node, so a document
    /// that passes decode at a given `max_depth` also passes encode at the same
    /// limit.
    fn check_depth(&self, depth: usize) -> Result<(), FormatError> {
        if depth > self.budget.max_depth {
            return Err(FormatError::budget(
                FORMAT_ID,
                format!("nesting depth {depth} exceeds the limit of {}", self.budget.max_depth),
            ));
        }
        Ok(())
    }

    fn encode_document(&mut self, document: &PandocDocument) -> Result<Value, FormatError> {
        if document.api_version != API_VERSION {
            return Err(FormatError::parse(
                FORMAT_ID,
                format!("cannot encode unsupported `pandoc-api-version` {:?}", document.api_version),
            ));
        }
        self.check_depth(0)?;
        let mut object = Map::new();
        object.insert(
            "pandoc-api-version".into(),
            Value::Array(vec![
                Value::from(document.api_version[0]),
                Value::from(document.api_version[1]),
                Value::from(document.api_version[2]),
            ]),
        );
        object.insert("meta".into(), self.encode_meta_map(&document.meta, 1)?);
        object.insert("blocks".into(), Value::Array(self.encode_blocks(&document.blocks, 0)?));
        Ok(Value::Object(object))
    }

    fn encode_meta_map(&mut self, entries: &[(String, MetaValue)], depth: usize) -> Result<Value, FormatError> {
        self.check_depth(depth)?;
        let mut object = Map::new();
        for (key, value) in entries {
            object.insert(key.clone(), self.encode_meta(value, depth + 1)?);
        }
        Ok(Value::Object(object))
    }

    fn encode_meta(&mut self, value: &MetaValue, depth: usize) -> Result<Value, FormatError> {
        self.count_node()?;
        self.check_depth(depth)?;
        let (tag, payload) = match value {
            MetaValue::Map(entries) => ("MetaMap", self.encode_meta_map(entries, depth + 1)?),
            MetaValue::List(items) => {
                let mut encoded = Vec::with_capacity(items.len());
                for item in items {
                    encoded.push(self.encode_meta(item, depth + 1)?);
                }
                ("MetaList", Value::Array(encoded))
            }
            MetaValue::Bool(value) => ("MetaBool", Value::Bool(*value)),
            MetaValue::String(value) => ("MetaString", Value::String(value.clone())),
            MetaValue::Inlines(inlines) => ("MetaInlines", Value::Array(self.encode_inlines(inlines, depth + 1)?)),
            MetaValue::Blocks(blocks) => ("MetaBlocks", Value::Array(self.encode_blocks(blocks, depth + 1)?)),
        };
        Ok(tagged(tag, payload))
    }

    fn encode_blocks(&mut self, blocks: &[Block], depth: usize) -> Result<Vec<Value>, FormatError> {
        self.check_depth(depth)?;
        let mut encoded = Vec::with_capacity(blocks.len());
        for block in blocks {
            encoded.push(self.encode_block(block, depth + 1)?);
        }
        Ok(encoded)
    }

    fn encode_inlines(&mut self, inlines: &[Inline], depth: usize) -> Result<Vec<Value>, FormatError> {
        self.check_depth(depth)?;
        let mut encoded = Vec::with_capacity(inlines.len());
        for inline in inlines {
            encoded.push(self.encode_inline(inline, depth + 1)?);
        }
        Ok(encoded)
    }

    fn encode_block(&mut self, block: &Block, depth: usize) -> Result<Value, FormatError> {
        self.count_node()?;
        self.check_depth(depth)?;
        match block {
            Block::Plain(inlines) => Ok(tagged("Plain", Value::Array(self.encode_inlines(inlines, depth + 1)?))),
            Block::Para(inlines) => Ok(tagged("Para", Value::Array(self.encode_inlines(inlines, depth + 1)?))),
            Block::LineBlock(lines) => {
                let mut encoded = Vec::with_capacity(lines.len());
                for line in lines {
                    encoded.push(Value::Array(self.encode_inlines(line, depth + 1)?));
                }
                Ok(tagged("LineBlock", Value::Array(encoded)))
            }
            Block::CodeBlock(attr, text) => {
                Ok(tagged("CodeBlock", Value::Array(vec![self.encode_attr(attr), Value::String(text.clone())])))
            }
            Block::RawBlock(format, text) => {
                Ok(tagged("RawBlock", Value::Array(vec![Value::String(format.clone()), Value::String(text.clone())])))
            }
            Block::BlockQuote(blocks) => Ok(tagged("BlockQuote", Value::Array(self.encode_blocks(blocks, depth + 1)?))),
            Block::OrderedList(list) => {
                let parameters = Value::Array(vec![
                    Value::from(list.start),
                    Value::String(encode_number_style(list.style).into()),
                    Value::String(encode_number_delim(list.delimiter).into()),
                ]);
                let mut items = Vec::with_capacity(list.items.len());
                for item in &list.items {
                    items.push(Value::Array(self.encode_blocks(item, depth + 1)?));
                }
                Ok(tagged("OrderedList", Value::Array(vec![parameters, Value::Array(items)])))
            }
            Block::BulletList(items) => {
                let mut encoded = Vec::with_capacity(items.len());
                for item in items {
                    encoded.push(Value::Array(self.encode_blocks(item, depth + 1)?));
                }
                Ok(tagged("BulletList", Value::Array(encoded)))
            }
            Block::DefinitionList(entries) => {
                let mut encoded = Vec::with_capacity(entries.len());
                for (term, definitions) in entries {
                    let mut definition_values = Vec::with_capacity(definitions.len());
                    for definition in definitions {
                        definition_values.push(Value::Array(self.encode_blocks(definition, depth + 1)?));
                    }
                    encoded.push(Value::Array(vec![
                        Value::Array(self.encode_inlines(term, depth + 1)?),
                        Value::Array(definition_values),
                    ]));
                }
                Ok(tagged("DefinitionList", Value::Array(encoded)))
            }
            Block::Header { level, attr, inlines } => Ok(tagged(
                "Header",
                Value::Array(vec![
                    Value::from(*level),
                    self.encode_attr(attr),
                    Value::Array(self.encode_inlines(inlines, depth + 1)?),
                ]),
            )),
            Block::HorizontalRule => Ok(tagged_nullary("HorizontalRule")),
            Block::Table(table) => Ok(tagged("Table", self.encode_table(table, depth + 1)?)),
            Block::Figure { attr, caption, blocks } => Ok(tagged(
                "Figure",
                Value::Array(vec![
                    self.encode_attr(attr),
                    self.encode_caption(caption, depth + 1)?,
                    Value::Array(self.encode_blocks(blocks, depth + 1)?),
                ]),
            )),
            Block::Div { attr, blocks } => Ok(tagged(
                "Div",
                Value::Array(vec![self.encode_attr(attr), Value::Array(self.encode_blocks(blocks, depth + 1)?)]),
            )),
        }
    }

    fn encode_table(&mut self, table: &Table, depth: usize) -> Result<Value, FormatError> {
        self.check_depth(depth)?;
        let mut colspecs = Vec::with_capacity(table.colspecs.len());
        for colspec in &table.colspecs {
            colspecs.push(Value::Array(vec![
                Value::String(encode_alignment(colspec.alignment).into()),
                Value::from(colspec.width),
            ]));
        }
        let mut bodies = Vec::with_capacity(table.bodies.len());
        for body in &table.bodies {
            bodies.push(Value::Array(vec![
                self.encode_attr(&body.attr),
                Value::from(body.row_head_columns),
                Value::Array(self.encode_rows(&body.head, depth + 1)?),
                Value::Array(self.encode_rows(&body.body, depth + 1)?),
            ]));
        }
        Ok(Value::Array(vec![
            self.encode_attr(&table.attr),
            self.encode_caption(&table.caption, depth + 1)?,
            Value::Array(colspecs),
            Value::Array(vec![
                self.encode_attr(&table.head.attr),
                Value::Array(self.encode_rows(&table.head.rows, depth + 1)?),
            ]),
            Value::Array(bodies),
            Value::Array(vec![
                self.encode_attr(&table.foot.attr),
                Value::Array(self.encode_rows(&table.foot.rows, depth + 1)?),
            ]),
        ]))
    }

    fn encode_caption(&mut self, caption: &Caption, depth: usize) -> Result<Value, FormatError> {
        let short = match &caption.short {
            Some(inlines) => Value::Array(self.encode_inlines(inlines, depth + 1)?),
            None => Value::Null,
        };
        Ok(Value::Array(vec![short, Value::Array(self.encode_blocks(&caption.blocks, depth + 1)?)]))
    }

    fn encode_rows(&mut self, rows: &[Row], depth: usize) -> Result<Vec<Value>, FormatError> {
        self.check_depth(depth)?;
        let mut encoded = Vec::with_capacity(rows.len());
        for row in rows {
            let mut cells = Vec::with_capacity(row.cells.len());
            for cell in &row.cells {
                cells.push(Value::Array(vec![
                    self.encode_attr(&cell.attr),
                    Value::String(encode_alignment(cell.alignment).into()),
                    Value::from(cell.row_span),
                    Value::from(cell.col_span),
                    Value::Array(self.encode_blocks(&cell.blocks, depth + 1)?),
                ]));
            }
            encoded.push(Value::Array(vec![self.encode_attr(&row.attr), Value::Array(cells)]));
        }
        Ok(encoded)
    }

    fn encode_attr(&self, attr: &Attr) -> Value {
        Value::Array(vec![
            Value::String(attr.id.clone()),
            Value::Array(attr.classes.iter().cloned().map(Value::String).collect()),
            Value::Array(
                attr.pairs
                    .iter()
                    .map(|(key, value)| Value::Array(vec![Value::String(key.clone()), Value::String(value.clone())]))
                    .collect(),
            ),
        ])
    }

    fn encode_inline(&mut self, inline: &Inline, depth: usize) -> Result<Value, FormatError> {
        self.count_node()?;
        self.check_depth(depth)?;
        match inline {
            Inline::Str(text) => Ok(tagged("Str", Value::String(text.clone()))),
            Inline::Emph(inlines) => Ok(tagged("Emph", Value::Array(self.encode_inlines(inlines, depth + 1)?))),
            Inline::Underline(inlines) => Ok(tagged("Underline", Value::Array(self.encode_inlines(inlines, depth + 1)?))),
            Inline::Strong(inlines) => Ok(tagged("Strong", Value::Array(self.encode_inlines(inlines, depth + 1)?))),
            Inline::Strikeout(inlines) => Ok(tagged("Strikeout", Value::Array(self.encode_inlines(inlines, depth + 1)?))),
            Inline::Superscript(inlines) => Ok(tagged("Superscript", Value::Array(self.encode_inlines(inlines, depth + 1)?))),
            Inline::Subscript(inlines) => Ok(tagged("Subscript", Value::Array(self.encode_inlines(inlines, depth + 1)?))),
            Inline::SmallCaps(inlines) => Ok(tagged("SmallCaps", Value::Array(self.encode_inlines(inlines, depth + 1)?))),
            Inline::Quoted { kind, inlines } => Ok(tagged(
                "Quoted",
                Value::Array(vec![
                    Value::String(encode_quote_type(*kind).into()),
                    Value::Array(self.encode_inlines(inlines, depth + 1)?),
                ]),
            )),
            Inline::Cite { citations, inlines } => {
                let mut encoded = Vec::with_capacity(citations.len());
                for citation in citations {
                    encoded.push(self.encode_citation(citation, depth + 1)?);
                }
                Ok(tagged(
                    "Cite",
                    Value::Array(vec![Value::Array(encoded), Value::Array(self.encode_inlines(inlines, depth + 1)?)]),
                ))
            }
            Inline::Code { attr, text } => {
                Ok(tagged("Code", Value::Array(vec![self.encode_attr(attr), Value::String(text.clone())])))
            }
            Inline::Space => Ok(tagged_nullary("Space")),
            Inline::SoftBreak => Ok(tagged_nullary("SoftBreak")),
            Inline::LineBreak => Ok(tagged_nullary("LineBreak")),
            Inline::Math { display, text } => Ok(tagged(
                "Math",
                Value::Array(vec![Value::String(encode_math_type(*display).into()), Value::String(text.clone())]),
            )),
            Inline::RawInline { format, text } => {
                Ok(tagged("RawInline", Value::Array(vec![Value::String(format.clone()), Value::String(text.clone())])))
            }
            Inline::Link { attr, inlines, target } => Ok(tagged(
                "Link",
                Value::Array(vec![
                    self.encode_attr(attr),
                    Value::Array(self.encode_inlines(inlines, depth + 1)?),
                    encode_target(target),
                ]),
            )),
            Inline::Image { attr, inlines, target } => Ok(tagged(
                "Image",
                Value::Array(vec![
                    self.encode_attr(attr),
                    Value::Array(self.encode_inlines(inlines, depth + 1)?),
                    encode_target(target),
                ]),
            )),
            Inline::Note(blocks) => Ok(tagged("Note", Value::Array(self.encode_blocks(blocks, depth + 1)?))),
            Inline::Span { attr, inlines } => Ok(tagged(
                "Span",
                Value::Array(vec![self.encode_attr(attr), Value::Array(self.encode_inlines(inlines, depth + 1)?)]),
            )),
        }
    }

    fn encode_citation(&mut self, citation: &Citation, depth: usize) -> Result<Value, FormatError> {
        self.check_depth(depth)?;
        let mut object = Map::new();
        object.insert("citationId".into(), Value::String(citation.id.clone()));
        object.insert("citationPrefix".into(), Value::Array(self.encode_inlines(&citation.prefix, depth + 1)?));
        object.insert("citationSuffix".into(), Value::Array(self.encode_inlines(&citation.suffix, depth + 1)?));
        object.insert("citationMode".into(), Value::String(encode_citation_mode(citation.mode).into()));
        object.insert("citationNoteNum".into(), Value::from(citation.note_num));
        object.insert("citationHash".into(), Value::from(citation.hash));
        Ok(Value::Object(object))
    }
}

fn tagged(tag: &str, payload: Value) -> Value {
    let mut object = Map::new();
    object.insert("t".into(), Value::String(tag.into()));
    object.insert("c".into(), payload);
    Value::Object(object)
}

fn tagged_nullary(tag: &str) -> Value {
    let mut object = Map::new();
    object.insert("t".into(), Value::String(tag.into()));
    Value::Object(object)
}

fn encode_target(target: &Target) -> Value {
    Value::Array(vec![Value::String(target.url.clone()), Value::String(target.title.clone())])
}

fn encode_alignment(value: Alignment) -> &'static str {
    match value {
        Alignment::Left => "AlignLeft",
        Alignment::Right => "AlignRight",
        Alignment::Center => "AlignCenter",
        Alignment::Default => "AlignDefault",
    }
}

fn encode_number_style(value: ListNumberStyle) -> &'static str {
    match value {
        ListNumberStyle::Default => "DefaultStyle",
        ListNumberStyle::Example => "Example",
        ListNumberStyle::Decimal => "Decimal",
        ListNumberStyle::LowerAlpha => "LowerAlpha",
        ListNumberStyle::UpperAlpha => "UpperAlpha",
        ListNumberStyle::LowerRoman => "LowerRoman",
        ListNumberStyle::UpperRoman => "UpperRoman",
    }
}

fn encode_number_delim(value: ListNumberDelim) -> &'static str {
    match value {
        ListNumberDelim::Default => "DefaultDelim",
        ListNumberDelim::Period => "Period",
        ListNumberDelim::OneParen => "OneParen",
        ListNumberDelim::TwoParens => "TwoParens",
    }
}

fn encode_quote_type(value: QuoteType) -> &'static str {
    match value {
        QuoteType::Single => "SingleQuote",
        QuoteType::Double => "DoubleQuote",
    }
}

fn encode_math_type(value: MathType) -> &'static str {
    match value {
        MathType::Display => "DisplayMath",
        MathType::Inline => "InlineMath",
    }
}

fn encode_citation_mode(value: CitationMode) -> &'static str {
    match value {
        CitationMode::Normal => "NormalCitation",
        CitationMode::AuthorInText => "AuthorInText",
        CitationMode::SuppressAuthor => "SuppressAuthor",
    }
}
