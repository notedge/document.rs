use document_compose::Severity;
use document_input_markdown::{codes, import_source, render_svg};
use document_ir::{Block, Inline, RelationEndpoint};
use document_layout::Diagrams;

const SOURCE: &str = "\
# Markdown to SVG

Inline math $a^2 + b^2$ stays in the paragraph.

$$
E = mc^2
$$

```mermaid
graph TD
  A[Start] --> B[End]
```
";

fn diagram_scenes(imported: &document_input_markdown::MarkdownDocument) -> Diagrams {
    let diagram = imported
        .graph
        .blocks
        .iter()
        .find(|node| matches!(&node.block, Block::Diagram { language, .. } if language == "mermaid"))
        .expect("the mermaid fence must lower to a diagram block");
    let Block::Diagram { source, .. } = &diagram.block
    else {
        unreachable!("the node was selected as a diagram block");
    };
    let lowered = plotter_input_mermaid::mermaid_to_scene(source).expect("mermaid fence must compile");
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);

    let mut diagrams = Diagrams::new();
    diagrams.insert(diagram.id, lowered.scene, "Mermaid diagram");
    diagrams
}

#[test]
fn markdown_reaches_the_rendered_svg() {
    let imported = import_source("virtual://markdown-to-svg.md", SOURCE).expect("fixture must import");
    assert!(imported.report.unsupported.is_empty(), "{:?}", imported.report.unsupported);
    assert!(imported.report.lossy.is_empty(), "{:?}", imported.report.lossy);
    assert!(imported.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error), "{:?}", imported.diagnostics);

    let diagrams = diagram_scenes(&imported);
    let rendered = render_svg("virtual://markdown-to-svg.md", SOURCE, &diagrams).expect("render must succeed");

    assert!(rendered.pages >= 1, "{}", rendered.pages);
    assert!(rendered.items >= 1, "{}", rendered.items);
    assert!(rendered.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error), "{:?}", rendered.diagnostics);
    assert!(rendered.svg.starts_with("<svg"), "{}", &rendered.svg[..rendered.svg.len().min(80)]);
    assert!(rendered.svg.contains("Markdown to SVG"), "heading text must reach the svg");
    assert!(rendered.svg.contains("data-diagram=\"Mermaid diagram\""), "diagram must reach the svg");
    assert!(rendered.svg.contains("<path"), "math glyph outlines must reach the svg");
}

#[test]
fn lossy_markdown_is_reported_with_a_structured_code() {
    let source = "Term\n: definition\n";
    let imported = import_source("virtual://definition-list.md", source).expect("lossy constructs are not fatal");

    let item = imported
        .report
        .lossy
        .iter()
        .find(|item| item.code == "import.markdown.definition_list")
        .expect("definition list downgrade must be reported");
    assert!(!item.detail.is_empty());

    let diagnostic = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "import.markdown.definition_list")
        .expect("definition list downgrade must produce a diagnostic");
    assert_eq!(diagnostic.severity, Severity::Warning);
}

#[test]
fn table_quote_nested_list_and_task_list_reach_the_rendered_svg() {
    let source = "\
## Structures

> quoted text

| Name | Value |
| --- | --- |
| A | 1 |

- outer
  - inner

- [x] done
- [ ] todo
";
    let imported = import_source("virtual://structures.md", source).expect("the structures must import");

    assert!(matches!(imported.graph.blocks[0].block, Block::Section { .. }), "{:#?}", imported.graph.blocks);
    assert!(
        imported.graph.blocks.iter().any(|node| matches!(node.block, Block::Quote { .. })),
        "a blockquote must lower to a quote block: {:#?}",
        imported.graph.blocks
    );
    let table = imported
        .graph
        .blocks
        .iter()
        .find_map(|node| match &node.block {
            Block::Table { rows, .. } => Some(rows),
            _ => None,
        })
        .expect("a GFM table must lower to a table block");
    assert_eq!(table.len(), 2, "one header row and one body row: {table:#?}");
    assert!(table[0].is_header);

    let lists: Vec<_> = imported
        .graph
        .blocks
        .iter()
        .filter_map(|node| match &node.block {
            Block::List { items, .. } => Some(items),
            _ => None,
        })
        .collect();
    let has_item = |needle: &str| {
        lists
            .iter()
            .any(|items| items.iter().any(|item| item.content.iter().any(|inline| inline.plain_text().contains(needle))))
    };
    assert!(has_item("outer"), "the outer item must lower to a list block: {lists:#?}");
    assert!(has_item("inner"), "the nested item must not be dropped: {lists:#?}");

    let tasks = imported
        .graph
        .blocks
        .iter()
        .find_map(|node| match &node.block {
            Block::List { items, .. } if items.iter().any(|item| item.is_task) => Some(items),
            _ => None,
        })
        .expect("a task list must keep its checkbox state");
    assert_eq!(tasks.len(), 2, "{tasks:#?}");
    assert!(tasks[0].is_checked == Some(true));
    assert!(tasks[1].is_checked == Some(false));

    let rendered = render_svg("virtual://structures.md", source, &Diagrams::new()).expect("the structures must render");
    assert!(rendered.svg.contains("quoted text"), "quote text must reach the svg");
    assert!(rendered.svg.contains("Name") && rendered.svg.contains("Value"), "table cells must reach the svg");
    assert!(rendered.svg.contains("outer") && rendered.svg.contains("inner"), "nested list text must reach the svg");
    assert!(rendered.svg.contains("done") && rendered.svg.contains("todo"), "task list text must reach the svg");
}

#[test]
fn aligned_table_only_warns_and_still_renders() {
    let source = "| Left | Right |\n|:--|--:|\n| a | b |\n";
    let imported = import_source("virtual://aligned-table.md", source).expect("alignment is a downgrade, not a rejection");

    let lossy = imported
        .report
        .lossy
        .iter()
        .find(|item| item.code == "import.markdown.table_alignment")
        .unwrap_or_else(|| panic!("alignment markers must be reported: {:#?}", imported.report));
    assert!(!lossy.detail.is_empty());
    assert!(
        imported.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "import.markdown.table_alignment" && diagnostic.severity == Severity::Warning
        }),
        "{:#?}",
        imported.diagnostics
    );
    assert!(
        imported.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error),
        "{:#?}",
        imported.diagnostics
    );

    let rendered = render_svg("virtual://aligned-table.md", source, &Diagrams::new()).expect("the table must render");
    assert!(rendered.svg.contains("Left") && rendered.svg.contains("Right"), "cells must reach the svg");
}

#[test]
fn construct_losses_keep_their_own_spans() {
    let source = "Term\n: definition\n\n| Left | Right |\n|:--|--:|\n| a | b |\n";
    let imported = import_source("virtual://loss-spans.md", source).expect("both constructs are downgrades, not failures");

    let definition = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "import.markdown.definition_list")
        .expect("the definition list loss must be diagnosed");
    let definition_span = definition.span.expect("a construct-level loss must keep its range");
    let slice = &source[definition_span.start as usize..definition_span.end as usize];
    assert!(slice.contains("definition"), "{definition:?} slice={slice:?}");

    let alignment = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "import.markdown.table_alignment")
        .expect("the alignment loss must be diagnosed");
    let alignment_span = alignment.span.expect("a construct-level loss must keep its range");
    assert!(source[alignment_span.start as usize..alignment_span.end as usize].contains(":--"), "{alignment:?}");
}

#[test]
fn opaque_blocks_are_reported_instead_of_dropped() {
    let source = "Above\n\n---\n\nBelow\n";
    let imported = import_source("virtual://thematic-break.md", source).expect("thematic breaks are not fatal");

    assert!(imported.report.opaque.iter().any(|item| item.detail.contains("thematic_break")), "{:?}", imported.report.opaque);
    let diagnostic = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == codes::OPAQUE_BLOCK)
        .expect("opaque block must produce a diagnostic");
    assert_eq!(diagnostic.severity, Severity::Info);
}

#[test]
fn autolinks_and_raw_html_do_not_disappear_without_a_report() {
    let source = "See <https://example.com> now.\n\n<div>authored block</div>\n";
    let imported = import_source("virtual://raw-html.md", source).expect("html is a downgrade, not a rejection");

    let rendered = render_svg("virtual://raw-html.md", source, &Diagrams::new()).expect("render must succeed");
    assert!(rendered.svg.contains("https://example.com"), "the autolink must survive into svg: {}", rendered.svg);
    assert!(!rendered.svg.contains("authored block"), "the semantic renderer does not lay out raw html");

    let has_link = imported.graph.blocks.iter().any(|node| match &node.block {
        Block::Paragraph { content } => content.iter().any(|inline| {
            matches!(
                inline,
                Inline::Link { target: RelationEndpoint::ExternalUri(url), .. } if url == "https://example.com"
            )
        }),
        _ => false,
    });
    assert!(has_link, "the autolink must lower to a semantic link");

    let raw = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "import.markdown.raw_html_unrendered")
        .expect("raw html must be reported");
    let span = raw.span.expect("the raw html loss must keep its authored range");
    let authored = &source[span.start as usize..span.end as usize];
    assert!(authored.contains("<div>authored block</div>"), "the loss must point at the html block: {authored:?}");
    assert_eq!(
        imported.diagnostics.iter().filter(|diagnostic| diagnostic.code == "import.markdown.raw_html_unrendered").count(),
        1,
        "the recovered autolink must not be reported as raw html"
    );
}

#[test]
fn markdown_constructs_reach_the_rendered_svg() {
    let source = "\
1. first
2. second

[link](https://example.com/page) and ![alt text](https://example.com/a.png) and `inline code` and \\*literal\\*.

> outer
> > inner

```rust
fn demo() {}
```

<div>authored block</div>
";
    let imported = import_source("virtual://constructs.md", source).expect("markdown constructs are not fatal");
    assert!(imported.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error));

    let rendered = render_svg("virtual://constructs.md", source, &Diagrams::new()).expect("render must succeed");
    for expected in ["first", "second", "link", "inline code", "*literal*", "outer", "inner", "fn demo() {}"] {
        assert!(rendered.svg.contains(expected), "svg must contain {expected:?}: {}", rendered.svg);
    }
    assert!(rendered.svg.contains("https://example.com/a.png"), "the image source must reach svg: {}", rendered.svg);
    assert!(rendered.svg.contains("data-alt=\"alt text\""), "the image alt must reach svg: {}", rendered.svg);
}
