use document_formats::import::epub::{ast_lowering::blocks_from_html_document, html_extensions::parse_html_bytes};
use document_ir::{Block, DocumentGraph, DocumentId, Inline};

#[test]
fn body_selector_scopes_block_lowering() {
    const XHTML: &str = r#"<html><head><title>Skip</title></head><body><p>Body</p></body></html>"#;
    let document = parse_html_bytes(XHTML.as_bytes()).expect("parse");
    let mut graph = DocumentGraph::new(DocumentId(0));
    let (blocks, _) = blocks_from_html_document(&document, &mut graph, None);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0], Block::Paragraph { .. }));
}

#[test]
fn unnamed_spans_splice_their_children_instead_of_inventing_a_style() {
    const XHTML: &str = r#"<html><body><p>a<span>b<span>c</span></span>d <span class="lead">marked</span></p></body></html>"#;
    let document = parse_html_bytes(XHTML.as_bytes()).expect("parse");
    let mut graph = DocumentGraph::new(DocumentId(0));
    let (blocks, _) = blocks_from_html_document(&document, &mut graph, None);
    let Block::Paragraph { content } = &blocks[0]
    else {
        panic!("paragraph expected: {blocks:?}")
    };

    assert!(
        !content.iter().any(|inline| matches!(inline, Inline::Styled { style, .. } if style == "span")),
        "an unnamed span must not become a `span` style: {content:?}"
    );
    assert_eq!(content.iter().map(Inline::plain_text).collect::<String>(), "abcd marked");

    let Inline::Attributed { attributes, children } = content.last().expect("attributed span")
    else {
        panic!("an authored span attribute must keep an attributed wrapper: {content:?}")
    };
    assert_eq!(attributes.classes, ["lead"]);
    assert_eq!(children, &[Inline::Text { text: "marked".into() }]);
}
