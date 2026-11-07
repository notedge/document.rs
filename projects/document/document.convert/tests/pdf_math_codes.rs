//! Math content codes: the PDF report keeps the losses that change what the
//! page shows, and leaves environment warnings to the logger.
//!
//! The layout pass serves only the PDF writer, so this test needs its own
//! binary: the math install is process-wide and runs once, and the first half
//! proves the code that fires before any host math pack is installed.

#[path = "common/mod.rs"]
mod common;

use document_convert::write_document_pdf;
use document_fonts::{MathFontPack, install_math_fonts};
use document_formats::lower_tex_formula_source;
use document_ir::{Block, DocumentGraph, DocumentId, Inline, MathDisplay};

use common::pdf_body_faces;

fn formula_graph(source: &str) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(99));
    graph.push_block(Block::Math { formula: lower_tex_formula_source(source, MathDisplay::Display) });
    graph
}

fn loss_codes(graph: &DocumentGraph) -> Vec<String> {
    graph.coverage.loss.iter().map(|loss| loss.code.clone()).collect()
}

fn assert_reported(graph: &DocumentGraph, expected: &str) {
    let codes = loss_codes(graph);
    assert!(codes.iter().any(|code| code == expected), "expected {expected} in the report: {codes:?}");
}

#[test]
fn pdf_report_keeps_every_math_content_loss_and_drops_environment_warnings() {
    let faces = pdf_body_faces();

    // A paragraph carries no math, so nothing is reported for it.
    let mut plain = DocumentGraph::new(DocumentId(99));
    plain.push_block(Block::Paragraph { content: vec![Inline::Text { text: "Hello".into() }] });
    write_document_pdf(&mut plain, &faces).expect("PDF write");
    assert!(loss_codes(&plain).is_empty(), "{:?}", loss_codes(&plain));

    // No math pack is installed in this binary, so the engine cannot typeset a
    // formula and the page keeps the authored source instead.
    let mut graph = formula_graph("x^2");
    write_document_pdf(&mut graph, &faces).expect("PDF write keeps the source fallback");
    assert_reported(&graph, "vutex.layout.math.not_typeset");

    let pack = MathFontPack::load_from(&common::host_font_dir()).expect("host math pack");
    install_math_fonts(&pack);

    // A clean formula now typesets, so the report stays clean. The metric and
    // approximation warnings the pass emits along the way are logger-only.
    let mut graph = formula_graph("x^2");
    write_document_pdf(&mut graph, &faces).expect("PDF write");
    assert!(loss_codes(&graph).is_empty(), "{:?}", loss_codes(&graph));

    // An unknown command keeps a placeholder glyph, so the expression and the
    // missing outline are both content losses the report must carry.
    let mut graph = formula_graph("\\unknowncmd{1}");
    write_document_pdf(&mut graph, &faces).expect("PDF write");
    assert_reported(&graph, "vutex.math.unsupported_expression");
    assert_reported(&graph, "vutex.layout.math.outline_missing");
}
