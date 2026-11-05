//! Typst import via `oak-typst`.
//!
//! The lowering keeps markup semantics the IR can express: headings,
//! paragraphs, strong/emphasis spans, lists, math source, raw code, and links.
//! Dynamic content (`#` expressions, function calls, set/show rules) and any
//! structure the AST cannot express stay explicit opaque blocks or recorded
//! loss markers.
//!
//! Lowering is line-oriented: heading, list, and enum spans can extend past the
//! line they start on, so the lowerer resumes at the first meaningful line
//! boundary and reparses the remaining source rather than treating the whole
//! tail as heading or list content.

use std::path::Path;

use document_ir::{
    Block, DocumentGraph, DocumentId, Inline, ListItem, LossMarker, NodeId, RelationEndpoint, SemanticStatus, SourceSpan,
};
use oak_core::{Builder, SourceText, parser::session::ParseSession};
use oak_typst::{
    TypstBuilder, TypstLanguage,
    ast::{TypstHeading, TypstItem, TypstLink, TypstRoot},
};

use super::common::register_image_assets;
use crate::FormatError;

/// Import a Typst file from disk into `notedown-ir`.
pub fn import_typst(path: impl AsRef<Path>) -> Result<DocumentGraph, FormatError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path)
        .map_err(|error| FormatError::invalid_input(format!("failed to read {}: {error}", path.display())))?;
    import_typst_bytes(&path.display().to_string(), &text)
}

/// Import Typst source text into `notedown-ir`.
pub fn import_typst_bytes(label: &str, text: &str) -> Result<DocumentGraph, FormatError> {
    let root = parse_typst(label, text)?;
    Ok(lower_typst_root(&root, text, document_id_for(label)))
}

fn lower_typst_root(root: &TypstRoot, text: &str, id: DocumentId) -> DocumentGraph {
    let mut lowerer = TypstLowerer { graph: DocumentGraph::new(id), source: text, cursor: 0, line_breaks: 0 };
    let mut sink = Sink::document();
    lowerer.lower_root(&root.items, text.len(), &mut sink);
    sink.flush(&mut lowerer.graph);

    if lowerer.graph.blocks.is_empty() {
        lowerer.loss(
            "import.typst.empty_document",
            "Typst source produced no supported semantic blocks",
            SemanticStatus::Partial,
        );
    }
    register_image_assets(&mut lowerer.graph);
    lowerer.graph
}

fn parse_typst(label: &str, text: &str) -> Result<TypstRoot, FormatError> {
    let source = SourceText::new(text);
    let language = TypstLanguage::default();
    let builder = TypstBuilder::new(&language);
    let mut session = ParseSession::<TypstLanguage>::default();
    builder.build(&source, &[], &mut session).result.map_err(|error| FormatError::parse("typst", format!("{label}: {error}")))
}

fn document_id_for(label: &str) -> DocumentId {
    let mut hash = 1u64;
    for byte in label.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    DocumentId(hash)
}

struct Sink {
    inlines: Vec<Inline>,
}

impl Sink {
    fn document() -> Self {
        Self { inlines: Vec::new() }
    }

    fn flush(&mut self, graph: &mut DocumentGraph) {
        trim_inlines(&mut self.inlines);
        if self.inlines.is_empty() {
            return;
        }
        let inlines = std::mem::take(&mut self.inlines);
        graph.push_block(Block::Paragraph { content: inlines });
    }
}

struct TypstLowerer<'a> {
    graph: DocumentGraph,
    source: &'a str,
    cursor: usize,
    line_breaks: usize,
}

impl<'a> TypstLowerer<'a> {
    fn raw(&self, start: usize, end: usize) -> &'a str {
        self.source.get(start.min(self.source.len())..end.min(self.source.len())).unwrap_or("")
    }

    fn loss(&mut self, code: &str, message: impl Into<String>, status: SemanticStatus) {
        self.graph.push_loss(LossMarker { code: code.into(), message: message.into(), status });
    }

    fn loss_at(&mut self, code: &str, message: impl Into<String>, status: SemanticStatus, span: Option<SourceSpan>) {
        self.graph.push_loss_at(LossMarker { code: code.into(), message: message.into(), status }, span);
    }

    fn push_block(&mut self, sink: &mut Sink, block: Block) {
        sink.flush(&mut self.graph);
        self.graph.push_block(block);
    }

    fn push_block_at(&mut self, sink: &mut Sink, block: Block, span: Option<SourceSpan>) {
        sink.flush(&mut self.graph);
        self.graph.push_block_at(block, span);
    }

    fn lower_root(&mut self, items: &[TypstItem], region_end: usize, sink: &mut Sink) {
        let mut index = 0;
        while index < items.len() {
            if let Some((lines, _, end)) = self.list_ahead() {
                self.lower_list_items(lines, sink);
                self.cursor = end;
                self.lower_tail(end, region_end, sink);
                return;
            }
            match &items[index] {
                TypstItem::Heading(heading) => {
                    if self.lower_heading(heading, sink) {
                        return;
                    }
                    index += 1;
                }
                TypstItem::ListItem(_) | TypstItem::EnumItem(_) => {
                    let ordered = matches!(items[index], TypstItem::EnumItem(_));
                    let mut collected = Vec::new();
                    let mut tail = None;
                    while index < items.len()
                        && matches!(items[index], TypstItem::EnumItem(_)) == ordered
                        && matches!(items[index], TypstItem::ListItem(_) | TypstItem::EnumItem(_))
                    {
                        let span = typst_span(&items[index]);
                        if let Some(span) = span {
                            let raw = self.raw(span.start, span.end);
                            let (mut lines, line_tail) = split_list_lines(raw, ordered);
                            collected.append(&mut lines);
                            tail = line_tail.map(|offset| span.start + offset);
                            self.cursor = span.end;
                        }
                        index += 1;
                        if tail.is_some() {
                            break;
                        }
                    }
                    self.lower_list_items(collected, sink);
                    if let Some(tail_start) = tail {
                        self.lower_tail(tail_start, region_end, sink);
                        return;
                    }
                }
                TypstItem::Quote(quote) => {
                    if let Some(segments) = self.link_quote(quote) {
                        self.line_breaks = 0;
                        self.cursor = quote.span.end;
                        self.push_link_segments(segments, &mut sink.inlines);
                        index += 1;
                        continue;
                    }
                    if self.lower_expression(quote, region_end, sink) {
                        return;
                    }
                    index += 1;
                }
                TypstItem::Raw(_) => {
                    let mut raw = String::new();
                    while index < items.len() {
                        let TypstItem::Raw(chunk) = &items[index]
                        else {
                            break;
                        };
                        raw.push_str(chunk);
                        index += 1;
                    }
                    self.lower_raw(&raw, sink);
                }
                _ => {
                    self.lower_inline_item(&items[index], sink);
                    index += 1;
                }
            }
        }
    }

    fn lower_inline_item(&mut self, item: &TypstItem, sink: &mut Sink) {
        match item {
            TypstItem::Text(text) => {
                if is_comment(text) {
                    return;
                }
                self.advance_text(text);
                self.line_breaks = 0;
                self.push_text(text, sink);
                self.recover_line_comment_url(sink);
            }
            TypstItem::Space => {
                let newlines = self.advance_space();
                if newlines > 0 {
                    self.line_breaks += newlines;
                }
                if self.line_breaks >= 2 {
                    sink.flush(&mut self.graph);
                    self.line_breaks = 0;
                }
                else if !sink.inlines.is_empty() {
                    push_text_inlines(&mut sink.inlines, " ");
                }
            }
            TypstItem::Parbreak => sink.flush(&mut self.graph),
            TypstItem::Strong(root) => {
                let (children, trailing_space) = self.styled_children(root, '*');
                self.cursor = root.span.end;
                sink.inlines.push(Inline::Styled { style: "bold".into(), children });
                if trailing_space {
                    push_text_inlines(&mut sink.inlines, " ");
                }
                self.line_breaks = 0;
            }
            TypstItem::Emphasis(root) => {
                let (children, trailing_space) = self.styled_children(root, '_');
                self.cursor = root.span.end;
                sink.inlines.push(Inline::Styled { style: "italic".into(), children });
                if trailing_space {
                    push_text_inlines(&mut sink.inlines, " ");
                }
                self.line_breaks = 0;
            }
            TypstItem::Math(root) => {
                let (content, display) = math_content(self.raw(root.span.start, root.span.end));
                self.cursor = root.span.end;
                self.line_breaks = 0;
                if display {
                    let formula = document_ir::MathFormula::unsupported(
                        content,
                        Some("typst".into()),
                        document_ir::MathDisplay::Display,
                        "typst-math",
                    );
                    self.push_block(sink, Block::Math { formula });
                }
                else {
                    let formula = document_ir::MathFormula::unsupported(
                        content,
                        Some("typst".into()),
                        document_ir::MathDisplay::Inline,
                        "typst-math",
                    );
                    sink.inlines.push(Inline::InlineMath { formula });
                }
            }
            TypstItem::Link(link) => {
                self.line_breaks = 0;
                sink.inlines.push(self.link_inline(link));
            }
            TypstItem::Block(root) => self.lower_root(&root.items, root.span.end, sink),
            TypstItem::Raw(_)
            | TypstItem::Heading(_)
            | TypstItem::ListItem(_)
            | TypstItem::EnumItem(_)
            | TypstItem::Quote(_) => {
                unreachable!()
            }
        }
    }

    fn lower_raw(&mut self, raw: &str, sink: &mut Sink) {
        self.advance_text(raw);
        self.line_breaks = 0;
        let ticks = raw.trim().chars().take_while(|character| *character == '`').count();
        let (content, language) = raw_content(raw);
        if content.trim().is_empty() {
            return;
        }
        if ticks < 3 {
            sink.inlines.push(Inline::InlineCode { text: content });
            return;
        }
        self.push_block(sink, Block::Code { language, content });
    }

    fn styled_children(&mut self, root: &TypstRoot, delimiter: char) -> (Vec<Inline>, bool) {
        let raw = self.raw(root.span.start, root.span.end);
        let trailing_space = raw.ends_with(char::is_whitespace);
        let inner = raw.trim();
        let inner = inner.strip_prefix(delimiter).unwrap_or(inner);
        let inner = inner.strip_suffix(delimiter).unwrap_or(inner);
        (self.lower_inline_fragment(inner.trim()), trailing_space)
    }

    fn recover_line_comment_url(&mut self, sink: &mut Sink) {
        let scheme_terminated =
            matches!(sink.inlines.last(), Some(Inline::Text { text }) if text.ends_with("http:") || text.ends_with("https:"));
        if !scheme_terminated {
            return;
        }
        let rest = self.source.get(self.cursor..).unwrap_or("");
        if !rest.starts_with("//") {
            return;
        }
        let end = rest.find('\n').unwrap_or(rest.len());
        let tail = rest[..end].trim_end_matches('\r');
        if tail.len() <= 2 {
            return;
        }
        push_inline_text(&mut sink.inlines, tail);
        self.cursor += tail.len();
    }

    fn lower_heading(&mut self, heading: &TypstHeading, sink: &mut Sink) -> bool {
        let span = heading.content.span;
        let raw = self.raw(span.start, span.end);
        let line_end = raw.find('\n').unwrap_or(raw.len());
        let line = raw[..line_end].trim_end_matches('\r');
        let marker = line.chars().take_while(|character| *character == '=').count();
        let level = marker.max(1) as u8;
        let title_source = if marker == 0 { line.trim() } else { line[marker..].trim() };
        let title = self.lower_inline_fragment(title_source);
        self.push_block(sink, Block::Section { level, title, children: Vec::new() });
        self.line_breaks = 0;
        self.cursor = span.start + line_end;
        if line_end < raw.len() && !raw[line_end..].trim().is_empty() {
            self.lower_tail(span.start + line_end + 1, span.end, sink);
            return true;
        }
        self.cursor = span.end;
        false
    }

    fn lower_expression(&mut self, quote: &TypstRoot, region_end: usize, sink: &mut Sink) -> bool {
        let span = quote.span;
        let limit = region_end.min(self.source.len());
        match scan_dynamic_group(self.source, span.start, limit) {
            DynamicGroup::Closed { end } if end > span.end => return self.lower_dynamic_group(span.start, end, limit, sink),
            DynamicGroup::Unclosed { end } => return self.lower_unclosed_group(span.start, end, sink),
            _ => {}
        }
        let raw = self.raw(span.start, span.end);
        let line_end = raw.find('\n').unwrap_or(raw.len());
        let line = raw[..line_end].trim();
        let kind = expression_kind(line);
        let line_span = SourceSpan::new(span.start, span.start + line_end);
        self.loss_at(
            "import.typst.dynamic_content",
            format!("Typst dynamic expression kept opaque: {line}"),
            SemanticStatus::Unsupported,
            line_span,
        );
        // The HTML projection skips `typst.*` opaque kinds, so the node carries
        // the same refusal status as the marker instead of claiming a partial
        // mapping that readers never see.
        self.push_block(sink, Block::Opaque { kind, payload_hint: line.to_string(), status: SemanticStatus::Unsupported });
        self.line_breaks = 0;
        self.cursor = span.start + line_end;
        if line_end < raw.len() && !raw[line_end..].trim().is_empty() {
            self.lower_tail(span.start + line_end + 1, span.end, sink);
            return true;
        }
        self.cursor = span.end;
        false
    }

    /// Keep a balanced dynamic group that continues past the line the parser
    /// captured as exactly one opaque block.
    ///
    /// The Typst parser stops a `#` expression at the first line break unless it
    /// opens a content bracket on the same line, so the argument list of a
    /// multi-line `#figure(...)` is authored as separate root items. Lowering
    /// each of them would print the construct body as body text. Instead the
    /// whole balanced group is retained once and the source after the group is
    /// lowered again from its closing bracket.
    fn lower_dynamic_group(&mut self, start: usize, end: usize, limit: usize, sink: &mut Sink) -> bool {
        let payload = self.raw(start, end).trim().to_string();
        let head = payload.lines().next().unwrap_or_default().trim().to_string();
        let kind = expression_kind(&head);
        let lines = payload.lines().count();
        let span = SourceSpan::new(start, end);
        self.loss_at(
            "import.typst.dynamic_content",
            format!("Typst dynamic group kept opaque across {lines} lines: {head}"),
            SemanticStatus::Unsupported,
            span,
        );
        self.push_block_at(sink, Block::Opaque { kind, payload_hint: payload, status: SemanticStatus::Unsupported }, span);
        self.line_breaks = 0;
        self.cursor = end;
        if end < limit {
            self.lower_tail(end, limit, sink);
        }
        true
    }

    /// Record a structured diagnostic for a dynamic group whose brackets never
    /// balance before the region ends.
    fn lower_unclosed_group(&mut self, start: usize, end: usize, sink: &mut Sink) -> bool {
        let rest = self.raw(start, end).trim().to_string();
        let head = rest.lines().next().unwrap_or_default().trim().to_string();
        let kind = expression_kind(&head);
        let span = SourceSpan::new(start, end);
        self.loss_at(
            "import.typst.unterminated_dynamic_group",
            format!("Typst dynamic group is not balanced before the region ends: {head}"),
            SemanticStatus::Unsupported,
            span,
        );
        self.push_block_at(sink, Block::Opaque { kind, payload_hint: rest, status: SemanticStatus::Unsupported }, span);
        self.line_breaks = 0;
        self.cursor = end;
        true
    }

    fn lower_list_items(&mut self, lines: Vec<ListLine>, sink: &mut Sink) {
        if lines.is_empty() {
            self.loss("import.typst.empty_list", "list markup contains no item line", SemanticStatus::Partial);
            return;
        }
        // A list always starts a new block, so pending paragraph inlines flush
        // before the first level is created.
        sink.flush(&mut self.graph);
        let mut stack: Vec<OpenTypstList> = Vec::new();
        for line in lines {
            let content = self.lower_inline_fragment(&line.content);
            while stack.last().is_some_and(|open| open.indent > line.indent) {
                stack.pop();
            }
            let continuation = match stack.last() {
                Some(open) if open.indent == line.indent => {
                    if open.ordered == line.ordered {
                        Some(open.id)
                    }
                    else {
                        stack.pop();
                        None
                    }
                }
                _ => None,
            };
            let id = match continuation {
                Some(id) => id,
                None => {
                    let id = self.graph.push_block(Block::List { ordered: line.ordered, items: Vec::new() });
                    if let Some(parent) = stack.last() {
                        attach_nested_list(&mut self.graph, parent.id, id);
                    }
                    stack.push(OpenTypstList { indent: line.indent, ordered: line.ordered, id });
                    id
                }
            };
            if let Some(items) = list_items_mut(&mut self.graph, id) {
                // Typst numbered lists without an explicit marker number still
                // count from one, so the graph needs the ordinal the source
                // leaves implicit.
                let number = line.number.or_else(|| line.ordered.then(|| items.len() as u32 + 1));
                items.push(ListItem {
                    content,
                    children: Vec::new(),
                    number,
                    is_task: false,
                    is_checked: None,
                });
            }
        }
    }

    fn lower_tail(&mut self, start: usize, end: usize, sink: &mut Sink) {
        let tail = self.raw(start, end);
        if tail.trim().is_empty() {
            return;
        }
        let Ok(root) = parse_typst("<tail>", tail)
        else {
            self.loss_at(
                "import.typst.fragment_parse_failed",
                "reparsed Typst tail could not be parsed",
                SemanticStatus::Partial,
                SourceSpan::new(start, end),
            );
            return;
        };
        let mut inner = TypstLowerer {
            graph: std::mem::replace(&mut self.graph, DocumentGraph::new(DocumentId(0))),
            source: tail,
            cursor: 0,
            line_breaks: 0,
        };
        inner.lower_root(&root.items, tail.len(), sink);
        self.graph = inner.graph;
    }

    fn lower_inline_fragment(&mut self, fragment: &str) -> Vec<Inline> {
        if fragment.trim().is_empty() {
            return Vec::new();
        }
        let Ok(root) = parse_typst("<inline>", fragment)
        else {
            self.loss(
                "import.typst.fragment_parse_failed",
                "inline Typst fragment could not be parsed",
                SemanticStatus::Partial,
            );
            return Vec::new();
        };
        let mut inner = TypstLowerer {
            graph: std::mem::replace(&mut self.graph, DocumentGraph::new(DocumentId(0))),
            source: fragment,
            cursor: 0,
            line_breaks: 0,
        };
        let mut inlines = inner.collect_inlines(&root.items);
        trim_inlines(&mut inlines);
        self.graph = inner.graph;
        inlines
    }

    fn collect_inlines(&mut self, items: &[TypstItem]) -> Vec<Inline> {
        let mut inlines = Vec::new();
        let mut index = 0;
        while index < items.len() {
            match &items[index] {
                TypstItem::Text(text) => {
                    if !is_comment(text) {
                        push_text_inlines(&mut inlines, text);
                    }
                }
                TypstItem::Space | TypstItem::Parbreak => push_text_inlines(&mut inlines, " "),
                TypstItem::Strong(root) => {
                    let (children, trailing_space) = self.styled_children(root, '*');
                    inlines.push(Inline::Styled { style: "bold".into(), children });
                    if trailing_space {
                        push_text_inlines(&mut inlines, " ");
                    }
                }
                TypstItem::Emphasis(root) => {
                    let (children, trailing_space) = self.styled_children(root, '_');
                    inlines.push(Inline::Styled { style: "italic".into(), children });
                    if trailing_space {
                        push_text_inlines(&mut inlines, " ");
                    }
                }
                TypstItem::Math(root) => {
                    let (content, _) = math_content(self.raw(root.span.start, root.span.end));
                    let formula = document_ir::MathFormula::unsupported(
                        content,
                        Some("typst".into()),
                        document_ir::MathDisplay::Inline,
                        "typst-math",
                    );
                    inlines.push(Inline::InlineMath { formula });
                }
                TypstItem::Raw(_) => {
                    let mut raw = String::new();
                    while index < items.len() {
                        let TypstItem::Raw(chunk) = &items[index]
                        else {
                            break;
                        };
                        raw.push_str(chunk);
                        index += 1;
                    }
                    index -= 1;
                    let (content, _) = raw_content(&raw);
                    if !content.is_empty() {
                        inlines.push(Inline::InlineCode { text: content });
                    }
                }
                TypstItem::Link(link) => inlines.push(self.link_inline(link)),
                TypstItem::Quote(quote) => match self.link_quote(quote) {
                    Some(segments) => self.push_link_segments(segments, &mut inlines),
                    None => self.loss(
                        "import.typst.dynamic_content",
                        "dynamic Typst expression cannot be represented as inline content",
                        SemanticStatus::Unsupported,
                    ),
                },
                TypstItem::Heading(_) | TypstItem::ListItem(_) | TypstItem::EnumItem(_) | TypstItem::Block(_) => self.loss(
                    "import.typst.block_in_inline_context",
                    "block Typst structure cannot be represented as inline content",
                    SemanticStatus::Partial,
                ),
            }
            index += 1;
        }
        inlines
    }

    fn link_inline(&mut self, link: &TypstLink) -> Inline {
        let display = link
            .content
            .as_ref()
            .map(|content| self.collect_inlines(&content.items))
            .filter(|display| !display.is_empty())
            .unwrap_or_else(|| vec![Inline::Text { text: link.url.clone() }]);
        Inline::Link { target: RelationEndpoint::ExternalUri(link.url.clone()), display, title: None }
    }

    /// Recover the inline content of a `#` quote span that consists of one or
    /// more `#link("destination")[body]` expressions separated by plain markup.
    ///
    /// `None` keeps the span on the opaque path, so an unbalanced group, a
    /// cross-line group, a destination that is not a string literal, or any
    /// other dynamic expression still fails closed with its authored range.
    fn link_quote(&self, quote: &TypstRoot) -> Option<Vec<QuoteSegment<'a>>> {
        split_link_quote(self.raw(quote.span.start, quote.span.end))
    }

    fn push_link_segments(&mut self, segments: Vec<QuoteSegment<'a>>, inlines: &mut Vec<Inline>) {
        for segment in segments {
            match segment {
                QuoteSegment::Text(text) => {
                    if text.starts_with(char::is_whitespace)
                        && !inlines.is_empty()
                        && !last_inline_ends_with_space(inlines)
                    {
                        push_inline_text(inlines, " ");
                    }
                    push_text_inlines(inlines, text);
                }
                QuoteSegment::Link { destination, body } => {
                    let display = body
                        .map(|body| self.lower_inline_fragment(body))
                        .filter(|display| !display.is_empty())
                        .unwrap_or_else(|| vec![Inline::Text { text: destination.clone() }]);
                    inlines.push(Inline::Link {
                        target: RelationEndpoint::ExternalUri(destination),
                        display,
                        title: None,
                    });
                }
            }
        }
    }

    fn list_ahead(&self) -> Option<(Vec<ListLine>, bool, usize)> {
        if !(self.cursor == 0 || self.source.as_bytes().get(self.cursor - 1) == Some(&b'\n')) {
            return None;
        }
        let mut offset = self.cursor;
        let mut lines = Vec::new();
        let mut ordered = None;
        let mut base_indent = 0usize;
        while offset < self.source.len() {
            let end = self.source[offset..].find('\n').map(|index| offset + index).unwrap_or(self.source.len());
            let line = self.source[offset..end].trim_end_matches('\r');
            let Some((indent, line_ordered, number, content)) = list_marker(line)
            else {
                break;
            };
            match ordered {
                None => {
                    ordered = Some(line_ordered);
                    base_indent = indent;
                }
                Some(ordered_marker) => {
                    // A line at the base indentation with a different marker
                    // starts the next list. A deeper line is a nested list and
                    // stays in this run.
                    if line_ordered != ordered_marker && indent <= base_indent {
                        break;
                    }
                }
            }
            lines.push(ListLine { indent, ordered: line_ordered, number, content: content.to_string() });
            offset = if end < self.source.len() { end + 1 } else { end };
        }
        let ordered = ordered?;
        Some((lines, ordered, offset))
    }

    fn advance_text(&mut self, text: &str) {
        let rest = self.source.get(self.cursor..).unwrap_or("");
        if rest.starts_with(text) {
            self.cursor += text.len();
            return;
        }
        if let Some(offset) = rest.find(text) {
            self.cursor += offset + text.len();
            return;
        }
        self.loss(
            "import.typst.text_span_mismatch",
            format!("text item could not be located in source order: {text}"),
            SemanticStatus::Partial,
        );
    }

    fn advance_space(&mut self) -> usize {
        let bytes = self.source.as_bytes();
        let mut newlines = 0;
        if bytes.get(self.cursor) == Some(&b'\r') {
            self.cursor += 1;
        }
        if bytes.get(self.cursor) == Some(&b'\n') {
            self.cursor += 1;
            newlines += 1;
        }
        else {
            while matches!(bytes.get(self.cursor), Some(b' ' | b'\t')) {
                self.cursor += 1;
            }
        }
        newlines
    }

    fn push_text(&mut self, text: &str, sink: &mut Sink) {
        for (index, chunk) in split_paragraphs(text).into_iter().enumerate() {
            if index > 0 {
                sink.flush(&mut self.graph);
            }
            push_text_inlines(&mut sink.inlines, chunk);
        }
    }
}

struct ListLine {
    indent: usize,
    ordered: bool,
    number: Option<u32>,
    content: String,
}

/// One list level that is still open while items are placed.
struct OpenTypstList {
    indent: usize,
    ordered: bool,
    id: NodeId,
}

fn list_items_mut(graph: &mut DocumentGraph, id: NodeId) -> Option<&mut Vec<ListItem>> {
    let index = graph.blocks.iter().position(|node| node.id == id)?;
    match &mut graph.blocks[index].block {
        Block::List { items, .. } => Some(items),
        _ => None,
    }
}

fn attach_nested_list(graph: &mut DocumentGraph, parent: NodeId, child: NodeId) {
    if let Some(items) = list_items_mut(graph, parent) {
        if let Some(last) = items.last_mut() {
            last.children.push(child);
        }
    }
}

/// One piece of a single-line `#` quote span: authored markup or a link.
enum QuoteSegment<'a> {
    Text(&'a str),
    Link { destination: String, body: Option<&'a str> },
}

/// Split a single-line quote span into `#link("destination")[body]` expressions
/// and the markup between them.
///
/// The parser only accepts the exact shapes Typst authors use for links: a
/// string literal first argument and an optional content block. Anything else,
/// including a group that crosses a line break or a `#` expression that is not
/// a link, returns `None` so the caller keeps the opaque + span behaviour.
fn split_link_quote(raw: &str) -> Option<Vec<QuoteSegment<'_>>> {
    if raw.contains('\n') {
        return None;
    }
    let mut segments = Vec::new();
    let mut offset = 0usize;
    while offset < raw.len() {
        let rest = &raw[offset..];
        let hashed = match rest.find('#') {
            Some(hashed) => hashed,
            None => {
                segments.push(QuoteSegment::Text(rest));
                break;
            }
        };
        if hashed > 0 {
            segments.push(QuoteSegment::Text(&rest[..hashed]));
        }
        let (destination, body, consumed) = parse_typst_link(&rest[hashed..])?;
        segments.push(QuoteSegment::Link { destination, body });
        offset += hashed + consumed;
    }
    Some(segments)
}

/// Parse a single-line `#link("destination")[body]` expression at the start of
/// `raw`.
///
/// Returns the destination, the raw body between the content brackets, and how
/// many bytes the expression occupies. The body is omitted when the call has no
/// content block.
fn parse_typst_link(raw: &str) -> Option<(String, Option<&str>, usize)> {
    if raw.contains('\n') {
        return None;
    }
    let rest = raw.strip_prefix("#link")?.trim_start().strip_prefix('(')?.trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let mut destination = String::new();
    let mut index = 1usize;
    let mut closed = false;
    while index < rest.len() {
        let character = rest[index..].chars().next()?;
        match character {
            '\\' => {
                let escaped = rest[index + 1..].chars().next()?;
                destination.push(escaped);
                index += 1 + escaped.len_utf8();
            }
            '"' => {
                index += 1;
                closed = true;
                break;
            }
            other => {
                destination.push(other);
                index += other.len_utf8();
            }
        }
    }
    if !closed {
        return None;
    }
    let rest = rest[index..].trim_start().strip_prefix(')')?.trim_start();
    let Some(after) = rest.strip_prefix('[')
    else {
        let consumed = raw.len() - rest.len();
        return Some((destination, None, consumed));
    };
    let mut depth = 1usize;
    let mut end = None;
    for (offset, character) in after.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    let consumed = raw.len() - rest.len() + end + 2;
    Some((destination, Some(&after[..end]), consumed))
}

fn typst_span(item: &TypstItem) -> Option<oak_core::Range<usize>> {
    match item {
        TypstItem::Heading(heading) => Some(heading.content.span),
        TypstItem::Strong(root) | TypstItem::Emphasis(root) | TypstItem::Math(root) | TypstItem::Quote(root) => Some(root.span),
        TypstItem::ListItem(root) | TypstItem::EnumItem(root) => Some(root.span),
        TypstItem::Block(root) => Some(root.span),
        _ => None,
    }
}

fn split_list_lines(raw: &str, ordered: bool) -> (Vec<ListLine>, Option<usize>) {
    let mut lines = Vec::new();
    let mut offset = 0usize;
    let mut saw_item = false;
    let mut base_indent = 0usize;
    for segment in raw.split_inclusive('\n') {
        let line = segment.trim_end_matches('\n').trim_end_matches('\r');
        if let Some((indent, line_ordered, number, content)) = list_marker(line) {
            if !saw_item {
                base_indent = indent;
            }
            // A deeper line belongs to the same run even when its marker type
            // differs: that is a nested list, not a new paragraph.
            if line_ordered == ordered || (saw_item && indent > base_indent) {
                lines.push(ListLine { indent, ordered: line_ordered, number, content: content.to_string() });
                saw_item = true;
                offset += segment.len();
                continue;
            }
        }
        if !saw_item {
            return (lines, None);
        }
        if line.trim().is_empty() {
            offset += segment.len();
            continue;
        }
        return (lines, Some(offset));
    }
    (lines, None)
}

fn list_marker(line: &str) -> Option<(usize, bool, Option<u32>, &str)> {
    let mut indent = 0usize;
    let mut rest = line;
    while let Some(after) = rest.strip_prefix(' ') {
        indent += 1;
        rest = after;
    }
    while let Some(after) = rest.strip_prefix('\t') {
        indent = (indent / 4 + 1) * 4;
        rest = after;
    }
    if let Some(body) = rest.strip_prefix("- ").or_else(|| rest.strip_prefix("-\t")) {
        return Some((indent, false, None, body.trim_end_matches('\r')));
    }
    if let Some(body) = rest.strip_prefix("+ ").or_else(|| rest.strip_prefix("+\t")) {
        return Some((indent, true, None, body.trim_end_matches('\r')));
    }
    let digits: String = rest.chars().take_while(|character| character.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let after_digits = &rest[digits.len()..];
    let after_dot = after_digits.strip_prefix('.')?;
    let body = after_dot.strip_prefix(' ').or_else(|| after_dot.strip_prefix('\t'))?;
    Some((indent, true, digits.parse().ok(), body.trim_end_matches('\r')))
}

fn expression_kind(line: &str) -> String {
    let rest = line.strip_prefix('#').unwrap_or(line);
    let keyword: String = rest.chars().take_while(|character| character.is_ascii_alphabetic()).collect();
    if keyword.is_empty() { "typst.expression".into() } else { format!("typst.expression.{keyword}") }
}

/// Bracket structure found after a `#` dynamic expression head.
enum DynamicGroup {
    /// The head is not followed by a bracket group on its own line.
    Absent,
    /// The group opened and closed, with the exclusive end offset it closed at.
    Closed { end: usize },
    /// The group opened and never balanced before the search limit.
    Unclosed { end: usize },
}

/// Scan the bracket group of a `#`-prefixed Typst expression.
///
/// Only a group that starts on the same line as the expression head counts.
/// Parentheses, brackets and braces nest. String literals and both comment
/// forms are skipped so their brackets cannot close the group early.
fn scan_dynamic_group(source: &str, start: usize, limit: usize) -> DynamicGroup {
    let limit = limit.min(source.len());
    if start >= limit {
        return DynamicGroup::Absent;
    }
    let bytes = source.as_bytes();
    let mut index = if bytes[start] == b'#' { start + 1 } else { start };
    while index < limit && matches!(bytes[index], b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-') {
        index += 1;
    }
    if index >= limit || !matches!(bytes[index], b'(' | b'[' | b'{') {
        return DynamicGroup::Absent;
    }
    let mut expected: Vec<u8> = Vec::new();
    while index < limit {
        match bytes[index] {
            b'"' => index = skip_string_literal(bytes, index, limit),
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < limit && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => index = skip_block_comment(bytes, index, limit),
            open @ (b'(' | b'[' | b'{') => {
                expected.push(closing_bracket(open));
                index += 1;
            }
            close @ (b')' | b']' | b'}') => {
                if expected.last() != Some(&close) {
                    return DynamicGroup::Unclosed { end: limit };
                }
                expected.pop();
                index += 1;
                if expected.is_empty() {
                    return DynamicGroup::Closed { end: index };
                }
            }
            _ => index += 1,
        }
    }
    DynamicGroup::Unclosed { end: limit }
}

fn closing_bracket(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        _ => open,
    }
}

/// Skip a Typst string literal and return the offset just past it.
fn skip_string_literal(bytes: &[u8], start: usize, limit: usize) -> usize {
    let mut index = start + 1;
    while index < limit {
        match bytes[index] {
            b'\\' => index = (index + 2).min(limit),
            b'"' => return index + 1,
            b'\n' => return index,
            _ => index += 1,
        }
    }
    limit
}

/// Skip a possibly nested Typst block comment and return the offset just past it.
fn skip_block_comment(bytes: &[u8], start: usize, limit: usize) -> usize {
    let mut index = start + 2;
    let mut nesting = 1usize;
    while index < limit {
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            nesting += 1;
            index += 2;
            continue;
        }
        if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
            nesting -= 1;
            index += 2;
            if nesting == 0 {
                return index;
            }
            continue;
        }
        index += 1;
    }
    limit
}

fn is_comment(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with("/*")
}

fn math_content(raw: &str) -> (String, bool) {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('$') && trimmed.ends_with('$') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let display = inner.starts_with(char::is_whitespace) || inner.ends_with(char::is_whitespace);
        return (inner.trim().to_string(), display);
    }
    (trimmed.to_string(), false)
}

fn raw_content(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim();
    let ticks = trimmed.chars().take_while(|character| *character == '`').count();
    if ticks == 0 {
        return (trimmed.to_string(), None);
    }
    if trimmed.len() < ticks * 2 {
        return (String::new(), None);
    }
    let inner = &trimmed[ticks..trimmed.len() - ticks];
    if ticks >= 3 {
        if let Some(newline) = inner.find('\n') {
            let first = inner[..newline].trim();
            let is_language = !first.is_empty()
                && first.chars().all(|character| character.is_alphanumeric() || matches!(character, '+' | '-' | '_'));
            if is_language {
                return (inner[newline + 1..].trim_matches('\n').to_string(), Some(first.to_string()));
            }
        }
        return (inner.trim_matches('\n').to_string(), None);
    }
    (inner.to_string(), None)
}

fn split_paragraphs(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            let mut next = index + 1;
            while next < bytes.len() && matches!(bytes[next], b' ' | b'\t' | b'\r') {
                next += 1;
            }
            if next < bytes.len() && bytes[next] == b'\n' {
                chunks.push(&text[start..index]);
                index = next + 1;
                start = index;
                continue;
            }
        }
        index += 1;
    }
    chunks.push(&text[start..]);
    chunks
}

fn push_text_inlines(inlines: &mut Vec<Inline>, text: &str) {
    let normalized = normalize_text(text);
    if normalized.is_empty() {
        return;
    }
    if inlines.is_empty() {
        let trimmed = normalized.trim_start();
        if !trimmed.is_empty() {
            push_inline_text(inlines, trimmed);
        }
        return;
    }
    if last_inline_ends_with_space(inlines) && normalized.starts_with(' ') {
        push_inline_text(inlines, &normalized[1..]);
        return;
    }
    push_inline_text(inlines, &normalized);
}

fn normalize_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            pending_space = !normalized.is_empty();
            continue;
        }
        if pending_space {
            normalized.push(' ');
            pending_space = false;
        }
        normalized.push(character);
    }
    if pending_space {
        normalized.push(' ');
    }
    if normalized.is_empty() && !text.is_empty() {
        return " ".into();
    }
    normalized
}

fn push_inline_text(inlines: &mut Vec<Inline>, text: &str) {
    if let Some(Inline::Text { text: previous }) = inlines.last_mut() {
        previous.push_str(text);
        return;
    }
    inlines.push(Inline::Text { text: text.into() });
}

fn last_inline_ends_with_space(inlines: &[Inline]) -> bool {
    matches!(inlines.last(), Some(Inline::Text { text }) if text.ends_with(' '))
}

fn trim_inlines(inlines: &mut Vec<Inline>) {
    if let Some(Inline::Text { text }) = inlines.last_mut() {
        let trimmed = text.trim_end().len();
        text.truncate(trimmed);
    }
    if matches!(inlines.last(), Some(Inline::Text { text }) if text.is_empty()) {
        inlines.pop();
    }
    if let Some(Inline::Text { text }) = inlines.first_mut() {
        if text.starts_with(' ') {
            *text = text.trim_start().to_string();
        }
    }
    if matches!(inlines.first(), Some(Inline::Text { text }) if text.is_empty()) {
        inlines.remove(0);
    }
}
