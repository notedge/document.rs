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
fn unclosed_group_is_rejected() {
    let source = "Before\n\n#figure(\n  rect(width: 2cm),\n\nAfter\n";
    let error = import_source("virtual://typst-unclosed.typ", source).expect_err("an unclosed group must not import");

    assert_eq!(error.code(), UNTERMINATED_GROUP);
    assert_eq!(error.diagnostic.severity, Severity::Error);
    let span = error.diagnostic.span.expect("the unclosed construct must be located in the source");
    assert!(source[span.start as usize..].starts_with("#figure("));
}
