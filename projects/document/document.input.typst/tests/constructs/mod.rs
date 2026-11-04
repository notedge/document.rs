//! Construct-level coverage: dynamic Typst constructs keep their authored kind
//! and range, and inline versus display math stay counted apart.

use document_input_typst::{import_source, render_svg};

#[test]
fn dynamic_constructs_keep_their_kind_and_span() {
    let source = "Before\n\n#let answer = 42\n\n#show heading: set text(size: 9pt)\n\n#import \"utils.typ\": *\n\n#table(columns: 2, [A], [B])\n\n#image(\"figure.png\")\n\nAfter\n";
    let imported = import_source("virtual://typst-dynamic.typ", source).expect("dynamic constructs must import as opaque");

    let kinds: Vec<&str> = imported.report.opaque.iter().map(|item| item.code.as_str()).collect();
    for kind in [
        "typst.expression.let",
        "typst.expression.show",
        "typst.expression.import",
        "typst.expression.table",
        "typst.expression.image",
    ] {
        assert!(kinds.contains(&kind), "missing opaque kind {kind}: {kinds:?}");
    }
    assert_eq!(kinds.len(), 5, "one opaque block per construct: {kinds:?}");

    let losses: Vec<_> =
        imported.diagnostics.iter().filter(|diagnostic| diagnostic.code == "import.typst.dynamic_content").collect();
    assert_eq!(losses.len(), 5, "{:#?}", imported.diagnostics);

    let mut covered: Vec<String> = losses
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("every dynamic construct keeps its authored range");
            source[span.start as usize..span.end as usize].to_string()
        })
        .collect();
    covered.sort();
    let mut expected = vec![
        "#import \"utils.typ\": *".to_string(),
        "#image(\"figure.png\")".to_string(),
        "#let answer = 42".to_string(),
        "#show heading: set text(size: 9pt)".to_string(),
        "#table(columns: 2, [A], [B])".to_string(),
    ];
    expected.sort();
    assert_eq!(covered, expected);

    let rendered = render_svg("virtual://typst-dynamic.typ", source).expect("opaque constructs must not stop the page");
    assert!(rendered.svg.contains("Before") && rendered.svg.contains("After"), "surrounding text must survive");
    for leaked in ["answer", "figure.png", "columns"] {
        assert!(!rendered.svg.contains(leaked), "opaque payload must not reach the page as body text: {leaked}");
    }
}

#[test]
fn inline_and_display_math_are_counted_apart() {
    let source = "= Math\n\nInline $a^2$ stays.\n\n$ x + y $\n";
    let imported =
        import_source("virtual://typst-math-counts.typ", source).expect("Typst math must import as preserved source");

    let details: Vec<&str> = imported.report.preserved.iter().map(|item| item.detail.as_str()).collect();
    assert_eq!(details.len(), 2, "one preserved entry per formula: {details:?}");
    assert!(details.iter().any(|detail| detail.starts_with("inline math")), "{details:?}");
    assert!(details.iter().any(|detail| detail.starts_with("display math")), "{details:?}");

    let rendered = render_svg("virtual://typst-math-counts.typ", source).expect("render must succeed");
    assert!(rendered.svg.contains("a^2"), "the inline formula must stay visible");
    assert!(rendered.svg.contains("x + y"), "the display formula must stay visible");
}
