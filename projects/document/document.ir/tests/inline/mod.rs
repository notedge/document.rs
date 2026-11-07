use document_ir::{
    Asset, AssetId, AssetKind, AttributePair, Attributes, Block, BreakKind, DocumentEnvelope, DocumentGraph, DocumentId,
    Inline, LinkId, RelationEndpoint, SemanticStatus, ValidationIssue,
};

fn image_with(source: &str, asset: Option<AssetId>) -> Inline {
    Inline::Image { source: source.to_string(), alt: Vec::new(), title: None, asset }
}

fn issue_count(report: &document_ir::ValidationReport, matches: impl Fn(&ValidationIssue) -> bool) -> usize {
    report.issues.iter().filter(|issue| matches(issue)).count()
}

fn paragraph(graph: &mut DocumentGraph, content: Vec<Inline>) -> document_ir::NodeId {
    graph.push_block(Block::Paragraph { content })
}

#[test]
fn link_keeps_display_separate_from_destination_and_allows_unresolved_target() {
    let mut graph = DocumentGraph::new(DocumentId(1));
    paragraph(
        &mut graph,
        vec![Inline::Link {
            target: RelationEndpoint::Unresolved { label: "[[missing]]".to_string() },
            display: vec![Inline::Text { text: "missing".to_string() }],
            title: Some("tooltip".to_string()),
        }],
    );

    assert!(graph.validate().is_valid());
    let Block::Paragraph { content } = &graph.blocks[0].block
    else {
        panic!("expected paragraph");
    };
    let Inline::Link { target, display, title } = &content[0]
    else {
        panic!("expected link");
    };
    assert_eq!(display, &vec![Inline::Text { text: "missing".to_string() }]);
    assert_eq!(title.as_deref(), Some("tooltip"));
    assert!(matches!(target, RelationEndpoint::Unresolved { .. }));
}

#[test]
fn link_reports_unknown_semantic_target_instead_of_dropping_it() {
    for target in [
        RelationEndpoint::Node(document_ir::NodeId(99)),
        RelationEndpoint::Asset(AssetId(99)),
        RelationEndpoint::Document(DocumentId(2)),
    ] {
        let mut graph = DocumentGraph::new(DocumentId(1));
        paragraph(&mut graph, vec![Inline::Link { target: target.clone(), display: Vec::new(), title: None }]);

        let report = graph.validate();
        assert!(
            report.issues.iter().any(|issue| matches!(issue, ValidationIssue::UnknownInlineLinkTarget { .. })),
            "expected unresolved endpoint for {target:?}"
        );
    }
}

#[test]
fn link_to_existing_asset_and_document_endpoints_is_valid() {
    let mut graph = DocumentGraph::new(DocumentId(1));
    let asset_id = graph.push_asset(Asset {
        id: AssetId(1),
        kind: AssetKind::Attachment,
        content_identity: None,
        source: Some("data.csv".to_string()),
        media_type: None,
        status: SemanticStatus::Resolved,
        bytes: None,
    });
    paragraph(
        &mut graph,
        vec![Inline::Link {
            target: RelationEndpoint::Asset(asset_id),
            display: vec![Inline::Text { text: "data".to_string() }],
            title: None,
        }],
    );

    assert!(graph.validate().is_valid());
}

#[test]
fn image_asset_binding_is_validated_without_faking_a_location() {
    let mut graph = DocumentGraph::new(DocumentId(1));
    let asset_id = graph.push_asset(Asset {
        id: AssetId(1),
        kind: AssetKind::Image,
        content_identity: None,
        source: Some("assets/diagram.png".to_string()),
        media_type: Some("image/png".to_string()),
        status: SemanticStatus::Resolved,
        bytes: None,
    });
    paragraph(
        &mut graph,
        vec![Inline::Image {
            source: "graphics/renamed.png".to_string(),
            alt: vec![Inline::Text { text: "diagram".to_string() }],
            title: None,
            asset: Some(asset_id),
        }],
    );
    assert!(graph.validate().is_valid());

    let mut unbound = DocumentGraph::new(DocumentId(2));
    paragraph(
        &mut unbound,
        vec![Inline::Image { source: "assets/diagram.png".to_string(), alt: Vec::new(), title: None, asset: Some(AssetId(7)) }],
    );
    assert!(unbound.validate().issues.iter().any(|issue| matches!(issue, ValidationIssue::UnknownInlineImageAsset { .. })));
}

#[test]
fn note_and_citation_require_a_non_empty_label() {
    let mut graph = DocumentGraph::new(DocumentId(1));
    paragraph(
        &mut graph,
        vec![Inline::Note { label: "  ".to_string(), display: None }, Inline::Citation { key: String::new(), display: None }],
    );

    let report = graph.validate();
    assert_eq!(report.issues.iter().filter(|issue| matches!(issue, ValidationIssue::EmptyInlineLabel { .. })).count(), 2);
}

#[test]
fn note_and_citation_validate_their_optional_display_content() {
    let mut graph = DocumentGraph::new(DocumentId(1));
    paragraph(
        &mut graph,
        vec![Inline::Note {
            label: "1".to_string(),
            display: Some(vec![Inline::Reference { target: document_ir::NodeId(42), display: "missing".to_string() }]),
        }],
    );

    assert!(graph.validate().issues.iter().any(|issue| matches!(issue, ValidationIssue::UnknownInlineReference { .. })));
}

#[test]
fn attributed_children_are_validated_and_carry_stable_order_attributes() {
    let mut graph = DocumentGraph::new(DocumentId(1));
    paragraph(
        &mut graph,
        vec![Inline::Attributed {
            attributes: Attributes {
                id: Some("intro".to_string()),
                classes: vec!["lead".to_string(), "wide".to_string()],
                pairs: vec![AttributePair { key: "data-role".to_string(), value: "summary".to_string() }],
            },
            children: vec![Inline::Link {
                target: RelationEndpoint::Node(document_ir::NodeId(9)),
                display: Vec::new(),
                title: None,
            }],
        }],
    );

    let report = graph.validate();
    assert!(report.issues.iter().any(|issue| matches!(issue, ValidationIssue::UnknownInlineLinkTarget { .. })));

    let Block::Paragraph { content } = &graph.blocks[0].block
    else {
        panic!("expected paragraph");
    };
    let Inline::Attributed { attributes, .. } = &content[0]
    else {
        panic!("expected attributed inline");
    };
    assert_eq!(attributes.id.as_deref(), Some("intro"));
    assert_eq!(attributes.classes, vec!["lead".to_string(), "wide".to_string()]);
    assert_eq!(attributes.pairs[0].key, "data-role");
    assert!(!attributes.is_empty());
}

#[test]
fn break_kinds_are_distinct_semantics() {
    let mut graph = DocumentGraph::new(DocumentId(1));
    paragraph(
        &mut graph,
        vec![
            Inline::Text { text: "a".to_string() },
            Inline::Break { kind: BreakKind::Soft },
            Inline::Break { kind: BreakKind::Hard },
        ],
    );

    assert!(graph.validate().is_valid());
    let Block::Paragraph { content } = &graph.blocks[0].block
    else {
        panic!("expected paragraph");
    };
    assert_eq!(content[1], Inline::Break { kind: BreakKind::Soft });
    assert_eq!(content[2], Inline::Break { kind: BreakKind::Hard });
}

#[test]
fn inline_semantics_round_trip_through_the_wire_without_losing_identity() {
    let mut graph = DocumentGraph::new(DocumentId(3));
    graph.bump_revision();
    let asset_id = graph.push_asset(Asset {
        id: AssetId(1),
        kind: AssetKind::Image,
        content_identity: None,
        source: Some("assets/diagram.png".to_string()),
        media_type: Some("image/png".to_string()),
        status: SemanticStatus::Resolved,
        bytes: None,
    });
    let link_id = LinkId(1);
    let node = paragraph(
        &mut graph,
        vec![
            Inline::Image {
                source: "assets/diagram.png".to_string(),
                alt: vec![Inline::Text { text: "diagram".to_string() }],
                title: None,
                asset: Some(asset_id),
            },
            Inline::Note { label: "1".to_string(), display: None },
            Inline::Citation { key: "knuth1984".to_string(), display: Some(vec![Inline::Text { text: "Knuth".to_string() }]) },
            Inline::Attributed {
                attributes: Attributes { id: None, classes: vec!["aside".to_string()], pairs: Vec::new() },
                children: vec![Inline::Break { kind: BreakKind::Hard }],
            },
        ],
    );
    graph.push_relation(document_ir::Relation {
        id: link_id,
        kind: document_ir::RelationKind::Embeds,
        source: RelationEndpoint::Node(node),
        target: RelationEndpoint::Asset(asset_id),
    });

    let envelope = DocumentEnvelope::new(graph.clone());
    let encoded = serde_json::to_string(&envelope).expect("serialize envelope");
    let decoded: DocumentEnvelope = serde_json::from_str(&encoded).expect("deserialize envelope");
    decoded.ensure_supported().expect("supported schema");

    assert_eq!(decoded.document, graph);
    assert!(decoded.document.validate().is_valid());

    let Block::Paragraph { content } = &decoded.document.blocks[0].block
    else {
        panic!("expected paragraph");
    };
    let inline_json = serde_json::to_string(content).expect("serialize inline content");
    assert!(!inline_json.contains("\"title\""), "empty optional link/image titles should not be serialized: {inline_json}");
    assert!(inline_json.contains(r#""Note":{"label":"1"}"#), "an omitted note display should not be serialized: {inline_json}");
    assert!(
        inline_json.contains(r#""attributes":{"classes":["aside"]}"#),
        "empty attribute identity and pairs should be omitted: {inline_json}"
    );
}

#[test]
fn document_without_new_inline_variants_still_loads_on_the_current_wire() {
    let legacy = r#"{
        "schema_version": "notedown-ir/v1",
        "document": {
            "id": 1,
            "revision": 0,
            "metadata": { "title": null, "language": null, "authors": [], "tags": [] },
            "blocks": [
                {
                    "id": 1,
                    "block": {
                        "Paragraph": {
                            "content": [
                                { "Text": { "text": "hello " } },
                                { "Styled": { "style": "bold", "children": [{ "Text": { "text": "world" } }] } }
                            ]
                        }
                    }
                }
            ],
            "relations": [],
            "assets": [],
            "sources": [],
            "coverage": { "complete": true, "loss": [] },
            "next_node": 1
        }
    }"#;

    let envelope: DocumentEnvelope = serde_json::from_str(legacy).expect("load legacy document");
    envelope.ensure_supported().expect("supported schema");
    assert!(envelope.document.validate().is_valid());
    assert_eq!(envelope.document.blocks.len(), 1);
}

#[test]
fn inline_semantics_use_their_declared_wire_keys() {
    let mut graph = DocumentGraph::new(DocumentId(4));
    paragraph(
        &mut graph,
        vec![
            Inline::Link {
                target: RelationEndpoint::Unresolved { label: "[[missing]]".to_string() },
                display: vec![Inline::Text { text: "missing".to_string() }],
                title: None,
            },
            image_with("assets/diagram.png", None),
            Inline::Note { label: "1".to_string(), display: None },
            Inline::Citation { key: "knuth1984".to_string(), display: None },
            Inline::Attributed {
                attributes: Attributes { id: Some("aside".to_string()), classes: Vec::new(), pairs: Vec::new() },
                children: vec![Inline::Text { text: "wrapped".to_string() }],
            },
            Inline::Break { kind: BreakKind::Hard },
        ],
    );
    assert!(graph.validate().is_valid());

    let wire = serde_json::to_value(&graph.blocks[0].block).expect("serialize block");
    let content = wire["Paragraph"]["content"].as_array().expect("inline array");
    for (index, expected) in ["Link", "Image", "Note", "Citation", "Attributed", "Break"].iter().enumerate() {
        let variant = content[index].as_object().expect("variant object");
        assert_eq!(variant.len(), 1, "each inline must carry exactly one variant key");
        assert!(variant.contains_key(*expected), "inline {index} must stay under `{expected}`");
    }
    assert!(content[0]["Link"]["display"].is_array());
    assert!(content[0]["Link"]["target"].is_object());
    assert!(content[1]["Image"]["source"].is_string());
    assert!(content[1]["Image"]["alt"].is_array());
    assert!(content[2]["Note"]["label"].is_string());
    assert!(content[3]["Citation"]["key"].is_string());
    assert!(content[4]["Attributed"]["attributes"]["id"].is_string());
    assert!(content[4]["Attributed"]["children"].is_array());
    assert_eq!(content[5]["Break"]["kind"], serde_json::json!("Hard"));

    // Optional payloads stay absent instead of being synthesized on the wire.
    assert!(content[0]["Link"].get("title").is_none());
    assert!(content[1]["Image"].get("title").is_none());
    assert!(content[1]["Image"].get("asset").is_none());
    assert!(content[2]["Note"].get("display").is_none());
    assert!(content[3]["Citation"].get("display").is_none());
}

#[test]
fn image_without_source_or_bound_asset_is_reported() {
    let mut unbound = DocumentGraph::new(DocumentId(5));
    paragraph(&mut unbound, vec![image_with("   ", None)]);
    assert_eq!(issue_count(&unbound.validate(), |issue| matches!(issue, ValidationIssue::EmptyInlineImageSource { .. })), 1);

    let mut bound = DocumentGraph::new(DocumentId(6));
    let asset_id = bound.push_asset(Asset {
        id: AssetId(1),
        kind: AssetKind::Image,
        content_identity: None,
        source: Some("assets/diagram.png".to_string()),
        media_type: Some("image/png".to_string()),
        status: SemanticStatus::Resolved,
        bytes: None,
    });
    paragraph(&mut bound, vec![image_with("", Some(asset_id))]);
    assert!(bound.validate().is_valid(), "an asset binding makes the location optional: {:?}", bound.validate().issues);
}

#[test]
fn attributed_wrapper_requires_at_least_one_attribute() {
    let mut graph = DocumentGraph::new(DocumentId(7));
    paragraph(
        &mut graph,
        vec![Inline::Attributed {
            attributes: Attributes { id: None, classes: Vec::new(), pairs: Vec::new() },
            children: vec![Inline::Text { text: "bare".to_string() }],
        }],
    );
    assert_eq!(issue_count(&graph.validate(), |issue| matches!(issue, ValidationIssue::EmptyInlineAttributes { .. })), 1);
}

#[test]
fn inline_attribute_names_must_not_be_blank() {
    let mut graph = DocumentGraph::new(DocumentId(8));
    paragraph(
        &mut graph,
        vec![Inline::Attributed {
            attributes: Attributes {
                id: Some("  ".to_string()),
                classes: vec![String::new()],
                pairs: vec![AttributePair { key: " ".to_string(), value: "v".to_string() }],
            },
            children: vec![Inline::Text { text: "wrapped".to_string() }],
        }],
    );
    let report = graph.validate();
    let slots = report
        .issues
        .iter()
        .filter_map(|issue| match issue {
            ValidationIssue::EmptyInlineAttributeName { slot, .. } => Some(slot.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(slots.len(), 3, "{slots:?}");
    for expected in ["id", "class", "key"] {
        assert!(slots.contains(&expected), "`{expected}` slot must be reported: {slots:?}");
    }
    assert_eq!(issue_count(&report, |issue| matches!(issue, ValidationIssue::EmptyInlineAttributes { .. })), 0);
}

#[test]
fn styled_inline_requires_a_style_name() {
    let mut graph = DocumentGraph::new(DocumentId(9));
    paragraph(&mut graph, vec![Inline::Styled { style: "  ".to_string(), children: Vec::new() }]);
    assert_eq!(issue_count(&graph.validate(), |issue| matches!(issue, ValidationIssue::EmptyInlineStyle { .. })), 1);
}

#[test]
fn legacy_style_aliases_load_as_canonical_styles() {
    let legacy = r#"{
        "schema_version": "notedown-ir/v1",
        "document": {
            "id": 9,
            "revision": 0,
            "metadata": { "title": null, "language": null, "authors": [], "tags": [] },
            "blocks": [
                {
                    "id": 1,
                    "block": {
                        "Section": {
                            "level": 1,
                            "title": [
                                { "Styled": { "style": "strong", "children": [{ "Text": { "text": "Heading" } }] } }
                            ],
                            "children": [2]
                        }
                    }
                },
                {
                    "id": 2,
                    "block": {
                        "Paragraph": {
                            "content": [
                                { "Styled": { "style": "strong", "children": [
                                    { "Styled": { "style": "emphasis", "children": [{ "Text": { "text": "nested" } }] } }
                                ] } },
                                { "Styled": { "style": "emph", "children": [{ "Text": { "text": "em" } }] } },
                                { "Styled": { "style": "strikethrough", "children": [{ "Text": { "text": "old" } }] } },
                                { "Styled": { "style": "strikeout", "children": [{ "Text": { "text": "older" } }] } },
                                { "Styled": { "style": "sup", "children": [{ "Text": { "text": "2" } }] } },
                                { "Styled": { "style": "sub", "children": [{ "Text": { "text": "i" } }] } },
                                { "Link": {
                                    "target": { "Unresolved": { "label": "[[missing]]" } },
                                    "display": [
                                        { "Styled": { "style": "strong", "children": [{ "Text": { "text": "link" } }] } }
                                    ]
                                } },
                                { "Image": {
                                    "source": "assets/diagram.png",
                                    "alt": [
                                        { "Styled": { "style": "sup", "children": [{ "Text": { "text": "alt" } }] } }
                                    ]
                                } },
                                { "Note": {
                                    "label": "1",
                                    "display": [
                                        { "Styled": { "style": "sub", "children": [{ "Text": { "text": "note" } }] } }
                                    ]
                                } },
                                { "Citation": {
                                    "key": "knuth1984",
                                    "display": [
                                        { "Styled": { "style": "emph", "children": [{ "Text": { "text": "cite" } }] } }
                                    ]
                                } },
                                { "Attributed": {
                                    "attributes": { "id": "aside" },
                                    "children": [
                                        { "Styled": { "style": "strikethrough", "children": [{ "Text": { "text": "wrapped" } }] } }
                                    ]
                                } }
                            ]
                        }
                    }
                },
                {
                    "id": 3,
                    "block": {
                        "List": {
                            "ordered": false,
                            "items": [
                                {
                                    "content": [
                                        { "Styled": { "style": "strong", "children": [{ "Text": { "text": "item" } }] } }
                                    ],
                                    "children": []
                                }
                            ]
                        }
                    }
                }
            ],
            "relations": [],
            "assets": [],
            "sources": [],
            "coverage": { "complete": true, "loss": [] },
            "next_node": 4
        }
    }"#;

    let envelope: DocumentEnvelope = serde_json::from_str(legacy).expect("load legacy aliases");
    envelope.ensure_supported().expect("supported schema");
    assert!(envelope.document.validate().is_valid(), "{:?}", envelope.document.validate().issues);

    let styles = |inlines: &[Inline]| -> Vec<String> {
        inlines
            .iter()
            .filter_map(|inline| match inline {
                Inline::Styled { style, .. } => Some(style.clone()),
                _ => None,
            })
            .collect()
    };

    let Block::Section { title, .. } = &envelope.document.blocks[0].block
    else {
        panic!("expected section");
    };
    assert_eq!(styles(title), ["bold"]);

    let Block::Paragraph { content } = &envelope.document.blocks[1].block
    else {
        panic!("expected paragraph");
    };
    assert_eq!(
        styles(content),
        ["bold", "italic", "strike", "strike", "superscript", "subscript"],
        "top-level aliases must load with canonical spellings"
    );
    let Inline::Styled { children, .. } = &content[0]
    else {
        panic!("expected styled wrapper");
    };
    assert_eq!(styles(children), ["italic"], "nested aliases normalize too");
    let Inline::Link { display, .. } = &content[6]
    else {
        panic!("expected link");
    };
    assert_eq!(styles(display), ["bold"]);
    let Inline::Image { alt, .. } = &content[7]
    else {
        panic!("expected image");
    };
    assert_eq!(styles(alt), ["superscript"]);
    let Inline::Note { display: Some(display), .. } = &content[8]
    else {
        panic!("expected note");
    };
    assert_eq!(styles(display), ["subscript"]);
    let Inline::Citation { display: Some(display), .. } = &content[9]
    else {
        panic!("expected citation");
    };
    assert_eq!(styles(display), ["italic"]);
    let Inline::Attributed { children, .. } = &content[10]
    else {
        panic!("expected attributed wrapper");
    };
    assert_eq!(styles(children), ["strike"]);

    let Block::List { items, .. } = &envelope.document.blocks[2].block
    else {
        panic!("expected list");
    };
    assert_eq!(styles(&items[0].content), ["bold"]);
}
