use document_ir::{
    Asset, AssetKind, Block, DocumentGraph, IdAllocator, Inline, Relation, RelationEndpoint, RelationKind, SemanticStatus,
    SourceKind, SourceRef, ValidationIssue,
};

#[test]
fn unknown_inline_reference() {
    let mut ids = IdAllocator::default();
    let mut graph = DocumentGraph::new(ids.document_id());
    let _allocated = ids.node_id();
    let missing = ids.node_id();
    let _paragraph = graph
        .push_block(Block::Paragraph { content: vec![Inline::Reference { target: missing, display: "missing".to_string() }] });

    let report = graph.validate();
    assert!(!report.is_valid());
    assert!(report.issues.iter().any(|issue| matches!(issue, ValidationIssue::UnknownInlineReference { .. })));
}

#[test]
fn unresolved_relation_target_is_valid() {
    let mut ids = IdAllocator::default();
    let mut graph = DocumentGraph::new(ids.document_id());
    let paragraph = graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: "x".to_string() }] });

    graph.push_relation(Relation {
        id: ids.link_id(),
        kind: RelationKind::References,
        source: RelationEndpoint::Node(paragraph),
        target: RelationEndpoint::Unresolved { label: "[[note]]".to_string() },
    });

    assert!(graph.validate().is_valid());
}

#[test]
fn duplicate_asset_ids() {
    let mut ids = IdAllocator::default();
    let mut graph = DocumentGraph::new(ids.document_id());
    let asset_id = ids.asset_id();

    for _ in 0..2 {
        graph.push_asset(Asset {
            id: asset_id,
            kind: AssetKind::Attachment,
            content_identity: None,
            source: None,
            media_type: None,
            status: SemanticStatus::Resolved,
            bytes: None,
        });
    }

    let report = graph.validate();
    assert!(report.issues.iter().any(|issue| matches!(issue, ValidationIssue::DuplicateAssetId { .. })));
}

#[test]
fn duplicate_node_and_link_ids() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    graph.push_block_with_id(document_ir::NodeId(7), Block::Paragraph { content: Vec::new() });
    graph.push_block_with_id(document_ir::NodeId(7), Block::Paragraph { content: Vec::new() });
    let relation = Relation {
        id: document_ir::LinkId(3),
        kind: RelationKind::References,
        source: RelationEndpoint::Node(document_ir::NodeId(7)),
        target: RelationEndpoint::ExternalUri("https://example.test/".to_string()),
    };
    graph.push_relation(relation.clone());
    graph.push_relation(relation);

    let report = graph.validate();
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        ValidationIssue::DuplicateNodeId { id } if *id == document_ir::NodeId(7)
    )));
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        ValidationIssue::DuplicateLinkId { id } if *id == document_ir::LinkId(3)
    )));
}

#[test]
fn missing_source_and_relation_endpoints() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    graph.attach_source(SourceRef {
        node: document_ir::NodeId(10),
        kind: SourceKind::Manual,
        precision: SemanticStatus::Unresolved,
    });
    for (id, source, target) in [
        (
            1,
            RelationEndpoint::Node(document_ir::NodeId(11)),
            RelationEndpoint::ExternalUri("https://example.test/".to_string()),
        ),
        (2, RelationEndpoint::Asset(document_ir::AssetId(12)), RelationEndpoint::Node(document_ir::NodeId(13))),
        (
            3,
            RelationEndpoint::Document(document_ir::DocumentId(2)),
            RelationEndpoint::Unresolved { label: "unknown".to_string() },
        ),
    ] {
        graph.push_relation(Relation { id: document_ir::LinkId(id), kind: RelationKind::References, source, target });
    }

    let report = graph.validate();
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        ValidationIssue::UnknownNode { id } if *id == document_ir::NodeId(10)
    )));
    assert_eq!(
        report.issues.iter().filter(|issue| matches!(issue, ValidationIssue::UnknownRelationEndpoint { .. })).count(),
        3
    );
}

#[test]
fn missing_section_and_list_children() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    graph.push_block(Block::Section { level: 1, title: Vec::new(), children: vec![document_ir::NodeId(20)] });
    graph.push_block(Block::List {
        ordered: false,
        items: vec![document_ir::ListItem {
            content: Vec::new(),
            children: vec![document_ir::NodeId(21)],
            number: None,
            ..Default::default()
        }],
    });

    let report = graph.validate();
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        ValidationIssue::UnknownSectionChild { section, child }
            if *section == document_ir::NodeId(1) && *child == document_ir::NodeId(20)
    )));
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        ValidationIssue::UnknownListChild { list, child }
            if *list == document_ir::NodeId(2) && *child == document_ir::NodeId(21)
    )));
}

#[test]
fn containment_cycles_rejected_relation_cycles_allowed() {
    let mut cyclic = DocumentGraph::new(document_ir::DocumentId(1));
    cyclic.push_block_with_id(
        document_ir::NodeId(1),
        Block::Section { level: 1, title: Vec::new(), children: vec![document_ir::NodeId(2)] },
    );
    cyclic.push_block_with_id(
        document_ir::NodeId(2),
        Block::List {
            ordered: false,
            items: vec![document_ir::ListItem {
                content: Vec::new(),
                children: vec![document_ir::NodeId(1)],
                number: None,
                ..Default::default()
            }],
        },
    );
    assert!(cyclic.validate().issues.iter().any(|issue| matches!(issue, ValidationIssue::ContainmentCycle { .. })));

    let mut relations = DocumentGraph::new(document_ir::DocumentId(2));
    let first = relations.push_block(Block::Paragraph { content: Vec::new() });
    let second = relations.push_block(Block::Paragraph { content: Vec::new() });
    relations.push_relation(Relation {
        id: document_ir::LinkId(1),
        kind: RelationKind::References,
        source: RelationEndpoint::Node(first),
        target: RelationEndpoint::Node(second),
    });
    relations.push_relation(Relation {
        id: document_ir::LinkId(2),
        kind: RelationKind::References,
        source: RelationEndpoint::Node(second),
        target: RelationEndpoint::Node(first),
    });
    assert!(relations.validate().is_valid());

    relations.relations[0].kind = RelationKind::Contains;
    relations.relations[1].kind = RelationKind::Contains;
    assert!(relations.validate().issues.iter().any(|issue| matches!(issue, ValidationIssue::ContainmentCycle { .. })));
}

#[test]
fn deep_containment_without_stack_overflow() {
    const NODE_COUNT: u64 = 20_000;
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    for id in 1..=NODE_COUNT {
        let children = if id == NODE_COUNT { Vec::new() } else { vec![document_ir::NodeId(id + 1)] };
        graph.push_block_with_id(document_ir::NodeId(id), Block::Section { level: 1, title: Vec::new(), children });
    }
    assert!(graph.validate().is_valid());
}

#[test]
fn ordered_list_requires_increasing_nonzero_numbers() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    graph.push_block(Block::List {
        ordered: true,
        items: vec![
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(1), ..Default::default() },
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(2), ..Default::default() },
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(3), ..Default::default() },
        ],
    });
    assert!(graph.validate().is_valid());
}

#[test]
fn ordered_list_allows_non_continuous_increasing_numbers() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    graph.push_block(Block::List {
        ordered: true,
        items: vec![
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(3), ..Default::default() },
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(7), ..Default::default() },
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(9), ..Default::default() },
        ],
    });
    assert!(graph.validate().is_valid());
}

#[test]
fn ordered_list_missing_number_is_rejected() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    graph.push_block(Block::List {
        ordered: true,
        items: vec![
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(1), ..Default::default() },
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: None, ..Default::default() },
        ],
    });
    let report = graph.validate();
    assert!(report.issues.iter().any(|issue| matches!(issue, ValidationIssue::MissingListItemNumber { item_index: 1, .. })));
}

#[test]
fn ordered_list_zero_or_non_increasing_numbers_are_rejected() {
    // Zero start
    {
        let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
        graph.push_block(Block::List {
            ordered: true,
            items: vec![document_ir::ListItem {
                content: Vec::new(),
                children: Vec::new(),
                number: Some(0),
                ..Default::default()
            }],
        });
        let report = graph.validate();
        assert!(
            report
                .issues
                .iter()
                .any(|issue| matches!(issue, ValidationIssue::OutOfOrderListItemNumber { item_index: 0, found: Some(0), .. }))
        );
    }

    // Duplicate
    {
        let mut graph = DocumentGraph::new(document_ir::DocumentId(2));
        graph.push_block(Block::List {
            ordered: true,
            items: vec![
                document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(1), ..Default::default() },
                document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(1), ..Default::default() },
                document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(2), ..Default::default() },
            ],
        });
        let report = graph.validate();
        assert!(report.issues.iter().any(|issue| matches!(
            issue,
            ValidationIssue::OutOfOrderListItemNumber { item_index: 1, found: Some(1), previous: Some(1), .. }
        )));
    }

    // Reverse order
    {
        let mut graph = DocumentGraph::new(document_ir::DocumentId(3));
        graph.push_block(Block::List {
            ordered: true,
            items: vec![
                document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(5), ..Default::default() },
                document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(2), ..Default::default() },
            ],
        });
        let report = graph.validate();
        assert!(report.issues.iter().any(|issue| matches!(
            issue,
            ValidationIssue::OutOfOrderListItemNumber { item_index: 1, found: Some(2), previous: Some(5), .. }
        )));
    }
}

#[test]
fn unordered_list_must_not_carry_number() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    graph.push_block(Block::List {
        ordered: false,
        items: vec![
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: None, ..Default::default() },
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(7), ..Default::default() },
        ],
    });
    let report = graph.validate();
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, ValidationIssue::UnexpectedListItemNumber { item_index: 1, found: 7, .. }))
    );
}

#[test]
fn nested_ordered_lists_have_independent_numbering() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    let inner = graph.push_block(Block::List {
        ordered: true,
        items: vec![
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(1), ..Default::default() },
            document_ir::ListItem { content: Vec::new(), children: Vec::new(), number: Some(2), ..Default::default() },
        ],
    });
    graph.push_block(Block::List {
        ordered: true,
        items: vec![document_ir::ListItem {
            content: Vec::new(),
            children: vec![inner],
            number: Some(1),
            ..Default::default()
        }],
    });
    assert!(graph.validate().is_valid());
}

fn text_cell(graph: &mut DocumentGraph, text: &str, colspan: u32, rowspan: u32) -> document_ir::TableCell {
    let child = graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: text.into() }] });
    document_ir::TableCell { children: vec![child], colspan, rowspan }
}

fn empty_cell(colspan: u32, rowspan: u32) -> document_ir::TableCell {
    document_ir::TableCell { children: Vec::new(), colspan, rowspan }
}

fn table(head: Vec<document_ir::TableRow>, body: Vec<document_ir::TableRow>, foot: Vec<document_ir::TableRow>) -> Block {
    Block::Table { caption: None, head, body, foot }
}

fn simple_table(graph: &mut DocumentGraph, is_header: bool) -> Block {
    let row = document_ir::TableRow { cells: vec![text_cell(graph, "A", 1, 1), text_cell(graph, "B", 1, 1)] };
    if is_header { table(vec![row], Vec::new(), Vec::new()) } else { table(Vec::new(), vec![row], Vec::new()) }
}

#[test]
fn simple_table_passes_validation() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    let head_table = simple_table(&mut graph, true);
    graph.push_block(head_table);
    let body_table = simple_table(&mut graph, false);
    graph.push_block(body_table);
    assert!(graph.validate().is_valid());
}

#[test]
fn zero_colspan_is_rejected() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    graph.push_block(Block::Table {
        caption: None,
        head: Vec::new(),
        body: vec![document_ir::TableRow { cells: vec![empty_cell(0, 1)] }],
        foot: Vec::new(),
    });
    let report = graph.validate();
    assert!(
        report.issues.iter().any(|issue| matches!(issue, ValidationIssue::ZeroColspan { row_index: 0, cell_index: 0, .. }))
    );
}

#[test]
fn zero_rowspan_is_rejected() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    graph.push_block(Block::Table {
        caption: None,
        head: Vec::new(),
        body: vec![document_ir::TableRow { cells: vec![empty_cell(1, 0)] }],
        foot: Vec::new(),
    });
    let report = graph.validate();
    assert!(
        report.issues.iter().any(|issue| matches!(issue, ValidationIssue::ZeroRowspan { row_index: 0, cell_index: 0, .. }))
    );
}

#[test]
fn colspan_out_of_bounds_is_rejected() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    // table has 2 columns (first row), one cell declares colspan=10
    graph.push_block(Block::Table {
        caption: None,
        head: Vec::new(),
        body: vec![
            document_ir::TableRow { cells: vec![empty_cell(1, 1), empty_cell(1, 1)] },
            document_ir::TableRow { cells: vec![empty_cell(10, 1)] },
        ],
        foot: Vec::new(),
    });
    let report = graph.validate();
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        ValidationIssue::ColspanOutOfBounds { row_index: 1, cell_index: 0, found: 10, max_columns: 2, .. }
    )));
}

#[test]
fn colspan_equal_to_max_columns_is_ok() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    graph.push_block(Block::Table {
        caption: None,
        head: Vec::new(),
        body: vec![
            document_ir::TableRow { cells: vec![empty_cell(1, 1), empty_cell(1, 1)] },
            document_ir::TableRow { cells: vec![empty_cell(2, 1)] },
        ],
        foot: Vec::new(),
    });
    assert!(graph.validate().is_valid());
}

#[test]
fn rowspan_out_of_bounds_is_rejected() {
    let mut graph = DocumentGraph::new(document_ir::DocumentId(1));
    // 2 rows total, row 0 cell declares rowspan=5
    graph.push_block(Block::Table {
        caption: None,
        head: Vec::new(),
        body: vec![
            document_ir::TableRow { cells: vec![empty_cell(1, 5)] },
            document_ir::TableRow { cells: vec![empty_cell(1, 1)] },
        ],
        foot: Vec::new(),
    });
    let report = graph.validate();
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        ValidationIssue::RowspanOutOfBounds { row_index: 0, cell_index: 0, found: 5, total_rows: 2, .. }
    )));
}

#[test]
fn table_with_caption_and_spans_roundtrips_via_serde() {
    use document_ir::{DocumentEnvelope, DocumentGraph, IdAllocator};

    let mut ids = IdAllocator::default();
    let doc_id = ids.document_id();
    let mut graph = DocumentGraph::new(doc_id);

    let head = vec![
        document_ir::TableRow { cells: vec![text_cell(&mut graph, "模型", 1, 2), text_cell(&mut graph, "准确率", 2, 1)] },
        document_ir::TableRow { cells: vec![text_cell(&mut graph, "Top-1", 1, 1), text_cell(&mut graph, "Top-5", 1, 1)] },
    ];
    let body = vec![document_ir::TableRow {
        cells: vec![
            text_cell(&mut graph, "ResNet", 1, 1),
            text_cell(&mut graph, "77%", 1, 1),
            text_cell(&mut graph, "93%", 1, 1),
        ],
    }];
    let table_id =
        graph.push_block(Block::Table { caption: Some("表 1：总体准确率".into()), head, body, foot: Vec::new() });

    assert!(graph.validate().is_valid());

    let envelope = DocumentEnvelope::new(graph.clone());
    let json = serde_json::to_string(&envelope).expect("serialize");
    let restored: DocumentEnvelope = serde_json::from_str(&json).expect("deserialize");
    restored.ensure_supported().expect("schema");
    let restored = restored.document;

    let Block::Table { caption, head, body, foot } = &restored.block(table_id).expect("table node").block
    else {
        panic!("expected table");
    };
    assert_eq!(caption.as_deref(), Some("表 1：总体准确率"));
    assert_eq!(head.len(), 2);
    assert_eq!(body.len(), 1);
    assert!(foot.is_empty());
    assert_eq!(head[0].cells[0].colspan, 1);
    assert_eq!(head[0].cells[0].rowspan, 2);
    assert_eq!(head[0].cells[1].colspan, 2);
    assert_eq!(head[0].cells[1].rowspan, 1);
    assert_eq!(body[0].cells.len(), 3);

    let Block::Paragraph { content } = &restored.block(head[0].cells[0].children[0]).expect("header child block").block
    else {
        panic!("expected paragraph child");
    };
    assert!(matches!(&content[0], Inline::Text { text } if text == "模型"));
    assert_eq!(restored, graph); // full equality including ids
}

#[test]
fn legacy_table_payload_loads_as_bands_and_paragraph_children() {
    use document_ir::{DocumentEnvelope, DocumentGraph};

    let envelope = DocumentEnvelope::new(DocumentGraph::new(document_ir::DocumentId(1)));
    let mut value = serde_json::to_value(&envelope).expect("serialize");
    value["document"]["blocks"] = serde_json::json!([{
        "id": 1,
        "block": {"Table": {
            "caption": "Legacy caption",
            "rows": [
                {"is_header": true, "cells": [{"content": [{"Text": {"text": "H"}}], "colspan": 1, "rowspan": 1}]},
                {"is_header": false, "cells": [{"content": [{"Text": {"text": "B"}}], "colspan": 1, "rowspan": 1}]}
            ]
        }}
    }]);

    let restored: DocumentEnvelope = serde_json::from_value(value).expect("legacy table load");
    restored.ensure_supported().expect("schema");
    let restored = restored.document;

    let (caption, head, body, foot) = restored
        .blocks
        .iter()
        .find_map(|node| match &node.block {
            Block::Table { caption, head, body, foot } => Some((caption, head, body, foot)),
            _ => None,
        })
        .expect("table");
    assert_eq!(caption.as_deref(), Some("Legacy caption"));
    assert_eq!(head.len(), 1);
    assert_eq!(body.len(), 1);
    assert!(foot.is_empty());
    let Block::Paragraph { content } = &restored.block(head[0].cells[0].children[0]).expect("header child block").block
    else {
        panic!("expected paragraph child");
    };
    assert!(matches!(&content[0], Inline::Text { text } if text == "H"));
    let Block::Paragraph { content } = &restored.block(body[0].cells[0].children[0]).expect("body child block").block
    else {
        panic!("expected paragraph child");
    };
    assert!(matches!(&content[0], Inline::Text { text } if text == "B"));
    assert!(restored.validate().is_valid());
}
