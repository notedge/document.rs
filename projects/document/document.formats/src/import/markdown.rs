use std::{collections::HashMap, path::Path};

use document_ir::{
    Block, BreakKind, DocumentGraph, DocumentId, Inline, ListItem, LossMarker, MathDisplay, NodeId, RelationEndpoint,
    SemanticStatus, SourceSpan, TableCell, TableRow,
};
use oak_core::{GreenNode, Parser, RedNode, RedTree, SourceText, parser::session::ParseSession};
use oak_markdown::{
    MarkdownLanguage, MarkdownParser, lexer::token_type::MarkdownTokenType, parser::element_type::MarkdownElementType,
};

use super::{
    common::{decode_escaped_text, register_image_assets},
    tex_math::lower_formula_source,
};
use crate::FormatError;

/// Import Markdown from disk into `notedown-ir`.
pub fn import_markdown(path: impl AsRef<Path>) -> Result<DocumentGraph, FormatError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path)
        .map_err(|error| FormatError::invalid_input(format!("failed to read {}: {error}", path.display())))?;
    import_markdown_bytes(&path.display().to_string(), &text)
}

/// Oak profile for semantic Markdown import: enable the constructs required by
/// `notedown-formats` round-trip and markdown-project reopen probes.
///
/// Math and footnotes are part of the import contract, so the profile has to
/// enable them explicitly instead of leaving every extension at its default.
fn markdown_import_language() -> MarkdownLanguage {
    let mut language = MarkdownLanguage::default();
    language.allow_headings = true;
    language.allow_math = true;
    language.allow_footnotes = true;
    language.allow_front_matter = true;
    language.allow_strikethrough = true;
    language.allow_lists = true;
    language.allow_definition_lists = true;
    language.allow_blockquotes = true;
    language.allow_fenced_code_blocks = true;
    language.allow_tables = true;
    language.allow_horizontal_rules = true;
    language.allow_task_lists = true;
    language.allow_html = true;
    language.allow_html_tag_filter = true;
    language
}

/// Import Markdown text into `notedown-ir` via `oak-markdown`.
///
/// Footnote definitions stay `Block::Opaque { kind: "footnote_definition" }`
/// with their authored payload, and references stay `Inline::Note`, because
/// every writer that can place footnotes (HTML, EPUB, DOCX, Markdown) reads
/// those two carries directly. Importing alone loses nothing, so this profile
/// records no footnote loss.
pub fn import_markdown_bytes(label: &str, text: &str) -> Result<DocumentGraph, FormatError> {
    import_markdown_bytes_with_footnote_flow(label, text, false)
}

/// Import Markdown for a flow-only target such as SVG/PDF reflow.
///
/// The graph is identical to [`import_markdown_bytes`], but the two footnote
/// loss markers are recorded: a flow target has no footnote collection, so the
/// body is never placed and the reference keeps only its textual marker.
pub fn import_markdown_bytes_for_flow(label: &str, text: &str) -> Result<DocumentGraph, FormatError> {
    import_markdown_bytes_with_footnote_flow(label, text, true)
}

fn import_markdown_bytes_with_footnote_flow(
    label: &str,
    text: &str,
    flow_footnotes: bool,
) -> Result<DocumentGraph, FormatError> {
    let language = markdown_import_language();
    let masked = mask_fenced_code_angles(text);
    let lex_source = SourceText::new(masked.as_str());
    let source = SourceText::new(text);
    let mut cache = ParseSession::<MarkdownLanguage>::default();
    let parser = MarkdownParser::new(&language);
    let parsed = parser.parse(&lex_source, &[], &mut cache);
    let green_tree = parsed.result.map_err(|error| FormatError::parse("markdown", error.to_string()))?;
    lower_markdown_tree(label, green_tree, &source, flow_footnotes)
}

/// Hide `<` inside fenced code bodies from the lexer.
///
/// `oak-markdown` re-lexes a fenced body as ordinary Markdown, and its HTML tag
/// scanner (`lexer::MarkdownLexer::lex_any_tag`) searches for the closing `>`
/// without stopping at a line break. A single comparison in code — `if n <= 1:`
/// is enough — therefore swallows every token up to the next `>` anywhere later
/// in the document, including the fence that should close its own block. The
/// parser then builds one `CodeBlock` that runs to EOF, so every heading, list
/// and table after the fence silently disappears while the conversion still
/// reports success.
///
/// The lexer is context-free, so the parse input has to be masked instead of
/// taught about fences. This returns a byte-for-byte copy of `text` with `<`
/// replaced by `x` inside fenced bodies: every span keeps its offset, and the
/// lowering still slices payloads out of the untouched original source.
fn mask_fenced_code_angles(text: &str) -> String {
    let mut masked = String::with_capacity(text.len());
    let mut in_fence = false;
    let length = text.len();
    let mut offset = 0;
    loop {
        let rest = &text[offset..];
        let line_end = rest.find(['\r', '\n']).map(|index| offset + index).unwrap_or(length);
        let line = &text[offset..line_end];
        if is_fence_line(line) {
            in_fence = !in_fence;
            masked.push_str(line);
        }
        else if in_fence {
            for character in line.chars() {
                masked.push(if character == '<' { 'x' } else { character });
            }
        }
        else {
            masked.push_str(line);
        }
        if line_end >= length {
            break;
        }
        if rest[line_end - offset..].starts_with("\r\n") {
            masked.push_str("\r\n");
            offset = line_end + 2;
        }
        else {
            masked.push(rest[line_end - offset..].chars().next().unwrap_or('\n'));
            offset = line_end + 1;
        }
    }
    masked
}

/// Mirror the lexer's own fence test: at most three leading blanks followed by
/// a run of at least three `` ` `` or `~`. `oak-markdown` closes a block at the
/// next fence of either character, so the same predicate ends the masked
/// region.
fn is_fence_line(line: &str) -> bool {
    let indent = line.chars().take_while(|character| *character == ' ' || *character == '\t').count();
    if indent > 3 {
        return false;
    }
    let rest = &line[indent..];
    let Some(character) = rest.chars().next()
    else {
        return false;
    };
    if character != '`' && character != '~' {
        return false;
    }
    rest.chars().take_while(|item| *item == character).count() >= 3
}

fn lower_markdown_tree(
    label: &str,
    green_tree: &GreenNode<MarkdownLanguage>,
    source: &SourceText,
    flow_footnotes: bool,
) -> Result<DocumentGraph, FormatError> {
    let red_root = RedNode::new(green_tree, 0);
    let mut graph = DocumentGraph::new(document_id_for(label));
    let mut context = LinkContext { flow_footnotes, ..LinkContext::default() };
    let children = red_root.children().collect::<Vec<_>>();
    for child in &children {
        if let RedTree::Node(node) = *child {
            register_link_definition_node(node, source, &mut context);
        }
    }
    let mut index = 0;
    let mut pending_footnote: Option<(usize, usize)> = None;
    let mut list_stack: Vec<OpenList> = Vec::new();
    while let Some(child) = children.get(index).copied() {
        index += 1;
        let RedTree::Node(node) = child
        else {
            continue;
        };
        // A list run is the only construct that can continue onto the next
        // top-level node: `oak-markdown` ends a parent list at its first nested
        // line, so the remaining sibling items come back as root-level `List`
        // nodes. Any other block ends the run.
        if node.element_type() != MarkdownElementType::List {
            list_stack.clear();
        }
        if let Some((offset, heading)) = setext_heading(node, children.get(index..).unwrap_or_default(), source, &mut context) {
            graph.push_block(heading);
            index += offset + 1;
            pending_footnote = None;
            continue;
        }
        if node.element_type() == MarkdownElementType::CodeBlock {
            let (language, content, consumed, body_offset) =
                extract_code_block(node, source, children.get(index..).unwrap_or_default());
            index += consumed;
            let span = Some(SourceSpan { start: body_offset as u32, end: (body_offset + content.len()) as u32 });
            let block = match language {
                Some(language) if is_diagram_language(&language) => Block::Diagram { language, source: content },
                Some(language) if is_chart_language(&language) => Block::Chart { language, source: content },
                language => Block::Code { language, content },
            };
            graph.push_block_at(block, span);
            pending_footnote = None;
            continue;
        }
        if node.element_type() == MarkdownElementType::FrontMatter {
            // Front matter carries metadata, not document content, and the IR has
            // no metadata field yet. Keeping it as an unsupported opaque block
            // preserves the authored bytes and the span, so the adapter can fail
            // closed instead of rendering the YAML as horizontal rules and text.
            let payload = node.text(source).into_owned();
            let span = node.span();
            graph.push_block_at(
                Block::Opaque { kind: "front_matter".into(), payload_hint: payload, status: SemanticStatus::Unsupported },
                Some(SourceSpan { start: span.start as u32, end: span.end as u32 }),
            );
            graph.push_loss_at(
                LossMarker {
                    code: "import.markdown.front_matter_unmapped".into(),
                    message:
                        "front matter carries metadata that the IR has no field for, so it stays an unsupported opaque block"
                            .into(),
                    status: SemanticStatus::Unsupported,
                },
                Some(SourceSpan { start: span.start as u32, end: span.end as u32 }),
            );
            pending_footnote = None;
            continue;
        }
        if node.element_type() == MarkdownElementType::FootnoteDefinition {
            let span = node.span();
            let site = Some(SourceSpan { start: span.start as u32, end: span.end as u32 });
            // The definition keeps its authored payload for the writers that
            // place it at document end, so the block itself is resolved. Only a
            // flow-only import records the loss, because only a flow target
            // drops the body.
            graph.push_block_at(
                Block::Opaque {
                    kind: "footnote_definition".into(),
                    payload_hint: node.text(source).into_owned(),
                    status: SemanticStatus::Resolved,
                },
                site,
            );
            if context.flow_footnotes {
                graph.push_loss_at(footnote_body_loss_marker(), site);
            }
            pending_footnote = Some((graph.blocks.len() - 1, node.span().end));
            continue;
        }
        if let Some((block_index, definition_end)) = pending_footnote {
            if node.element_type() == MarkdownElementType::Paragraph
                && append_footnote_continuation(&mut graph, block_index, definition_end, node, source)
            {
                pending_footnote = Some((block_index, node.span().end));
                continue;
            }
            pending_footnote = None;
        }
        if matches!(node.element_type(), MarkdownElementType::HtmlComment | MarkdownElementType::XmlComment) {
            pending_footnote = None;
            continue;
        }
        if matches!(node.element_type(), MarkdownElementType::HtmlTag | MarkdownElementType::XmlTag) {
            let raw = node.text(source);
            if let Some((url, display)) = autolink_target(raw.trim_end()) {
                let span = node.span();
                graph.push_block_at(
                    Block::Paragraph {
                        content: vec![Inline::Link {
                            target: RelationEndpoint::ExternalUri(url),
                            display: vec![Inline::Text { text: display }],
                            title: None,
                        }],
                    },
                    Some(SourceSpan { start: span.start as u32, end: span.end as u32 }),
                );
                pending_footnote = None;
                continue;
            }
            let (html, consumed) = extract_html_block(node, children.get(index..).unwrap_or_default(), source);
            index += consumed;
            let html = html.trim_end_matches('\n').trim_end_matches('\r').to_owned();
            let span = SourceSpan { start: node.span().start as u32, end: (node.span().start + html.len()) as u32 };
            // GFM tables cannot express merged cells, so a spanning table is
            // exported as an inline HTML table. Reopening it as an opaque raw
            // block would quietly turn the table back into markup, so a
            // standalone `<table>` element is lowered into the IR table.
            #[cfg(feature = "html")]
            let block = lower_html_table_block(&html, &mut graph).unwrap_or(Block::RawHtml { html });
            // Without the HTML importer the table shape cannot be read back,
            // so the authored block stays verbatim rather than being guessed.
            #[cfg(not(feature = "html"))]
            let block = Block::RawHtml { html };
            // A raw HTML block is preserved for HTML/EPUB export, but the
            // semantic SVG/PDF reflow does not lay it out. Record the loss so
            // the report never claims a rendering that did not happen.
            if matches!(&block, Block::RawHtml { .. }) {
                graph.push_loss_at(raw_html_loss_marker(), Some(span));
            }
            graph.push_block(block);
            pending_footnote = None;
            continue;
        }
        if node.element_type() == MarkdownElementType::DefinitionList {
            // The IR has no definition list node. The term stays a paragraph and
            // each description becomes a paragraph of its own, and the downgrade
            // is recorded so it is never mistaken for a faithful definition list.
            let span = node.span();
            graph.push_loss_at(
                LossMarker {
                    code: "import.markdown.definition_list".into(),
                    message:
                        "definition list is lowered to term and description paragraphs because the IR has no definition list node"
                            .into(),
                    status: SemanticStatus::Partial,
                },
                Some(SourceSpan { start: span.start as u32, end: span.end as u32 }),
            );
            let content = definition_description_inlines(node, source, &mut context);
            if !content.is_empty() {
                graph.push_block(Block::Paragraph { content });
            }
            pending_footnote = None;
            continue;
        }
        if node.element_type() == MarkdownElementType::List {
            let mut flat = Vec::new();
            collect_list_items(node, source, &mut context, &mut flat);
            place_flat_items(&mut graph, &mut list_stack, flat);
            pending_footnote = None;
            continue;
        }
        match lower_block_node(node, source, &mut context, &mut graph) {
            BlockOutcome::Block(block) => {
                graph.push_block(block);
            }
            BlockOutcome::Loss(marker) => {
                let span = node.span();
                graph.push_loss_at(marker, Some(SourceSpan { start: span.start as u32, end: span.end as u32 }));
            }
            BlockOutcome::Skip => {}
        }
    }

    for (marker, span) in context.losses {
        graph.push_loss_at(marker, span);
    }

    register_image_assets(&mut graph);

    if graph.blocks.is_empty() && graph.coverage.loss.is_empty() {
        graph.push_loss(LossMarker {
            code: "import.markdown.empty_document".into(),
            message: "markdown source produced no semantic blocks".into(),
            status: SemanticStatus::Partial,
        });
    }
    Ok(graph)
}

enum BlockOutcome {
    Block(Block),
    Loss(LossMarker),
    Skip,
}

#[derive(Default)]
struct LinkContext {
    definitions: HashMap<String, LinkDefinition>,
    losses: Vec<(LossMarker, Option<SourceSpan>)>,
    /// True when the graph serves a flow-only target that cannot place footnotes.
    flow_footnotes: bool,
}

struct LinkDefinition {
    destination: String,
    title: Option<String>,
}

/// Promote a CommonMark setext heading before the underline is lowered as a
/// thematic break.
///
/// `oak-markdown` does not parse setext headings: `Title\n---` or `Title\n===`
/// becomes a paragraph followed by a horizontal-rule node, exactly like a
/// thematic break. The two differ only by adjacency, so the underline must sit
/// on the very next line of the paragraph; a blank line in between keeps the
/// thematic break. `=` underlines an h1 and `-` an h2, as in CommonMark §4.3.
fn setext_heading(
    node: RedNode<MarkdownLanguage>,
    trailing: &[RedTree<MarkdownLanguage>],
    source: &SourceText,
    context: &mut LinkContext,
) -> Option<(usize, Block)> {
    if node.element_type() != MarkdownElementType::Paragraph || parse_link_definition(node.text(source).trim()).is_some() {
        return None;
    }
    let mut underline = None;
    for (offset, child) in trailing.iter().enumerate() {
        match child {
            RedTree::Node(candidate) => {
                underline = Some((offset, *candidate));
                break;
            }
            RedTree::Leaf(leaf) if leaf.text(source).trim().is_empty() => {}
            RedTree::Leaf(_) => return None,
        }
    }
    let (offset, underline) = underline?;
    if underline.element_type() != MarkdownElementType::HorizontalRule {
        return None;
    }
    let marker = underline.text(source);
    let marker = marker.trim();
    let Some(marker_byte) = marker.bytes().next()
    else {
        return None;
    };
    if marker_byte != b'-' && marker_byte != b'=' {
        return None;
    }
    if !marker.bytes().all(|byte| byte == marker_byte) {
        return None;
    }
    let level = if marker_byte == b'=' { 1 } else { 2 };
    let node_span = node.span();
    let gap = source.slice((node_span.end..underline.span().start).into());
    if !gap.is_empty() && gap != "\n" && gap != "\r\n" {
        return None;
    }
    let title = collect_inlines(node, source, context);
    if title.is_empty() {
        return None;
    }
    Some((offset, Block::Section { level, title, children: Vec::new() }))
}

/// Promote a setext h1 whose underline stayed inside the paragraph token.
///
/// `oak-markdown` only produces a separate `HorizontalRule` node for `---`.
/// A `===` underline is lexed as ordinary paragraph text, so `Title\n===` would
/// otherwise reach the IR as a paragraph containing the raw underline.
fn paragraph_setext_heading(node: RedNode<MarkdownLanguage>, source: &SourceText, context: &mut LinkContext) -> Option<Block> {
    if node.element_type() != MarkdownElementType::Paragraph {
        return None;
    }
    let raw = node.text(source);
    let raw = raw.trim_end_matches(['\r', '\n']);
    let marker = raw.rsplit('\n').next()?.trim();
    if marker.is_empty() || !marker.bytes().all(|byte| byte == b'=') {
        return None;
    }
    let mut title = collect_inlines(node, source, context);
    while matches!(title.last(), Some(Inline::Break { .. })) {
        title.pop();
    }
    if let Some(Inline::Text { text }) = title.last_mut() {
        if text.trim().bytes().all(|byte| byte == b'=') {
            title.pop();
        }
    }
    while matches!(title.last(), Some(Inline::Break { .. })) {
        title.pop();
    }
    if title.is_empty() {
        return None;
    }
    Some(Block::Section { level: 1, title, children: Vec::new() })
}

fn register_link_definition_node(node: RedNode<MarkdownLanguage>, source: &SourceText, context: &mut LinkContext) {
    if node.element_type() != MarkdownElementType::Paragraph {
        return;
    }
    let text = node.text(source);
    let raw = text.trim();
    if let Some((label, destination, title)) = parse_link_definition(raw) {
        context.definitions.entry(label).or_insert(LinkDefinition { destination, title });
        return;
    }
    if raw.starts_with('[') && raw.contains("]:") {
        let span = node.span();
        context.losses.push((
            LossMarker {
                code: "import.markdown.invalid_link_definition".into(),
                message: format!("unsupported or invalid link definition: {}", raw),
                status: SemanticStatus::Partial,
            },
            Some(SourceSpan { start: span.start as u32, end: span.end as u32 }),
        ));
    }
}

fn parse_link_definition(raw: &str) -> Option<(String, String, Option<String>)> {
    let close = reference_label_close(raw.strip_prefix('[')?)? + 1;
    if !raw[close..].starts_with("]:") {
        return None;
    }
    let label = raw.get(1..close)?.trim();
    let remainder = raw.get(close + 2..)?.trim();
    let (destination, title) = split_destination_and_title(remainder);
    if label.is_empty() || destination.is_empty() {
        return None;
    }
    Some((normalize_link_label(label), destination, title))
}

fn normalize_link_label(label: &str) -> String {
    decode_escaped_text(label).split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn reference_label_close(content: &str) -> Option<usize> {
    let mut escaped = false;
    for (index, character) in content.char_indices() {
        if escaped {
            escaped = false;
        }
        else if character == '\\' {
            escaped = true;
        }
        else if character == ']' {
            return Some(index);
        }
    }
    None
}

fn split_reference_link(raw: &str) -> Option<(String, String, bool)> {
    let content = raw.strip_prefix('[').or_else(|| raw.strip_prefix("!["))?;
    let close = reference_label_close(content)?;
    let text = decode_escaped_text(&content[..close]);
    let suffix = &content[close + 1..];
    if let Some(reference) = suffix.strip_prefix('[').and_then(|value| value.strip_suffix(']')) {
        let label = if reference.is_empty() { text.clone() } else { decode_escaped_text(reference) };
        return Some((text, label, true));
    }
    if suffix.is_empty() {
        return Some((text.clone(), text, false));
    }
    None
}

fn lower_block_node(
    node: RedNode<MarkdownLanguage>,
    source: &SourceText,
    context: &mut LinkContext,
    graph: &mut DocumentGraph,
) -> BlockOutcome {
    let kind = node.element_type();
    match kind {
        MarkdownElementType::Heading1
        | MarkdownElementType::Heading2
        | MarkdownElementType::Heading3
        | MarkdownElementType::Heading4
        | MarkdownElementType::Heading5
        | MarkdownElementType::Heading6 => BlockOutcome::Block(Block::Section {
            level: heading_level(kind),
            title: collect_heading_inlines(node, source, context),
            children: Vec::new(),
        }),
        MarkdownElementType::Paragraph => {
            if let Some(section) = paragraph_setext_heading(node, source, context) {
                return BlockOutcome::Block(section);
            }
            if let Some(code) = indented_code_block(node, source) {
                return BlockOutcome::Block(code);
            }
            let text = node.text(source);
            if parse_link_definition(text.trim()).is_some() {
                BlockOutcome::Skip
            }
            else {
                let mut content = collect_inlines(node, source, context);
                trim_trailing_line_break(&mut content);
                if content.is_empty() {
                    // Markdown blank lines and container-trailing newlines are not
                    // paragraphs: an empty inline result must never reach the graph.
                    return BlockOutcome::Skip;
                }
                BlockOutcome::Block(Block::Paragraph { content })
            }
        }
        MarkdownElementType::CodeBlock => {
            // Fenced code is lowered in `lower_markdown_tree` because a premature
            // short closing fence needs lookahead over the following siblings.
            BlockOutcome::Skip
        }
        MarkdownElementType::MathBlock => {
            let raw = node.text(source);
            let body = strip_math_delimiters(&raw, "$$");
            BlockOutcome::Block(Block::Math { formula: lower_formula_source(&body, MathDisplay::Display, "latex") })
        }
        MarkdownElementType::List => {
            // Top-level lists are intercepted before this lowering so their
            // levels can be rebuilt across split root nodes.
            BlockOutcome::Skip
        }
        MarkdownElementType::Blockquote => BlockOutcome::Block(lower_blockquote(node, source, context, graph)),
        MarkdownElementType::Table => {
            BlockOutcome::Block(Block::Table { caption: None, rows: extract_table(node, source, context) })
        }
        MarkdownElementType::HorizontalRule => BlockOutcome::Block(Block::Opaque {
            kind: "thematic_break".into(),
            payload_hint: "---".into(),
            status: SemanticStatus::Partial,
        }),
        MarkdownElementType::HtmlTag | MarkdownElementType::XmlTag => {
            let raw = node.text(source).into_owned();
            if let Some((url, display)) = autolink_target(raw.trim_end()) {
                return BlockOutcome::Block(Block::Paragraph {
                    content: vec![Inline::Link {
                        target: RelationEndpoint::ExternalUri(url),
                        display: vec![Inline::Text { text: display }],
                        title: None,
                    }],
                });
            }
            let span = node.span();
            context.losses.push((raw_html_loss_marker(), Some(SourceSpan { start: span.start as u32, end: span.end as u32 })));
            BlockOutcome::Block(Block::RawHtml { html: raw })
        }
        MarkdownElementType::HtmlComment | MarkdownElementType::XmlComment => BlockOutcome::Skip,
        _ => BlockOutcome::Loss(LossMarker {
            code: "import.markdown.unsupported_block".into(),
            message: format!("unsupported markdown block: {kind:?}"),
            status: SemanticStatus::Unsupported,
        }),
    }
}

fn collect_heading_inlines(node: RedNode<MarkdownLanguage>, source: &SourceText, context: &mut LinkContext) -> Vec<Inline> {
    let mut inlines = collect_inlines(node, source, context);
    let raw = node.text(source);
    let marker_length = raw.chars().take_while(|character| *character == '#').count();
    if !(1..=6).contains(&marker_length) {
        return inlines;
    }

    let Some(Inline::Text { text }) = inlines.first_mut()
    else {
        return inlines;
    };
    let Some(rest) = text.strip_prefix(&"#".repeat(marker_length))
    else {
        return inlines;
    };
    *text = rest.trim_start_matches([' ', '\t']).to_string();
    if text.is_empty() {
        inlines.remove(0);
    }
    inlines
}

fn collect_inlines(node: RedNode<MarkdownLanguage>, source: &SourceText, context: &mut LinkContext) -> Vec<Inline> {
    let mut inlines = Vec::new();
    for child in node.children() {
        match child {
            RedTree::Node(child_node) => {
                if matches!(child_node.element_type(), MarkdownElementType::Text | MarkdownElementType::HeadingText) {
                    push_text_inline(&mut inlines, &decode_escaped_text(&child_node.text(source)));
                    continue;
                }
                let link_like = matches!(child_node.element_type(), MarkdownElementType::Link | MarkdownElementType::Image);
                let trailing = if link_like { trailing_link_whitespace(&child_node.text(source)) } else { String::new() };
                if let Some(inline) = lower_inline_node(child_node, source, context) {
                    push_inline(&mut inlines, inline);
                }
                if !trailing.is_empty() {
                    push_inline(&mut inlines, Inline::Text { text: trailing });
                }
            }
            RedTree::Leaf(leaf) => {
                let text = leaf.text(source);
                if !text.is_empty() && !is_markdown_marker(text.as_ref()) {
                    let inline = match leaf.kind() {
                        MarkdownTokenType::HtmlComment => None,
                        MarkdownTokenType::HtmlTag if is_authored_html_tag(text.as_ref()) => {
                            if let Some((url, display)) = autolink_target(text.as_ref()) {
                                Some(Inline::Link {
                                    target: RelationEndpoint::ExternalUri(url),
                                    display: vec![Inline::Text { text: display }],
                                    title: None,
                                })
                            }
                            else {
                                let span = leaf.span();
                                context.losses.push((
                                    raw_html_loss_marker(),
                                    Some(SourceSpan { start: span.start as u32, end: span.end as u32 }),
                                ));
                                Some(Inline::RawHtml { html: text.into_owned() })
                            }
                        }
                        _ => {
                            let text = decode_escaped_text(text.as_ref());
                            push_text_inline(&mut inlines, &text);
                            None
                        }
                    };
                    if let Some(inline) = inline {
                        push_inline(&mut inlines, inline);
                    }
                }
            }
        }
    }
    if inlines.is_empty() {
        let text = node.text(source);
        if !text.trim().is_empty() {
            // A whole node that carries no element children still has to decode escapes,
            // otherwise `\[` stays a literal backslash on reopen.
            let decoded = decode_escaped_text(text.as_ref());
            push_text_inline(&mut inlines, &decoded);
        }
    }
    // `oak-markdown` splits a numeric entity into `&` and `#65;`, so decoding
    // has to happen on the merged text run instead of per token.
    merge_adjacent_text(&mut inlines);
    inlines
}

/// Merge adjacent text runs and decode HTML entities in each resulting run.
fn merge_adjacent_text(inlines: &mut Vec<Inline>) {
    let mut merged: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines.drain(..) {
        if let Inline::Text { text } = inline {
            if let Some(Inline::Text { text: previous }) = merged.last_mut() {
                previous.push_str(&text);
                continue;
            }
            merged.push(Inline::Text { text });
        }
        else {
            merged.push(inline);
        }
    }
    for inline in &mut merged {
        if let Inline::Text { text } = inline {
            *text = decode_entities(text);
        }
    }
    *inlines = merged;
}

fn lower_inline_node(node: RedNode<MarkdownLanguage>, source: &SourceText, context: &mut LinkContext) -> Option<Inline> {
    match node.element_type() {
        MarkdownElementType::Text | MarkdownElementType::HeadingText => {
            Some(Inline::Text { text: decode_escaped_text(&node.text(source)) })
        }
        MarkdownElementType::Strong => {
            Some(Inline::Styled { style: "bold".into(), children: collect_inlines(node, source, context) })
        }
        MarkdownElementType::Emphasis => {
            Some(Inline::Styled { style: "italic".into(), children: collect_inlines(node, source, context) })
        }
        MarkdownElementType::InlineCode => Some(Inline::InlineCode { text: inline_code_text(&node.text(source)) }),
        MarkdownElementType::Link => parse_inline_link(node, source, context),
        MarkdownElementType::Image => parse_inline_image(node, source, context),
        MarkdownElementType::MathInline => {
            let raw = node.text(source);
            let body = strip_math_delimiters(&raw, "$");
            Some(Inline::InlineMath { formula: lower_formula_source(&body, MathDisplay::Inline, "latex") })
        }
        MarkdownElementType::FootnoteReference => {
            let raw = node.text(source);
            let label = raw.trim().strip_prefix("[^").and_then(|value| value.strip_suffix(']')).unwrap_or_default().to_string();
            if context.flow_footnotes {
                let span = node.span();
                context.losses.push((
                    footnote_reference_loss_marker(),
                    Some(SourceSpan { start: span.start as u32, end: span.end as u32 }),
                ));
            }
            Some(Inline::Note { label, display: None })
        }
        MarkdownElementType::Strikethrough => {
            Some(Inline::Styled { style: "strike".into(), children: collect_inlines(node, source, context) })
        }
        MarkdownElementType::HtmlTag | MarkdownElementType::XmlTag => {
            let raw = node.text(source).into_owned();
            if let Some((url, display)) = autolink_target(raw.trim_end()) {
                return Some(Inline::Link {
                    target: RelationEndpoint::ExternalUri(url),
                    display: vec![Inline::Text { text: display }],
                    title: None,
                });
            }
            let span = node.span();
            context.losses.push((raw_html_loss_marker(), Some(SourceSpan { start: span.start as u32, end: span.end as u32 })));
            Some(Inline::RawHtml { html: raw })
        }
        MarkdownElementType::HtmlComment | MarkdownElementType::XmlComment => None,
        _ => None,
    }
}

fn push_inline(inlines: &mut Vec<Inline>, inline: Inline) {
    if let Inline::Text { text: right } = inline {
        if let Some(Inline::Text { text: left }) = inlines.last_mut() {
            left.push_str(&right);
            return;
        }
        inlines.push(Inline::Text { text: right });
        return;
    }
    inlines.push(inline);
}

fn push_soft_break(inlines: &mut Vec<Inline>) {
    if inlines.is_empty() || matches!(inlines.last(), Some(Inline::Break { .. })) {
        return;
    }
    inlines.push(Inline::Break { kind: BreakKind::Soft });
}

/// Push a text run, turning authored line endings into IR breaks.
///
/// `oak-markdown` keeps every raw line ending inside the text token, so without
/// this split a hard break (`two trailing spaces` or a trailing `\`) never
/// becomes `BreakKind::Hard` and the renderer emits the literal spaces instead
/// of a line break (CommonMark §6.9).
fn push_text_inline(inlines: &mut Vec<Inline>, raw: &str) {
    if !raw.contains('\n') {
        if !raw.is_empty() {
            push_inline(inlines, Inline::Text { text: raw.to_string() });
        }
        return;
    }
    let mut cursor = 0usize;
    for (index, _) in raw.match_indices('\n') {
        let segment = &raw[cursor..index];
        let segment = segment.strip_suffix('\r').unwrap_or(segment);
        if segment.is_empty() {
            // The lexer sometimes hands the line ending over as its own token,
            // so the hard-break markup is still attached to the previous text.
            let kind = resolve_break_kind(inlines);
            inlines.push(Inline::Break { kind });
        }
        else {
            let (text, hard) = split_hard_break(segment);
            if !text.is_empty() {
                push_inline(inlines, Inline::Text { text: text.to_string() });
            }
            inlines.push(Inline::Break { kind: if hard { BreakKind::Hard } else { BreakKind::Soft } });
        }
        cursor = index + 1;
    }
    let tail = &raw[cursor..];
    if !tail.is_empty() {
        push_inline(inlines, Inline::Text { text: tail.to_string() });
    }
}

/// Decide the break kind for a line ending that arrived as its own token, and
/// strip the hard-break markup off the text that precedes it.
fn resolve_break_kind(inlines: &mut Vec<Inline>) -> BreakKind {
    let Some(Inline::Text { text }) = inlines.last_mut()
    else {
        return BreakKind::Soft;
    };
    let (trimmed, hard) = split_hard_break(text);
    let trimmed = trimmed.to_string();
    let empty = trimmed.is_empty();
    *text = trimmed;
    if empty {
        inlines.pop();
    }
    if hard { BreakKind::Hard } else { BreakKind::Soft }
}

/// Decode the HTML entities that CommonMark renders as characters.
///
/// `oak-markdown` leaves entities untouched, so a numeric reference such as
/// `&#65;` would reach the IR as literal markup and the HTML renderer would
/// print it a second time (CommonMark §6.2). Named entities outside the XML set
/// are still literal; the coverage matrix tracks that as a fidelity gap.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find('&') {
        decoded.push_str(&rest[..index]);
        let tail = &rest[index..];
        match decode_one_entity(tail) {
            Some((character, consumed)) => {
                decoded.push(character);
                rest = &tail[consumed..];
            }
            None => {
                decoded.push('&');
                rest = &tail[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

/// Decode one entity at the start of `tail` (`&...;`) into a character and the
/// number of consumed bytes.
fn decode_one_entity(tail: &str) -> Option<(char, usize)> {
    let close = tail.find(';')?;
    if close > 32 {
        return None;
    }
    let body = &tail[1..close];
    let consumed = close + 1;
    if let Some(body) = body.strip_prefix('#') {
        let value = if let Some(hex) = body.strip_prefix('x').or_else(|| body.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok()?
        }
        else {
            body.parse::<u32>().ok()?
        };
        return char::from_u32(value).map(|character| (character, consumed));
    }
    let character = match body {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        _ => return None,
    };
    Some((character, consumed))
}

/// Split a single authored line into its text and whether it ends in a hard
/// break. Trailing spaces are markup, not content, in either case.
fn split_hard_break(segment: &str) -> (&str, bool) {
    let trailing_backslashes = segment.chars().rev().take_while(|character| *character == '\\').count();
    if trailing_backslashes % 2 == 1 {
        return (&segment[..segment.len() - 1], true);
    }
    let trimmed = segment.trim_end_matches(' ');
    let trailing_spaces = segment.len() - trimmed.len();
    (trimmed, trailing_spaces >= 2)
}

/// Lower a four-space or tab indented paragraph into a CommonMark indented
/// code block.
///
/// `oak-markdown` has no indented-code node, so the lexer hands the paragraph
/// over with its indentation already stripped and the lowering would emit a
/// normal paragraph. Reading the raw span keeps the code semantics instead of
/// silently flattening the block into prose (CommonMark §4.4).
fn indented_code_block(node: RedNode<MarkdownLanguage>, source: &SourceText) -> Option<Block> {
    let span = node.span();
    let full = source.text();
    let line_start = full[..span.start].rfind('\n').map(|index| index + 1).unwrap_or(0);
    let indent = leading_columns(&full[line_start..span.start]);
    if indent < 4 {
        return None;
    }
    let raw = &full[line_start..span.end];
    let mut body = String::new();
    for line in raw.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let stripped = strip_indent(line, 4);
        body.push_str(stripped);
        body.push('\n');
    }
    let body = body.trim_end_matches('\n').to_string();
    if body.trim().is_empty() {
        return None;
    }
    Some(Block::Code { language: None, content: body })
}

/// Count leading indentation columns, expanding a tab to four columns.
fn leading_columns(line: &str) -> usize {
    let mut columns = 0usize;
    for character in line.chars() {
        match character {
            ' ' => columns += 1,
            '\t' => columns += 4,
            _ => break,
        }
    }
    columns
}

/// Remove up to `columns` leading indentation columns from one line.
fn strip_indent(line: &str, columns: usize) -> &str {
    let mut removed = 0usize;
    let mut index = 0usize;
    for (offset, character) in line.char_indices() {
        match character {
            ' ' => removed += 1,
            '\t' => removed += 4,
            _ => break,
        }
        index = offset + character.len_utf8();
        if removed >= columns {
            break;
        }
    }
    &line[index..]
}

/// A paragraph's final line break terminates the block in the source; it is
/// not authored content and must not survive into the inline stream.
fn trim_trailing_line_break(inlines: &mut Vec<Inline>) {
    if let Some(Inline::Break { kind: BreakKind::Soft }) = inlines.last() {
        inlines.pop();
        return;
    }
    let Some(Inline::Text { text }) = inlines.last_mut()
    else {
        return;
    };
    let trim = if text.ends_with("\r\n") {
        2
    }
    else if text.ends_with('\n') || text.ends_with('\r') {
        1
    }
    else {
        0
    };
    if trim > 0 {
        let length = text.len();
        text.truncate(length - trim);
    }
    if text.is_empty() {
        inlines.pop();
    }
}

/// Strip CommonMark code-span delimiters: a matching backtick run on both
/// sides plus the optional single space padding inside the span.
fn inline_code_text(raw: &str) -> String {
    let trimmed = raw.trim_end();
    let opening = trimmed.chars().take_while(|character| *character == '`').count();
    let closing = trimmed.chars().rev().take_while(|character| *character == '`').count();
    let delimiter = opening.min(closing);
    if delimiter == 0 {
        return trimmed.to_string();
    }
    let Some(inner) = trimmed.get(delimiter..trimmed.len().saturating_sub(delimiter))
    else {
        return trimmed.to_string();
    };
    if inner.len() >= 2 && inner.starts_with(' ') && inner.ends_with(' ') && !inner.trim().is_empty() {
        return inner[1..inner.len() - 1].to_string();
    }
    inner.to_string()
}

fn parse_inline_link(node: RedNode<MarkdownLanguage>, source: &SourceText, context: &mut LinkContext) -> Option<Inline> {
    let raw = node.text(source);
    let span = node.span();
    let site = SourceSpan { start: span.start as u32, end: span.end as u32 };
    if let Some((_, url, title)) = resolve_link_destination(raw.as_ref(), context, site) {
        return Some(Inline::Link { target: RelationEndpoint::ExternalUri(url), display: link_display(raw.as_ref()), title });
    }
    Some(Inline::Text { text: decode_escaped_text(raw.trim_end()) })
}

fn parse_inline_image(node: RedNode<MarkdownLanguage>, source: &SourceText, context: &mut LinkContext) -> Option<Inline> {
    let raw = node.text(source);
    let span = node.span();
    let site = SourceSpan { start: span.start as u32, end: span.end as u32 };
    let Some((alt, url, title)) = resolve_link_destination(raw.as_ref(), context, site)
    else {
        return Some(Inline::Text { text: decode_escaped_text(raw.trim_end()) });
    };
    Some(Inline::Image { source: url, alt: vec![Inline::Text { text: alt }], title, asset: None })
}

/// Resolve an inline or reference destination, recording explicit unresolved
/// labels.
///
/// `site` is the syntactic range of the whole link / image token. The label
/// lookup happens in the same collector pass that owns the node, so an
/// unresolved explicit label can point at the exact source range instead of
/// leaving the loss span-less.
fn resolve_link_destination(
    raw: &str,
    context: &mut LinkContext,
    site: SourceSpan,
) -> Option<(String, String, Option<String>)> {
    if let Some((text, url, title)) = split_markdown_link(raw) {
        return Some((text, url, title));
    }
    if let Some((text, label, explicit)) = split_reference_link(raw.trim_end()) {
        if let Some(definition) = context.definitions.get(&normalize_link_label(&label)) {
            return Some((text, definition.destination.clone(), definition.title.clone()));
        }
        if explicit {
            context.losses.push((
                LossMarker {
                    code: "import.markdown.unresolved_reference_link".into(),
                    message: format!("unresolved reference link label: {label}"),
                    status: SemanticStatus::Unresolved,
                },
                Some(site),
            ));
        }
    }
    None
}

fn split_markdown_link(raw: &str) -> Option<(String, String, Option<String>)> {
    let end = outer_label_end(raw)?;
    let label = raw[..end].trim();
    let text = decode_escaped_text(label.strip_prefix("![").or_else(|| label.strip_prefix('['))?);
    let destination = raw.get(end + 1..)?.strip_prefix('(')?;
    let close = link_destination_close(destination)?;
    let (url, title) = split_destination_and_title(destination[..close].trim());
    // An empty alt text is valid CommonMark (`![](i.png)`), so only the
    // destination decides whether the token is a link or an image.
    if url.is_empty() {
        return None;
    }
    Some((text, url, title))
}

/// Byte index of the `]` that closes the outermost link or image label.
///
/// CommonMark allows balanced brackets inside link text, so `[![a](i.png)](/u)`
/// is one link whose text is an image. Anchoring on the first `](` truncates
/// exactly that construct, so the closing bracket is found by depth instead.
fn outer_label_end(raw: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (index, character) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Link text as inline content, so text nested inside the label keeps its own
/// form instead of collapsing into its markup spelling.
fn link_display(raw: &str) -> Vec<Inline> {
    let Some(end) = outer_label_end(raw)
    else {
        return vec![Inline::Text { text: decode_escaped_text(raw.trim_end()) }];
    };
    let label = raw[..end].trim();
    // The label keeps its own `[` prefix, so strip it before looking at what
    // the link text is made of.
    let inner = label.strip_prefix('[').unwrap_or(label);
    if let Some((alt, url, title)) = split_markdown_link(inner) {
        if inner.starts_with("![") {
            return vec![Inline::Image { source: url, alt: vec![Inline::Text { text: alt }], title, asset: None }];
        }
    }
    let text = inner.strip_prefix("![").or_else(|| inner.strip_prefix('[')).unwrap_or(inner);
    vec![Inline::Text { text: decode_escaped_text(text) }]
}

/// Split a CommonMark destination from its optional trailing title.
///
/// A bare destination ends at the first unescaped whitespace, an angle-bracket
/// destination ends at `>`, and the title must be wrapped in `"`, `'`, or
/// parentheses. Titles are part of the link IR, so they survive a round trip
/// instead of being folded into the URL.
fn split_destination_and_title(raw: &str) -> (String, Option<String>) {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix('<') {
        if let Some(end) = rest.find('>') {
            let title = parse_link_title(rest[end + 1..].trim());
            return (decode_escaped_text(&rest[..end]), title);
        }
        return (decode_escaped_text(raw), None);
    }
    match unescaped_whitespace(raw) {
        Some(index) => (decode_escaped_text(&raw[..index]), parse_link_title(raw[index..].trim())),
        None => (decode_escaped_text(raw), None),
    }
}

fn unescaped_whitespace(text: &str) -> Option<usize> {
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character.is_whitespace() {
            return Some(index);
        }
    }
    None
}

fn parse_link_title(raw: &str) -> Option<String> {
    let mut characters = raw.chars();
    let open = characters.next()?;
    let close = match open {
        '"' => '"',
        '\'' => '\'',
        '(' => ')',
        _ => return None,
    };
    if raw.len() < 2 || !raw.ends_with(close) {
        return None;
    }
    Some(decode_escaped_text(&raw[open.len_utf8()..raw.len() - close.len_utf8()]))
}

fn strip_math_delimiters(text: &str, delimiter: &str) -> String {
    text.trim().strip_prefix(delimiter).and_then(|value| value.strip_suffix(delimiter)).unwrap_or(text.trim()).to_string()
}

fn trailing_link_whitespace(raw: &str) -> String {
    let Some(end) = outer_label_end(raw)
    else {
        return raw[raw.trim_end_matches([' ', '\t']).len()..].to_string();
    };
    let destination = raw.get(end + 1..).and_then(|rest| rest.strip_prefix('(')).unwrap_or("");
    let Some(close) = link_destination_close(destination)
    else {
        return String::new();
    };
    destination[close + 1..].chars().take_while(|character| character.is_whitespace()).collect()
}

fn link_destination_close(destination: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (index, character) in destination.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '(' => depth += 1,
            ')' if depth == 0 => {
                return Some(index);
            }
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

fn is_markdown_marker(text: &str) -> bool {
    matches!(text, "*" | "**" | "_" | "__" | "~~" | "`" | "[" | "]" | "(" | ")" | "![")
}

/// Only an explicit known diagram info string becomes a `Block::Diagram`.
///
/// Unknown or absent language tags stay `Block::Code` instead of being guessed
/// as diagrams, and every listed language must have a lowering frontend.
fn is_diagram_language(language: &str) -> bool {
    language.eq_ignore_ascii_case("mermaid")
        || language.eq_ignore_ascii_case("dot")
        || language.eq_ignore_ascii_case("plantuml")
}

/// Chart fences are promoted to `Block::Chart`, never to `Block::Diagram`.
fn is_chart_language(language: &str) -> bool {
    language.eq_ignore_ascii_case("vega")
        || language.eq_ignore_ascii_case("vega-lite")
        || language.eq_ignore_ascii_case("chart")
}

/// Fenced code keeps the raw source slice between the two fences.
///
/// `oak-markdown` re-lexes fence bodies with the inline lexer, which turns
/// punctuation into dedicated tokens or nodes (`A[Start]` becomes `Text "A"`,
/// `Link "["` and `Text "Start"`). Rebuilding the body token by token dropped
/// every element the lowering did not know about, so the content is taken from
/// the source byte range instead.
///
/// `oak-markdown` closes a fence at the first fence run of the same character
/// regardless of length. CommonMark requires the closing fence to be at least
/// as long as the opening one, so a shorter run inside a longer fence splits
/// the block. The returned `consumed` count is the number of following siblings
/// that the repaired body swallowed.
fn extract_code_block(
    node: RedNode<MarkdownLanguage>,
    source: &SourceText,
    trailing: &[RedTree<MarkdownLanguage>],
) -> (Option<String>, String, usize, usize) {
    let node_span = node.span();
    let mut language = None;
    let mut body_start = None;
    let mut body_end = node_span.end;
    let mut opening = None;
    let mut closing = None;
    for child in node.children() {
        match child.kind::<MarkdownElementType>() {
            MarkdownElementType::CodeFence if body_start.is_none() => {
                opening = fence_run(&child.text(source));
                body_start = Some(child.span().end);
            }
            MarkdownElementType::CodeFence => {
                closing = fence_run(&child.text(source));
                body_end = child.span().start;
                break;
            }
            MarkdownElementType::CodeLanguage if body_start.is_some() && language.is_none() => {
                let text = child.text(source);
                let tag = text.trim();
                if !tag.is_empty() {
                    language = Some(tag.to_string());
                }
                body_start = Some(child.span().end);
            }
            _ => {}
        }
    }
    if let (Some(opening), Some(closing), Some(start)) = (opening, closing, body_start) {
        if closing.length < opening.length {
            if let Some((close_start, close_end)) = find_closing_fence(source, node_span.end, opening) {
                let raw = source.slice((start.min(close_start)..close_start).into());
                let consumed = trailing.iter().take_while(|child| child.span().start < close_end).count();
                let body_offset = start.min(close_start) + fence_body_offset(&raw);
                return (language, strip_fence_line_remainder(&raw).to_string(), consumed, body_offset);
            }
        }
    }
    let start = body_start.unwrap_or(node_span.start).min(body_end);
    let raw = source.slice((start..body_end).into());
    (language, strip_fence_line_remainder(&raw).to_string(), 0, start + fence_body_offset(&raw))
}

#[derive(Clone, Copy)]
struct FenceRun {
    character: char,
    length: usize,
}

fn fence_run(text: &str) -> Option<FenceRun> {
    let trimmed = text.trim_start_matches([' ', '\t']);
    let character = trimmed.chars().next()?;
    if character != '`' && character != '~' {
        return None;
    }
    let length = trimmed.chars().take_while(|item| *item == character).count();
    (length > 0).then_some(FenceRun { character, length })
}

/// Find the first line at or after `from` that closes `opening` per CommonMark:
/// only the same fence character repeated at least as many times, with no info
/// string.
fn find_closing_fence(source: &SourceText, from: usize, opening: FenceRun) -> Option<(usize, usize)> {
    let text = source.text();
    let length = text.len();
    let mut offset = from.min(length);
    while offset < length {
        let rest = &text[offset..];
        let line_end = rest.find(['\r', '\n']).map(|index| offset + index).unwrap_or(length);
        let line = &text[offset..line_end];
        if let Some(run) = fence_run(line) {
            let remainder = &line.trim_start_matches([' ', '\t'])[run.length..];
            if run.character == opening.character && run.length >= opening.length && remainder.trim().is_empty() {
                let after = if line_end >= length {
                    line_end
                }
                else if rest[line_end - offset..].starts_with("\r\n") {
                    line_end + 2
                }
                else {
                    line_end + 1
                };
                return Some((offset, after.min(length)));
            }
        }
        if line_end >= length {
            break;
        }
        offset = line_end + 1;
    }
    None
}

/// A fence body starts on the line after the opening fence and its info
/// string, and ends before the line break that precedes the closing fence.
fn strip_fence_line_remainder(text: &str) -> &str {
    let Some(index) = text.find(['\r', '\n'])
    else {
        return "";
    };
    let text = strip_leading_line_break(&text[index..]);
    // The closing fence may itself be indented by up to three spaces, and that
    // indentation is not part of the body.
    strip_trailing_line_break(text.trim_end_matches([' ', '\t']))
}

fn strip_leading_line_break(text: &str) -> &str {
    text.strip_prefix("\r\n").or_else(|| text.strip_prefix('\n')).or_else(|| text.strip_prefix('\r')).unwrap_or(text)
}

/// Offset of the stripped fence body inside the raw slice.
fn fence_body_offset(raw: &str) -> usize {
    match raw.find(['\r', '\n']) {
        Some(index) => index + (raw[index..].len() - strip_leading_line_break(&raw[index..]).len()),
        None => 0,
    }
}

fn strip_trailing_line_break(text: &str) -> &str {
    text.strip_suffix("\r\n").or_else(|| text.strip_suffix('\n')).or_else(|| text.strip_suffix('\r')).unwrap_or(text)
}

/// GFM footnote definitions continue on the immediately following lines when
/// those lines are indented by at least four spaces. `oak-markdown` ends the
/// definition at the first line, so the continuation arrives as a separate
/// paragraph and would move into the document body. Append its raw source to
/// the definition payload instead.
fn append_footnote_continuation(
    graph: &mut DocumentGraph,
    block_index: usize,
    definition_end: usize,
    node: RedNode<MarkdownLanguage>,
    source: &SourceText,
) -> bool {
    let continuation = source.slice((definition_end..node.span().end).into());
    let after_break = strip_leading_line_break(continuation.as_ref());
    if after_break.len() == continuation.len() || after_break.starts_with(['\n', '\r']) {
        return false;
    }
    if !(after_break.starts_with("    ") || after_break.starts_with('\t')) {
        return false;
    }
    let Some(Block::Opaque { payload_hint, .. }) = graph.blocks.get_mut(block_index).map(|item| &mut item.block)
    else {
        return false;
    };
    payload_hint.push('\n');
    payload_hint.push_str(strip_trailing_line_break(after_break));
    true
}

fn list_item_marker(node: RedNode<MarkdownLanguage>, source: &SourceText) -> Option<String> {
    node.children()
        .find(|child| child.kind::<MarkdownElementType>() == MarkdownElementType::ListMarker)
        .map(|child| child.text(source).trim().to_string())
}

/// One list level that is still open while flattened items are placed.
struct OpenList {
    indent: usize,
    ordered: bool,
    id: NodeId,
}

/// One list item flattened out of the `oak-markdown` tree with its authored
/// indentation.
///
/// `oak-markdown` nests list nodes by tree shape rather than by authored
/// indentation: a parent list node ends at the first indented line, deeper
/// sibling runs come back as separate root-level `List` nodes, and a shallower
/// item can end up inside the previous item's node. Rebuilding the levels from
/// the marker columns is the only way to keep every sibling on its own level.
struct FlatItem {
    indent: usize,
    ordered: bool,
    number: Option<u32>,
    is_task: bool,
    is_checked: Option<bool>,
    content: Vec<Inline>,
}

/// Authored column indentation of a byte offset, ignoring blockquote markers.
fn indent_column(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    let line_start = text.get(..offset).and_then(|head| head.rfind(['\n', '\r'])).map(|index| index + 1).unwrap_or(0);
    let mut columns = 0;
    for character in text.get(line_start..offset).unwrap_or("").chars() {
        match character {
            '>' => columns = 0,
            ' ' => columns += 1,
            '\t' => columns = (columns / 4 + 1) * 4,
            _ => break,
        }
    }
    columns
}

/// Authored column indentation of one list item's marker.
fn item_indent(node: RedNode<MarkdownLanguage>, source: &SourceText) -> usize {
    let text = source.text();
    node.children()
        .find(|child| child.kind::<MarkdownElementType>() == MarkdownElementType::ListMarker)
        .map(|marker| indent_column(text, marker.span().start))
        .unwrap_or(0)
}

fn list_items_mut(graph: &mut DocumentGraph, id: NodeId) -> Option<&mut Vec<ListItem>> {
    let index = graph.blocks.iter().position(|node| node.id == id)?;
    match &mut graph.blocks[index].block {
        Block::List { items, .. } => Some(items),
        _ => None,
    }
}

fn attach_list_to_last_item(graph: &mut DocumentGraph, parent: NodeId, child: NodeId) {
    if let Some(items) = list_items_mut(graph, parent) {
        if let Some(last) = items.last_mut() {
            last.children.push(child);
        }
    }
}

/// Flatten a `List` node into source-ordered items with their indentation.
///
/// Nested `List` children are recursed instead of becoming child blocks, so a
/// run that `oak-markdown` split across several root nodes is rebuilt as one
/// sequence before the levels are reconstructed.
fn collect_list_items(
    node: RedNode<MarkdownLanguage>,
    source: &SourceText,
    context: &mut LinkContext,
    out: &mut Vec<FlatItem>,
) {
    for child in node.children() {
        let RedTree::Node(item_node) = child
        else {
            continue;
        };
        if item_node.element_type() != MarkdownElementType::ListItem {
            continue;
        }
        let marker = list_item_marker(item_node, source);
        let ordered =
            marker.as_deref().and_then(|value| value.chars().next()).is_some_and(|character| character.is_ascii_digit());
        let number = if ordered {
            marker
                .as_deref()
                .map(|value| value.chars().take_while(char::is_ascii_digit).collect::<String>())
                .and_then(|digits| digits.parse::<u32>().ok())
        }
        else {
            None
        };
        let indent = item_indent(item_node, source);
        let (is_task, is_checked) = extract_task_marker(item_node, source);
        let (content, nested) = lower_list_item(item_node, source, context);
        out.push(FlatItem { indent, ordered, number, is_task, is_checked, content });
        for nested_node in nested {
            collect_list_items(nested_node, source, context, out);
        }
    }
}

/// Place flattened items back into list blocks by authored indentation.
///
/// A shallower item closes the deeper levels, an item at the innermost level
/// continues that list, and an item with a different marker type starts a new
/// list at its own level. The stack persists across root-level `List` nodes so
/// a continuation split by the parser still lands inside its parent.
fn place_flat_items(graph: &mut DocumentGraph, stack: &mut Vec<OpenList>, flat: Vec<FlatItem>) {
    for item in flat {
        while stack.last().is_some_and(|open| open.indent > item.indent) {
            stack.pop();
        }
        let continuation = match stack.last() {
            Some(open) if open.indent == item.indent => {
                if open.ordered == item.ordered {
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
                let id = graph.push_block(Block::List { ordered: item.ordered, items: Vec::new() });
                if let Some(parent) = stack.last() {
                    attach_list_to_last_item(graph, parent.id, id);
                }
                stack.push(OpenList { indent: item.indent, ordered: item.ordered, id });
                id
            }
        };
        if let Some(items) = list_items_mut(graph, id) {
            items.push(ListItem {
                content: item.content,
                children: Vec::new(),
                number: item.number,
                is_task: item.is_task,
                is_checked: item.is_checked,
            });
        }
    }
}

/// Scan a `ListItem` node for a `TaskMarker` child (`[x]` / `[ ]` / `[X]`).
///
/// Returns `(is_task, is_checked)`. A plain list item yields `(false, None)`.
fn extract_task_marker(node: RedNode<MarkdownLanguage>, source: &SourceText) -> (bool, Option<bool>) {
    for child in node.children() {
        let RedTree::Node(child_node) = child
        else {
            continue;
        };
        if child_node.element_type() == MarkdownElementType::TaskMarker {
            let text = child_node.text(source);
            let checked = text.contains('x') || text.contains('X');
            return (true, Some(checked));
        }
    }
    (false, None)
}

fn lower_list_item<'a>(
    node: RedNode<'a, MarkdownLanguage>,
    source: &SourceText,
    context: &mut LinkContext,
) -> (Vec<Inline>, Vec<RedNode<'a, MarkdownLanguage>>) {
    let mut content = Vec::new();
    let mut children = Vec::new();
    let mut skip_marker_space = false;
    let mut line_start = false;
    for child in node.children() {
        match child {
            RedTree::Leaf(_) => match child.kind::<MarkdownElementType>() {
                MarkdownElementType::ListMarker => skip_marker_space = true,
                MarkdownElementType::Newline => {
                    skip_marker_space = false;
                    line_start = true;
                }
                _ => {
                    let text = child.text(source);
                    if (skip_marker_space || line_start) && text.trim().is_empty() {
                        skip_marker_space = false;
                        line_start = false;
                        continue;
                    }
                    skip_marker_space = false;
                    line_start = false;
                    if !text.is_empty() {
                        push_inline(&mut content, Inline::Text { text: decode_escaped_text(text.as_ref()) });
                    }
                }
            },
            RedTree::Node(child_node) => {
                skip_marker_space = false;
                line_start = false;
                if child_node.element_type() == MarkdownElementType::TaskMarker {
                    // The task marker `[x]` / `[ ]` is already captured by
                    // `extract_task_marker`. Do not push its text into content
                    // and skip the trailing whitespace that follows it.
                    skip_marker_space = true;
                }
                else if child_node.element_type() == MarkdownElementType::List {
                    // Nested lists are flattened and rebuilt by indentation, so
                    // they are handed back instead of becoming child blocks here.
                    children.push(child_node);
                }
                else if let Some(inline) = lower_inline_node(child_node, source, context) {
                    push_inline(&mut content, inline);
                }
            }
        }
    }
    (content, children)
}

/// Collect the complete HTML block that starts at `node`.
///
/// `oak-markdown` emits only the opening tag as an `HtmlTag` node and leaves the
/// remaining lines as paragraph text, so the closing tag would be escaped on
/// export. CommonMark ends an HTML block at the first blank line, which makes
/// the block the contiguous run of siblings after the opening tag.
fn extract_html_block(
    node: RedNode<MarkdownLanguage>,
    trailing: &[RedTree<MarkdownLanguage>],
    source: &SourceText,
) -> (String, usize) {
    let text = source.text();
    let start = node.span().start;
    let end = html_block_terminator(text, start);
    let mut consumed = 0;
    for child in trailing {
        let child_end = match child {
            RedTree::Node(candidate) => candidate.span().end,
            RedTree::Leaf(leaf) => leaf.span().end,
        };
        if child_end > end {
            break;
        }
        consumed += 1;
    }
    (text.get(start.min(text.len())..end.min(text.len())).unwrap_or("").to_owned(), consumed)
}

/// Byte offset of the blank line that ends a raw HTML block.
///
/// CommonMark ends an HTML block at the first line that holds only whitespace.
/// The blank line itself belongs to the following content, so the returned
/// offset is the start of that line. `oak-markdown` emits the blank line as two
/// separate `Newline` leaves, which a sibling gap check never sees as `\n\n`,
/// so the scan works on the source instead of the sibling spans.
fn html_block_terminator(text: &str, start: usize) -> usize {
    let length = text.len();
    let mut line_start = start.min(length);
    let mut first_line = true;
    while line_start < length {
        let line_end =
            text.get(line_start..).and_then(|rest| rest.find(['\n', '\r'])).map(|index| line_start + index).unwrap_or(length);
        if !first_line && text.get(line_start..line_end).unwrap_or("").trim().is_empty() {
            return line_start;
        }
        first_line = false;
        if line_end >= length {
            return length;
        }
        line_start = line_end + if text.get(line_end..).is_some_and(|rest| rest.starts_with("\r\n")) { 2 } else { 1 };
    }
    length
}

/// Whether a lexed HTML token is an authored tag rather than a stray `<`.
///
/// `oak-markdown` lexes a bare `<` such as `1 < 2` as an HTML tag and lets the
/// run swallow the rest of the document. Only tokens that start like a tag are
/// kept verbatim, so a comparison operator stays text and is escaped on export.
fn is_authored_html_tag(text: &str) -> bool {
    if text.contains('\n') {
        return false;
    }
    let Some(rest) = text.strip_prefix('<')
    else {
        return false;
    };
    match rest.chars().next() {
        Some(first) if first.is_ascii_alphabetic() => true,
        Some('!') | Some('?') => true,
        Some('/') => matches!(rest[1..].chars().next(), Some(first) if first.is_ascii_alphabetic()),
        _ => false,
    }
}

/// Recover a CommonMark autolink that the lexer reports as an HTML tag.
///
/// Autolinks open with `<`, so the lexer marks `<https://example.com>` as an
/// authored tag. Keeping it as raw HTML hides the URL from every semantic
/// renderer, so the destination and display text are reconstructed here.
fn autolink_target(text: &str) -> Option<(String, String)> {
    let text = text.trim();
    let inner = text.strip_prefix('<')?.strip_suffix('>')?;
    if inner.is_empty() || inner.chars().any(|character| character.is_whitespace() || character == '<' || character == '>') {
        return None;
    }
    if let Some((scheme, rest)) = inner.split_once(':') {
        let scheme_ok = (2..=32).contains(&scheme.len())
            && scheme.chars().next().is_some_and(|character| character.is_ascii_alphabetic())
            && scheme.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'));
        if scheme_ok && !rest.is_empty() {
            return Some((inner.to_owned(), inner.to_owned()));
        }
    }
    let (local, domain) = inner.split_once('@')?;
    if local.is_empty() || domain.is_empty() || !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
        return None;
    }
    Some((format!("mailto:{inner}"), inner.to_owned()))
}

/// The loss marker for HTML the semantic reflow cannot lay out.
fn raw_html_loss_marker() -> LossMarker {
    LossMarker {
        code: "import.markdown.raw_html_unrendered".into(),
        message: "raw HTML is preserved for HTML/EPUB export but semantic SVG/PDF reflow does not lay it out".into(),
        status: SemanticStatus::Partial,
    }
}

/// The loss marker for a footnote body flow targets cannot place.
fn footnote_body_loss_marker() -> LossMarker {
    LossMarker {
        code: "import.markdown.footnote_body_unplaced".into(),
        message: "footnote body is kept as an opaque payload because flow targets have no footnote collection".into(),
        status: SemanticStatus::Partial,
    }
}

/// The loss marker for a footnote reference reduced to its textual marker.
fn footnote_reference_loss_marker() -> LossMarker {
    LossMarker {
        code: "import.markdown.footnote_reference_marker".into(),
        message: "footnote reference renders as a textual `[^label]` marker because flow targets have no footnote apparatus"
            .into(),
        status: SemanticStatus::Partial,
    }
}

/// Lower a definition description into paragraph inlines.
///
/// `oak-markdown` keeps the `:` marker in the node, so it is removed here while
/// the authored description text keeps its inline formatting.
fn definition_description_inlines(
    node: RedNode<MarkdownLanguage>,
    source: &SourceText,
    context: &mut LinkContext,
) -> Vec<Inline> {
    let mut content = collect_inlines(node, source, context);
    trim_trailing_line_break(&mut content);
    if let Some(Inline::Text { text }) = content.first_mut() {
        let stripped = text.strip_prefix(':').map(|rest| rest.trim_start_matches([' ', '\t'])).unwrap_or(text.as_str());
        *text = stripped.to_string();
    }
    if matches!(content.first(), Some(Inline::Text { text }) if text.is_empty()) {
        content.remove(0);
    }
    content
}

/// Lower a standalone HTML table block into the IR table.
///
/// Only a block that is exactly one `<table>` element is lowered. Anything
/// else stays verbatim raw HTML, so authored markup is never re-read
/// opportunistically.
#[cfg(feature = "html")]
fn lower_html_table_block(html: &str, graph: &mut DocumentGraph) -> Option<Block> {
    let trimmed = html.trim();
    if !trimmed.starts_with("<table") || !trimmed.ends_with("</table>") {
        return None;
    }
    let imported = crate::import::html::import_html_text("markdown-html-table", trimmed).ok()?;
    let mut blocks = imported.blocks.into_iter();
    let first = blocks.next()?;
    if blocks.next().is_some() {
        return None;
    }
    match first.block {
        block @ Block::Table { .. } => {
            // The nested HTML import flattens merged cells. Carry the table
            // losses back so the reopen never downgrades silently.
            for loss in imported
                .coverage
                .loss
                .into_iter()
                .filter(|loss| matches!(loss.code.as_str(), "import.html.table_spans" | "import.html.invalid_cell_span"))
            {
                graph.push_loss(loss);
            }
            Some(block)
        }
        _ => None,
    }
}

/// Lower a blockquote into a quote whose children carry its whole body.
///
/// `oak-markdown` keeps a blockquote's `>` markers and inner list/heading
/// markers inside one flat node, so nested structure cannot be read from the
/// tree. The dequoted body is re-read with the same import profile: a body that
/// lowers to paragraphs becomes a real `Paragraph` child, and any deeper block
/// (list, heading, table, code, nested quote) becomes another quote child.
fn lower_blockquote(
    node: RedNode<MarkdownLanguage>,
    source: &SourceText,
    context: &mut LinkContext,
    graph: &mut DocumentGraph,
) -> Block {
    let raw = node.text(source);
    let body = strip_blockquote_markers(&raw);
    if body != raw && !body.trim().is_empty() {
        if let Ok(inner) = import_markdown_bytes("blockquote", &body) {
            let mut payloads = root_block_payloads(&inner);
            let mut children = Vec::new();
            if let Some(Block::Paragraph { content: paragraph }) = payloads.first().cloned() {
                let content = split_text_newlines(paragraph);
                if !content.is_empty() {
                    children.push(graph.push_block(Block::Paragraph { content }));
                }
                payloads.remove(0);
            }
            children.extend(payloads.into_iter().map(|block| {
                let rehosted = rehost_block(&block, &inner, graph);
                graph.push_block(rehosted)
            }));
            if !children.is_empty() {
                return Block::Quote { children };
            }
        }
    }
    let content = lower_blockquote_inlines(node, source, context);
    let children = if content.is_empty() { Vec::new() } else { vec![graph.push_block(Block::Paragraph { content })] };
    Block::Quote { children }
}

/// Split raw newlines inside text runs into soft breaks so quoted inline
/// content keeps the same shape as a directly lowered paragraph.
fn split_text_newlines(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out = Vec::with_capacity(inlines.len());
    for inline in inlines {
        if let Inline::Text { text } = &inline {
            if text.contains('\n') {
                for (index, part) in text.split('\n').enumerate() {
                    if index > 0 {
                        push_soft_break(&mut out);
                    }
                    if !part.is_empty() {
                        out.push(Inline::Text { text: part.to_owned() });
                    }
                }
                continue;
            }
        }
        out.push(inline);
    }
    out
}

/// Copy a block payload from a freshly imported graph into the target graph.
///
/// Nested blocks carry [`NodeId`] values that belong to the source graph, so a
/// payload cannot be moved verbatim across graphs. Children are re-pushed and
/// remapped here before the block itself is materialized.
fn rehost_block(block: &Block, source_graph: &DocumentGraph, graph: &mut DocumentGraph) -> Block {
    match block {
        Block::Section { level, title, children } => {
            Block::Section { level: *level, title: title.clone(), children: rehost_children(children, source_graph, graph) }
        }
        Block::Quote { children } => Block::Quote { children: rehost_children(children, source_graph, graph) },
        Block::List { ordered, items } => Block::List {
            ordered: *ordered,
            items: items
                .iter()
                .map(|item| ListItem {
                    content: item.content.clone(),
                    children: rehost_children(&item.children, source_graph, graph),
                    number: item.number,
                    is_task: item.is_task,
                    is_checked: item.is_checked,
                })
                .collect(),
        },
        other => other.clone(),
    }
}

fn rehost_children(children: &[NodeId], source_graph: &DocumentGraph, graph: &mut DocumentGraph) -> Vec<NodeId> {
    children
        .iter()
        .filter_map(|child_id| source_graph.block(*child_id))
        .map(|node| {
            let block = rehost_block(&node.block, source_graph, graph);
            graph.push_block(block)
        })
        .collect()
}

/// Remove one `>` marker per line so the quoted body can be parsed again.
fn strip_blockquote_markers(raw: &str) -> String {
    let mut body = String::with_capacity(raw.len());
    for line in raw.split_inclusive('\n') {
        let (line, newline) = match line.strip_suffix('\n') {
            Some(line) => (line, "\n"),
            None => (line, ""),
        };
        let trimmed = line.trim_start_matches([' ', '\t']);
        let stripped = trimmed.strip_prefix('>').unwrap_or(trimmed);
        let stripped = stripped.strip_prefix(' ').unwrap_or(stripped);
        body.push_str(stripped);
        body.push_str(newline);
    }
    body
}

/// Payloads of the top-level blocks of a freshly imported graph.
fn root_block_payloads(graph: &DocumentGraph) -> Vec<Block> {
    let mut contained = std::collections::HashSet::new();
    for node in &graph.blocks {
        match &node.block {
            Block::Section { children, .. } | Block::Quote { children, .. } => contained.extend(children.iter().copied()),
            Block::List { items, .. } => {
                for item in items {
                    contained.extend(item.children.iter().copied());
                }
            }
            _ => {}
        }
    }
    graph.blocks.iter().filter(|node| !contained.contains(&node.id)).map(|node| node.block.clone()).collect()
}

fn lower_blockquote_inlines(node: RedNode<MarkdownLanguage>, source: &SourceText, context: &mut LinkContext) -> Vec<Inline> {
    let children = node.children().collect::<Vec<_>>();
    let mut inlines = Vec::new();
    let mut skip_marker_space = false;
    let mut line_start = false;
    for (index, child) in children.iter().enumerate() {
        match *child {
            RedTree::Leaf(_) => match child.kind::<MarkdownElementType>() {
                MarkdownElementType::BlockquoteMarker => skip_marker_space = true,
                MarkdownElementType::Newline => {
                    skip_marker_space = false;
                    line_start = true;
                    if index + 1 < children.len() {
                        push_soft_break(&mut inlines);
                    }
                }
                _ => {
                    let text = child.text(source);
                    if (skip_marker_space || line_start) && text.trim().is_empty() {
                        skip_marker_space = false;
                        line_start = false;
                        continue;
                    }
                    skip_marker_space = false;
                    line_start = false;
                    if !text.is_empty() {
                        push_inline(&mut inlines, Inline::Text { text: decode_escaped_text(text.as_ref()) });
                    }
                }
            },
            RedTree::Node(child_node) => {
                skip_marker_space = false;
                line_start = false;
                match child_node.element_type() {
                    MarkdownElementType::Blockquote | MarkdownElementType::Paragraph => {
                        push_soft_break(&mut inlines);
                        let nested = if child_node.element_type() == MarkdownElementType::Blockquote {
                            lower_blockquote_inlines(child_node, source, context)
                        }
                        else {
                            let mut content = collect_inlines(child_node, source, context);
                            trim_trailing_line_break(&mut content);
                            content
                        };
                        for inline in nested {
                            push_inline(&mut inlines, inline);
                        }
                    }
                    _ => {
                        if let Some(inline) = lower_inline_node(child_node, source, context) {
                            push_inline(&mut inlines, inline);
                        }
                    }
                }
            }
        }
    }
    inlines
}

fn extract_table(node: RedNode<MarkdownLanguage>, source: &SourceText, context: &mut LinkContext) -> Vec<TableRow> {
    let mut rows = Vec::new();
    let mut header_pending = true;
    for child in node.children() {
        let RedTree::Node(row_node) = child
        else {
            continue;
        };
        if row_node.element_type() != MarkdownElementType::TableRow {
            continue;
        }
        let mut cells = Vec::new();
        for cell_child in row_node.children() {
            let RedTree::Node(cell_node) = cell_child
            else {
                continue;
            };
            if cell_node.element_type() != MarkdownElementType::TableCell {
                continue;
            }
            let content = decode_table_cell_inline_code(trim_inline_edges(collect_inlines(cell_node, source, context)));
            cells.push(TableCell { content, colspan: 1, rowspan: 1 });
        }
        if cells.last().is_some_and(|cell| inline_text(&cell.content).is_empty()) {
            cells.pop();
        }
        if cells.is_empty() || is_table_separator_row(&cells) {
            if cells.iter().any(|cell| inline_text(&cell.content).contains(':')) {
                let span = row_node.span();
                context.losses.push((
                    LossMarker {
                        code: "import.markdown.table_alignment".into(),
                        message:
                            "GFM column alignment is not represented in the table IR, so the reopened table uses default alignment"
                                .into(),
                        status: SemanticStatus::Partial,
                    },
                    Some(SourceSpan { start: span.start as u32, end: span.end as u32 }),
                ));
            }
            continue;
        }
        let is_header = header_pending;
        header_pending = false;
        rows.push(TableRow { is_header, cells });
    }
    rows
}

fn is_table_separator_row(cells: &[TableCell]) -> bool {
    let text = cells.iter().flat_map(|cell| cell.content.iter().map(Inline::plain_text)).collect::<String>();
    let visible = text.trim();
    !visible.is_empty() && visible.contains('-') && visible.chars().all(|character| matches!(character, '-' | ':'))
}

fn trim_inline_edges(mut inlines: Vec<Inline>) -> Vec<Inline> {
    if let Some(Inline::Text { text }) = inlines.first_mut() {
        *text = text.trim_start().to_string();
    }
    if let Some(Inline::Text { text }) = inlines.last_mut() {
        *text = text.trim_end().to_string();
    }
    inlines.retain(|inline| !matches!(inline, Inline::Text { text } if text.is_empty()));
    inlines
}

/// Inside a GFM table row a `\|` is a literal pipe even in a code span, so the
/// code text is decoded on the way into the IR.
fn decode_table_cell_inline_code(inlines: Vec<Inline>) -> Vec<Inline> {
    inlines
        .into_iter()
        .map(|inline| match inline {
            Inline::InlineCode { text } => Inline::InlineCode { text: text.replace("\\|", "|") },
            Inline::Styled { style, children } => Inline::Styled { style, children: decode_table_cell_inline_code(children) },
            other => other,
        })
        .collect()
}

fn inline_text(inlines: &[Inline]) -> String {
    inlines.iter().map(Inline::plain_text).collect()
}

fn heading_level(kind: MarkdownElementType) -> u8 {
    match kind {
        MarkdownElementType::Heading1 => 1,
        MarkdownElementType::Heading2 => 2,
        MarkdownElementType::Heading3 => 3,
        MarkdownElementType::Heading4 => 4,
        MarkdownElementType::Heading5 => 5,
        MarkdownElementType::Heading6 => 6,
        _ => 1,
    }
}

fn document_id_for(label: &str) -> DocumentId {
    let mut hash = 1u64;
    for byte in label.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    DocumentId(hash)
}
