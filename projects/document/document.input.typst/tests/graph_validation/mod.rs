//! 合同 30 验收：Typst profile 的 graph 校验入口与 `vutex.input.typst.invalid_graph`。

use document_compose::Severity;
use document_input_typst::{INVALID_GRAPH, import_source, validate_graph};
use document_ir::{Block, DocumentGraph, DocumentId, Inline, LinkId, NodeId, Relation, RelationEndpoint, RelationKind};

/// 合法样张覆盖标题、列表、样式、数学与动态构造，validate 必须全过。
const VALID_SAMPLES: [(&str, &str); 3] = [
    ("headings-and-lists.typ", "= Intro\n\nBody with *bold* and _italic_.\n\n- one\n- two\n\n1. first\n2. second\n"),
    ("dynamic.typ", "= Report\n\n#set page(width: 10cm)\n\nBody text.\n"),
    ("math.typ", "= Formula\n\nThe value $x + y$ matters.\n\n$ a = b $\n"),
];

#[test]
fn valid_typst_samples_never_produce_invalid_graph() {
    for (label, source) in VALID_SAMPLES {
        let document = import_source(label, source).unwrap_or_else(|error| panic!("{label}: import failed: {error}"));
        assert!(!document.graph.blocks.is_empty(), "{label}: sample must lower to content");
        validate_graph(&document.graph).unwrap_or_else(|error| panic!("{label}: valid graph rejected: {error}"));
        assert!(!document.diagnostics.iter().any(|diagnostic| diagnostic.code == INVALID_GRAPH), "{label}");
    }
}

#[test]
fn duplicate_node_id_is_rejected_with_the_profile_code() {
    let mut graph = DocumentGraph::new(DocumentId(41));
    graph.push_block_with_id(NodeId(7), Block::Paragraph { content: vec![Inline::Text { text: "a".into() }] });
    graph.push_block_with_id(NodeId(7), Block::Paragraph { content: Vec::new() });

    let error = validate_graph(&graph).expect_err("a repeated node id breaks the model");
    assert_eq!(error.code(), INVALID_GRAPH);
    assert_eq!(error.diagnostic.severity, Severity::Error);
    assert_eq!(error.diagnostic.span, None);
    assert!(error.message().contains("lowered graph breaks document-model invariants"), "{}", error.message());
    assert!(error.message().contains("DuplicateNodeId"), "issues must be carried verbatim: {}", error.message());
}

#[test]
fn dangling_relation_endpoint_is_rejected_with_the_profile_code() {
    let mut graph = DocumentGraph::new(DocumentId(42));
    let source = graph.push_block(Block::Paragraph { content: Vec::new() });
    graph.push_relation(Relation {
        id: LinkId(1),
        kind: RelationKind::References,
        source: RelationEndpoint::Node(source),
        target: RelationEndpoint::Node(NodeId(999)),
    });

    let error = validate_graph(&graph).expect_err("a dangling relation endpoint breaks the model");
    assert_eq!(error.code(), INVALID_GRAPH);
    assert_eq!(error.diagnostic.severity, Severity::Error);
    assert!(error.message().contains("UnknownRelationEndpoint"), "issues must be carried verbatim: {}", error.message());
    assert_eq!(error.to_string(), format!("{INVALID_GRAPH}: {}", error.message()));
}
