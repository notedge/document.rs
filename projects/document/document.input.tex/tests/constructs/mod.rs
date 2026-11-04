//! Construct-level coverage for the TeX profile: nested environments survive
//! as children, `\section*` stays a reported downgrade, `\href` becomes a real
//! external link, multi-key citations keep every key, and a footnote body is
//! carried on the reference instead of flowing into the paragraph text.

use document_compose::Severity;
use document_input_tex::{import_source, render_svg};
use document_ir::{Block, Inline};

#[test]
fn nested_lists_keep_the_inner_level_as_a_child() {
    let source = "\\begin{document}\n\\begin{itemize}\n\\item Outer\n\\begin{itemize}\n\\item Inner\n\\end{itemize}\n\\end{itemize}\n\\end{document}\n";
    let imported = import_source("virtual://tex-nested-list.tex", source).expect("nested lists must import");
    let outer = imported
        .graph
        .blocks
        .iter()
        .find_map(|node| match &node.block {
            Block::List { items, .. } if !items.is_empty() && !items[0].children.is_empty() => Some(items),
            _ => None,
        })
        .expect("the outer list must keep its nested block as a child");

    let child = imported.graph.block(outer[0].children[0]).expect("the child block must be addressable");
    assert!(matches!(child.block, Block::List { .. }), "the nested item must stay a list: {child:?}");

    let rendered = render_svg("virtual://tex-nested-list.tex", source).expect("nested lists must render");
    assert!(rendered.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error), "{:?}", rendered.diagnostics);
    assert!(rendered.svg.contains("Outer") && rendered.svg.contains("Inner"), "both levels must stay visible");
}

#[test]
fn starred_section_stays_unnumbered_and_is_reported() {
    let source = "\\section*{Starred}\nBody.\n";
    let imported = import_source("virtual://tex-starred.tex", source).expect("a starred section must import");

    assert!(imported.report.lossy.iter().any(|item| item.code == "import.tex.starred_command"), "{:#?}", imported.report.lossy);
    assert!(
        imported.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error),
        "{:#?}",
        imported.diagnostics
    );

    let rendered = render_svg("virtual://tex-starred.tex", source).expect("render must succeed");
    assert!(rendered.svg.contains("Starred"), "the unnumbered title must stay visible");
    assert!(!rendered.svg.contains("1 Starred"), "a starred section must not grow a counter");
}

#[test]
fn href_becomes_an_external_link() {
    let source = "See \\href{https://example.org/docs}{Docs}.\n";
    let imported = import_source("virtual://tex-href.tex", source).expect("href must import");

    assert!(
        imported.report.unsupported.iter().all(|item| item.code != "import.tex.unsupported_command"),
        "{:#?}",
        imported.report.unsupported
    );

    let rendered = render_svg("virtual://tex-href.tex", source).expect("href must render");
    assert!(rendered.svg.contains("Docs"), "the anchor text must stay visible");
    assert!(rendered.svg.contains("<a href=\"https://example.org/docs\">"), "the destination must reach the svg anchor");
    assert!(!rendered.svg.contains("\\href"), "no authored command may leak into the page: {}", rendered.svg);
}

#[test]
fn citations_keep_every_key_and_ignore_the_optional_note() {
    let source = "Keys \\cite{a,b} and \\cite[p.~3]{c}.\n";
    let imported = import_source("virtual://tex-cite-keys.tex", source).expect("multi-key citations must import");
    assert!(imported.report.lossy.iter().any(|item| item.code == "import.tex.citation_marker"), "{:#?}", imported.report.lossy);

    assert!(
        imported.report.lossy.iter().any(|item| item.code == "import.tex.citation_note"),
        "the dropped optional note must be reported: {:#?}",
        imported.report.lossy
    );

    let rendered = render_svg("virtual://tex-cite-keys.tex", source).expect("citations must render");
    for marker in ["[@a]", "[@b]", "[@c]"] {
        assert!(rendered.svg.contains(marker), "missing citation marker {marker}");
    }
    assert!(!rendered.svg.contains("p.~3"), "the optional note has no IR carrier");
}

#[test]
fn footnote_body_is_carried_on_the_reference_not_the_flow() {
    let source = "Body\\footnote{Footnote body} text.\n";
    let imported = import_source("virtual://tex-footnote.tex", source).expect("a footnote must import");
    let paragraph = imported
        .graph
        .blocks
        .iter()
        .find_map(|node| match &node.block {
            Block::Paragraph { content } => Some(content),
            _ => None,
        })
        .expect("the body paragraph must exist");

    assert!(
        paragraph.iter().any(|inline| matches!(
            inline,
            Inline::Note { display: Some(display), .. }
                if display.iter().map(Inline::plain_text).collect::<String>().contains("Footnote body")
        )),
        "the footnote body must ride on the note inline: {paragraph:#?}"
    );
    assert!(
        !paragraph.iter().any(|inline| matches!(inline, Inline::Text { text } if text.contains("Footnote body"))),
        "the footnote body must not be flattened into the paragraph text: {paragraph:#?}"
    );
    assert!(imported.report.lossy.iter().any(|item| item.code == "import.tex.footnote_body"), "{:#?}", imported.report.lossy);
}
