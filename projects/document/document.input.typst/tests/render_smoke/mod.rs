//! Smoke coverage for the Typst profile: one minimal render, one balanced
//! multi-line dynamic group, and the rejection path for an unclosed group.

use document_compose::Severity;
use document_input_typst::{UNTERMINATED_GROUP, import_source, render_svg};

#[test]
fn minimal_source_reaches_the_rendered_svg() {
    let source = "= Typst to SVG\n\nHello *world*.\n\n- one\n- two\n";
    let imported = import_source("virtual://typst-smoke.typ", source).expect("minimal Typst must import");
    assert!(imported.report.unsupported.is_empty(), "{:?}", imported.report.unsupported);
    assert!(imported.report.opaque.is_empty(), "{:?}", imported.report.opaque);
    assert!(imported.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error));

    let rendered = render_svg("virtual://typst-smoke.typ", source).expect("render must succeed");
    assert!(rendered.pages >= 1, "{}", rendered.pages);
    assert!(rendered.items >= 1, "{}", rendered.items);
    assert!(rendered.svg.starts_with("<svg"), "{}", &rendered.svg[..rendered.svg.len().min(80)]);
    assert!(rendered.svg.contains("Typst to SVG"), "heading text must reach the svg");
    assert!(rendered.svg.contains("world"), "paragraph text must reach the svg");
}

#[test]
fn balanced_multiline_group_stays_one_opaque_construct() {
    let source = "Before\n\n#figure(\n  rect(width: 2cm),\n  caption: [A box],\n)\n\nAfter\n";
    let imported = import_source("virtual://typst-figure.typ", source).expect("a balanced group must import");

    assert_eq!(imported.report.opaque.len(), 1, "{:#?}", imported.report);
    assert!(
        imported.report.unsupported.iter().any(|item| item.code == "import.typst.dynamic_content"),
        "{:#?}",
        imported.report.unsupported
    );

    let group_diagnostics: Vec<_> = imported
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.code == "import.typst.dynamic_content" || diagnostic.code.starts_with("typst.expression")
        })
        .collect();
    assert_eq!(group_diagnostics.len(), 1, "one construct must produce one diagnostic: {:#?}", imported.diagnostics);
    let span = group_diagnostics[0].span.expect("the recorded group range must be reused");
    assert_eq!(&source[span.start as usize..span.end as usize], "#figure(\n  rect(width: 2cm),\n  caption: [A box],\n)");

    let rendered = render_svg("virtual://typst-figure.typ", source).expect("render must succeed");
    assert!(rendered.svg.contains("Before") && rendered.svg.contains("After"), "surrounding text must survive");
}

#[test]
fn single_line_dynamic_construct_keeps_its_exact_span() {
    let source = "Before\n\n#set page(width: 10cm)\n\nAfter\n";
    let imported = import_source("virtual://typst-set-page.typ", source).expect("a closed single-line group must import");

    assert_eq!(imported.report.opaque.len(), 1, "{:#?}", imported.report);
    assert!(
        imported.report.unsupported.iter().any(|item| item.code == "import.typst.dynamic_content"),
        "{:#?}",
        imported.report.unsupported
    );
    let diagnostic = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "import.typst.dynamic_content")
        .expect("the dynamic construct must be diagnosed");
    let span = diagnostic.span.expect("the single-line construct must keep its authored range");
    assert_eq!(&source[span.start as usize..span.end as usize], "#set page(width: 10cm)");

    let rendered = render_svg("virtual://typst-set-page.typ", source).expect("render must succeed");
    assert!(rendered.svg.contains("Before") && rendered.svg.contains("After"), "surrounding text must survive");
    assert!(!rendered.svg.contains("page(width"), "the opaque construct must not reach the svg as body text");
}

#[test]
fn typst_math_is_preserved_and_layout_reports_the_language_gap() {
    let source = "= Math\n\nThe identity $a^2 + b^2 = c^2$ holds.\n";
    let imported = import_source("virtual://typst-math.typ", source).expect("Typst math must import as preserved source");

    assert!(
        imported.report.preserved.iter().any(|item| item.code == "import.typst.math.source_preserved"),
        "Typst math must be reported as preserved source: {:#?}",
        imported.report.preserved
    );
    assert_eq!(imported.report.lowered.handled, 2, "the heading and the paragraph stay lowered: {:#?}", imported.report);

    let rendered = render_svg("virtual://typst-math.typ", source).expect("render must succeed");
    assert!(
        rendered.diagnostics.iter().any(|diagnostic| diagnostic.code == "vutex.layout.math.unsupported_language"),
        "layout must report the Typst language gap: {:#?}",
        rendered.diagnostics
    );
    assert!(rendered.svg.contains("a^2 + b^2 = c^2"), "the math source must stay visible");
    assert!(rendered.svg.contains("holds"), "the paragraph must reach the svg");
}

#[test]
fn typst_and_markdown_agree_on_the_shared_semantic_subset() {
    let markdown = "# Shared\n\nA paragraph.\n\n- one\n- two\n";
    let typst = "= Shared\n\nA paragraph.\n\n- one\n- two\n";
    let markdown_blocks = document_input_markdown::import_source("virtual://shared.md", markdown)
        .expect("the Markdown side must import")
        .graph
        .blocks;
    let typst_blocks = import_source("virtual://shared.typ", typst).expect("the Typst side must import").graph.blocks;

    let markdown_kinds: Vec<&str> = markdown_blocks.iter().map(|node| block_kind(&node.block)).collect();
    let typst_kinds: Vec<&str> = typst_blocks.iter().map(|node| block_kind(&node.block)).collect();
    assert_eq!(markdown_kinds, typst_kinds, "the shared subset must lower to the same block kinds");
    assert_eq!(markdown_kinds, ["section", "paragraph", "list"]);
}

fn block_kind(block: &document_ir::Block) -> &'static str {
    match block {
        document_ir::Block::Section { .. } => "section",
        document_ir::Block::Paragraph { .. } => "paragraph",
        document_ir::Block::List { .. } => "list",
        document_ir::Block::Table { .. } => "table",
        document_ir::Block::Quote { .. } => "quote",
        document_ir::Block::Code { .. } => "code",
        document_ir::Block::Math { .. } => "math",
        document_ir::Block::Diagram { .. } => "diagram",
        document_ir::Block::Chart { .. } => "chart",
        document_ir::Block::Opaque { .. } => "opaque",
        document_ir::Block::RawHtml { .. } => "raw-html",
    }
}

#[test]
fn unclosed_group_is_rejected() {
    let source = "Before\n\n#figure(\n  rect(width: 2cm),\n\nAfter\n";
    let error = import_source("virtual://typst-unclosed.typ", source).expect_err("an unclosed group must not import");

    assert_eq!(error.code(), UNTERMINATED_GROUP);
    assert_eq!(error.diagnostic.severity, Severity::Error);
    let span = error.diagnostic.span.expect("the unclosed construct must be located in the source");
    assert!(source[span.start as usize..].starts_with("#figure("));
}
