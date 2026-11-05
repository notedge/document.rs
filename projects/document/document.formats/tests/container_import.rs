//! Container import regressions: HTML footnote endnotes, inline style/image
//! sizes, structured tables, and EPUB ordered-list numbering with fail-closed
//! CSS diagnostics.

#![cfg(all(feature = "html", feature = "epub", feature = "markdown"))]

use std::path::PathBuf;

use document_formats::{
    export::markdown::export_markdown,
    import::{
        epub::import_epub_bytes,
        html::{import_html_bytes, import_html_text},
    },
};
use document_ir::{Block, DocumentGraph, Inline, SemanticStatus};

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(path)
}

fn read_fixture(path: &str) -> Vec<u8> {
    std::fs::read(fixture(path)).unwrap_or_else(|error| panic!("read fixture `{path}`: {error}"))
}

fn import_fixture_html(path: &str) -> DocumentGraph {
    import_html_bytes(path, &read_fixture(&format!("html/{path}"))).expect("html import")
}

fn loss_codes(graph: &DocumentGraph) -> Vec<String> {
    graph.coverage.loss.iter().map(|loss| loss.code.clone()).collect()
}

fn loss(graph: &DocumentGraph, code: &str) -> SemanticStatus {
    graph
        .coverage
        .loss
        .iter()
        .find(|loss| loss.code == code)
        .unwrap_or_else(|| panic!("missing loss `{code}` in {:?}", loss_codes(graph)))
        .status
}

fn footnote_definitions(graph: &DocumentGraph) -> Vec<&str> {
    graph
        .blocks
        .iter()
        .filter_map(|node| match &node.block {
            Block::Opaque { kind, payload_hint, .. } if kind == "footnote_definition" => Some(payload_hint.as_str()),
            _ => None,
        })
        .collect()
}

fn note_labels(graph: &DocumentGraph) -> Vec<String> {
    fn scan(inlines: &[Inline], labels: &mut Vec<String>) {
        for inline in inlines {
            match inline {
                Inline::Note { label, .. } => labels.push(label.clone()),
                Inline::Styled { children, .. } | Inline::Attributed { children, .. } => scan(children, labels),
                Inline::Link { display, .. } => scan(display, labels),
                _ => {}
            }
        }
    }
    let mut labels = Vec::new();
    for node in &graph.blocks {
        match &node.block {
            Block::Paragraph { content } | Block::Quote { content, .. } => scan(content, &mut labels),
            Block::Section { title, .. } => scan(title, &mut labels),
            Block::List { items, .. } => {
                for item in items {
                    scan(&item.content, &mut labels);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        scan(&cell.content, &mut labels);
                    }
                }
            }
            _ => {}
        }
    }
    labels
}

fn has_attributed_class(graph: &DocumentGraph, class: &str) -> bool {
    fn scan(inlines: &[Inline], class: &str) -> bool {
        inlines.iter().any(|inline| match inline {
            Inline::Attributed { attributes, children } => {
                attributes.classes.iter().any(|token| token == class) || scan(children, class)
            }
            Inline::Styled { children, .. } => scan(children, class),
            Inline::Link { display, .. } => scan(display, class),
            _ => false,
        })
    }
    graph.blocks.iter().any(|node| match &node.block {
        Block::Paragraph { content } => scan(content, class),
        Block::Section { title, .. } => scan(title, class),
        Block::Quote { content, .. } => scan(content, class),
        Block::List { items, .. } => items.iter().any(|item| scan(&item.content, class)),
        Block::Table { rows, .. } => rows.iter().any(|row| row.cells.iter().any(|cell| scan(&cell.content, class))),
        _ => false,
    })
}

fn collect_blocks<'a>(graph: &'a DocumentGraph, filter: &dyn Fn(&Block) -> bool) -> Vec<&'a Block> {
    graph.blocks.iter().map(|node| &node.block).filter(|block| filter(*block)).collect()
}

fn ordered_list_starting_with<'a>(graph: &'a DocumentGraph, marker: &str) -> &'a Block {
    graph
        .blocks
        .iter()
        .map(|node| &node.block)
        .find(|block| {
            matches!(block, Block::List { ordered: true, items }
                if matches!(items.first().map(|item| item.content.as_slice()), Some([Inline::Text { text }]) if text == marker))
        })
        .unwrap_or_else(|| panic!("ordered list starting with `{marker}`: {:?}", graph.blocks))
}

fn item_numbers(list: &Block) -> Vec<Option<u32>> {
    let Block::List { items, .. } = list else {
        panic!("not a list: {list:?}")
    };
    items.iter().map(|item| item.number).collect()
}

/// A semantic `<table>` lowers to `Block::Table`; the raw markup never survives
/// as `Block::RawHtml`.
#[test]
fn html_table_lowers_to_a_structured_table() {
    let graph = import_fixture_html("table.html");
    let tables = collect_blocks(&graph, &|block| matches!(block, Block::Table { .. }));
    assert_eq!(tables.len(), 1, "{:?}", collect_blocks(&graph, &|_| true));
    let Block::Table { caption, rows } = tables[0]
    else {
        unreachable!()
    };
    assert_eq!(caption.as_deref(), Some("Table 1. Coverage by region."), "{caption:?}");
    assert_eq!(rows.len(), 4, "{rows:?}");
    assert!(rows[0].is_header);
    assert!(rows[1].is_header);
    assert!(rows.iter().all(|row| row.cells.iter().all(|cell| cell.colspan == 1 && cell.rowspan == 1)));
    assert_eq!(loss(&graph, "import.html.table_spans"), SemanticStatus::Partial, "{:?}", loss_codes(&graph));
    let raw_tables: Vec<&Block> = collect_blocks(&graph, &|block| {
        matches!(block, Block::RawHtml { html } if html.to_ascii_lowercase().contains("<table"))
    });
    assert!(raw_tables.is_empty(), "table stayed raw: {raw_tables:?}");
    let markdown = export_markdown(&graph).expect("markdown export");
    assert!(!markdown.contains("<table"), "markdown kept a raw table: {markdown}");
    assert!(markdown.contains("| Region"), "{markdown}");
    assert!(markdown.contains("North"), "{markdown}");
}

/// Endnotes trailing the last section, including inside `<footer>`/`<aside>`,
/// are definitions rather than inline content that leaks `id`/`class`.
#[test]
fn html_endnotes_after_the_last_section_stay_definitions() {
    for fixture_name in ["div-footnotes.html", "early-footnotes.html"] {
        let graph = import_fixture_html(fixture_name);
        assert_eq!(footnote_definitions(&graph).len(), 2, "{fixture_name}: {:?}", graph.blocks);
        assert!(!has_attributed_class(&graph, "footnotes"), "{fixture_name}: endnotes class leaked into inlines");
        let markdown = export_markdown(&graph).expect("markdown export");
        assert!(markdown.contains("[^1]"), "{fixture_name}: {markdown}");
        assert!(markdown.contains("[^2]"), "{fixture_name}: {markdown}");
        assert!(markdown.contains("[^1]: Note one."), "{fixture_name}: {markdown}");
        assert!(markdown.contains("[^2]: Note two."), "{fixture_name}: {markdown}");
    }
}

/// Both authored reference shapes and the `[^label]` text collapse to a note
/// with no custom display, so the Markdown writer emits a real reference.
#[test]
fn html_footnote_reference_shapes_collapse_to_notes() {
    let cases = [
        (
            "exporter-sup",
            r##"<html><body><p>Body<sup class="footnote-reference" data-footnote-label="1">1</sup>.</p><section class="footnotes" role="doc-endnotes"><ol><li id="fn-1"><p>one</p></li></ol></section></body></html>"##,
        ),
        (
            "gfm-anchor",
            r##"<html><body><p>Body<a class="footnote-ref" href="#fn-1">[^1]</a>.</p><div class="footnotes"><ol><li id="fn-1">one</li></ol></div></body></html>"##,
        ),
        (
            "sup-anchor",
            r##"<html><body><p>Body<sup class="footnote-reference" data-footnote-label="1"><a href="#fn-1">[^1]</a></sup>.</p><div class="footnotes"><ol><li id="fn-1">one</li></ol></div></body></html>"##,
        ),
    ];
    for (name, source) in cases {
        let graph = import_html_text(name, source).expect("html import");
        let marked = graph.blocks.iter().any(|node| match &node.block {
            Block::Paragraph { content } => content.iter().any(|inline| matches!(inline, Inline::Note { display: None, .. })),
            _ => false,
        });
        assert!(marked, "{name}: {:?}", graph.blocks);
        let markdown = export_markdown(&graph).expect("markdown export");
        assert!(markdown.contains("Body[^1]."), "{name}: {markdown}");
    }
}

/// Inline `style` attributes keep the element text and record the downgrade.
#[test]
fn html_inline_style_is_a_recorded_loss() {
    let graph = import_fixture_html("inline-style.html");
    assert_eq!(loss(&graph, "import.html.unsupported_style"), SemanticStatus::Partial, "{:?}", loss_codes(&graph));
    let markdown = export_markdown(&graph).expect("markdown export");
    assert!(markdown.contains("Red text"), "{markdown}");
}

/// `<img width>`/`<height>` keep the bitmap and record the dropped display size
/// instead of becoming a sized `<span>` wrapper.
#[test]
fn html_image_size_is_a_recorded_loss() {
    let graph = import_fixture_html("img-size.html");
    assert_eq!(loss(&graph, "import.html.image_dimensions"), SemanticStatus::Partial, "{:?}", loss_codes(&graph));
    let markdown = export_markdown(&graph).expect("markdown export");
    assert!(markdown.contains("![chart](chart.png)"), "{markdown}");
    assert!(!markdown.contains("<span"), "{markdown}");
}

/// Ordered EPUB list items carry strictly increasing numbers, including
/// `<ol start>` and nested lists, so the graph validates.
#[test]
fn epub_ordered_list_items_carry_numbers() {
    let graph = import_epub_bytes("ordered-list.epub", &read_fixture("epub/ordered-list.epub")).expect("epub import");
    assert!(graph.validate().is_valid(), "{:?}", graph.validate().issues);
    let lists: Vec<&Block> =
        collect_blocks(&graph, &|block| matches!(block, Block::List { ordered: true, .. }));
    let main = lists
        .iter()
        .find(|block| {
            matches!(block, Block::List { items, .. }
                if matches!(items.first().map(|item| item.content.as_slice()), Some([Inline::Text { text }]) if text == "one"))
        })
        .copied()
        .unwrap_or_else(|| panic!("ordered list starting with `one`: {:?}", graph.blocks));
    let Block::List { items, .. } = main
    else {
        unreachable!()
    };
    assert_eq!(items.iter().map(|item| item.number).collect::<Vec<_>>(), vec![Some(1), Some(2)]);
    let started = lists
        .iter()
        .find(|block| {
            matches!(block, Block::List { items, .. }
                if matches!(items.first().map(|item| item.content.as_slice()), Some([Inline::Text { text }]) if text == "three"))
        })
        .copied()
        .unwrap_or_else(|| panic!("ordered list starting with `three`: {:?}", graph.blocks));
    let Block::List { items, .. } = started
    else {
        unreachable!()
    };
    assert_eq!(items[0].number, Some(3), "`start=\"3\"` must shift the sequence");
    assert_eq!(items[1].number, Some(4));
    let nested = items[1]
        .children
        .iter()
        .filter_map(|id| graph.block(*id))
        .find(|node| matches!(node.block, Block::List { .. }))
        .expect("nested list block");
    let Block::List { ordered: nested_ordered, items: nested_items } = &nested.block
    else {
        unreachable!()
    };
    assert!(!nested_ordered, "the nested unordered list stays unordered: {:?}", nested.block);
    assert!(nested_items.iter().all(|item| item.number.is_none()), "{nested_items:?}");
}

/// Linked and embedded CSS stay fail-closed with an explicit diagnostic and
/// still keep the chapter text.
#[test]
fn epub_css_stylesheets_stay_fail_closed_with_diagnostics() {
    let linked = import_epub_bytes("css-link.epub", &read_fixture("epub/css-link.epub")).expect("epub import");
    assert_eq!(loss(&linked, "import.epub.css_link_unsupported"), SemanticStatus::Unsupported);
    let markdown = export_markdown(&linked).expect("markdown export");
    assert!(markdown.contains("Styled paragraph."), "{markdown}");

    let embedded = import_epub_bytes("inline-style.epub", &read_fixture("epub/inline-style.epub")).expect("epub import");
    assert_eq!(loss(&embedded, "import.epub.css_inline_block_unsupported"), SemanticStatus::Unsupported);
    let markdown = export_markdown(&embedded).expect("markdown export");
    assert!(markdown.contains("Styled paragraph."), "{markdown}");
}

/// `<li value>` shifts the ordinal while it stays increasing. A backward reset
/// and `<ol reversed>` cannot fit the IR invariant, so both keep the list valid
/// and record an explicit loss.
#[test]
fn epub_list_value_and_reversed_are_explicit() {
    let graph = import_epub_bytes("list-value-reversed.epub", &read_fixture("epub/list-value-reversed.epub"))
        .expect("epub import");
    assert!(graph.validate().is_valid(), "{:?}", graph.validate().issues);

    let shifted = ordered_list_starting_with(&graph, "two");
    assert_eq!(item_numbers(shifted), vec![Some(2), Some(5), Some(6)], "`value=\"5\"` must move the run");

    let reset = ordered_list_starting_with(&graph, "one");
    assert_eq!(item_numbers(reset), vec![Some(1), Some(2)], "a backward `value` keeps the running sequence");
    assert_eq!(loss(&graph, "import.epub.list_item_value_unsupported"), SemanticStatus::Partial);

    let reversed = ordered_list_starting_with(&graph, "a");
    assert_eq!(item_numbers(reversed), vec![Some(1), Some(2)], "reversed lists fall back to ascending ordinals");
    assert_eq!(loss(&graph, "import.epub.list_reversed_unsupported"), SemanticStatus::Partial);
}

/// `<a epub:type="noteref">` and `<aside epub:type="footnote">` lower to the
/// same semantic shapes as the HTML endnotes path, so the Markdown writer can
/// emit real `[^label]` references and definitions.
#[test]
fn epub_structural_footnotes_become_notes_and_definitions() {
    let graph = import_epub_bytes("footnotes.epub", &read_fixture("epub/footnotes.epub")).expect("epub import");
    assert!(graph.validate().is_valid(), "{:?}", graph.validate().issues);
    assert_eq!(note_labels(&graph), vec!["1".to_string(), "2".to_string()], "{:?}", graph.blocks);
    assert_eq!(footnote_definitions(&graph), vec!["[^1]: Note one.", "[^2]: Note two."]);
    assert!(!loss_codes(&graph).iter().any(|code| code == "import.epub.unsupported_xhtml_block"), "{:?}", loss_codes(&graph));
    let markdown = export_markdown(&graph).expect("markdown export");
    assert!(markdown.contains("Body with one note[^1]"), "{markdown}");
    assert!(markdown.contains("another[^2]."), "{markdown}");
    assert!(markdown.contains("[^1]: Note one."), "{markdown}");
    assert!(markdown.contains("[^2]: Note two."), "{markdown}");
}

/// A reference without a definition keeps its note label. Duplicate asides
/// stay explicit definition blocks instead of being merged or dropped.
#[test]
fn epub_footnote_reference_without_definition_stays_a_note() {
    let graph = import_epub_bytes("footnotes-edge.epub", &read_fixture("epub/footnotes-edge.epub")).expect("epub import");
    assert!(graph.validate().is_valid(), "{:?}", graph.validate().issues);
    let notes = note_labels(&graph);
    assert!(notes.iter().any(|label| label == "9"), "{notes:?}");
    assert!(notes.iter().any(|label| label == "3"), "{notes:?}");
    let definitions = footnote_definitions(&graph);
    assert!(!definitions.iter().any(|definition| definition.starts_with("[^9]:")), "{definitions:?}");
    assert_eq!(
        definitions.iter().filter(|definition| definition.starts_with("[^3]:")).count(),
        2,
        "both duplicate definitions stay visible: {definitions:?}"
    );
    let markdown = export_markdown(&graph).expect("markdown export");
    assert!(markdown.contains("[^9]"), "{markdown}");
    assert!(!markdown.contains("[^9]:"), "{markdown}");
    assert!(markdown.contains("[^3]: First copy."), "{markdown}");
    assert!(markdown.contains("[^3]: Second copy."), "{markdown}");
}
