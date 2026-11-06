//! Pandoc JSON codec and IR adapter tests.

use document_formats::pandoc::{
    PandocBudget, PandocImportOptions,
    codec::{decode, encode},
    export_pandoc_json, import_pandoc_json_bytes_with,
    model::{
        API_VERSION, Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, Inline, ListNumberDelim,
        ListNumberStyle, MathType, MetaValue, OrderedList, PandocDocument, QuoteType, Row, Table, TableBody, TableFoot,
        TableHead, Target,
    },
};
use document_ir::{Block as IrBlock, DocumentGraph, Inline as IrInline, SemanticStatus};

fn encode_text(document: &PandocDocument) -> String {
    encode(document, &PandocBudget::default()).expect("encode")
}

fn decode_text(text: &str) -> PandocDocument {
    decode(text.as_bytes(), &PandocBudget::default()).expect("decode")
}

fn allow_loss() -> PandocImportOptions {
    PandocImportOptions::allow_loss()
}

fn import(text: &str) -> DocumentGraph {
    import_pandoc_json_bytes_with("pandoc-fixture", text.as_bytes(), &allow_loss()).expect("pandoc import")
}

fn attr(id: &str, classes: &[&str], pairs: &[(&str, &str)]) -> Attr {
    Attr {
        id: id.into(),
        classes: classes.iter().map(|class| (*class).into()).collect(),
        pairs: pairs.iter().map(|(key, value)| ((*key).into(), (*value).into())).collect(),
    }
}

fn inline_set() -> Vec<Inline> {
    let citation = Citation {
        id: "doe2024".into(),
        prefix: vec![Inline::Str("see".into())],
        suffix: vec![Inline::Str("p. 3".into())],
        mode: CitationMode::SuppressAuthor,
        note_num: 4,
        hash: 9,
    };
    vec![
        Inline::Str("text".into()),
        Inline::Emph(vec![Inline::Str("emph".into())]),
        Inline::Underline(vec![Inline::Str("under".into())]),
        Inline::Strong(vec![Inline::Str("strong".into())]),
        Inline::Strikeout(vec![Inline::Str("strike".into())]),
        Inline::Superscript(vec![Inline::Str("sup".into())]),
        Inline::Subscript(vec![Inline::Str("sub".into())]),
        Inline::SmallCaps(vec![Inline::Str("caps".into())]),
        Inline::Quoted { kind: QuoteType::Single, inlines: vec![Inline::Str("quoted".into())] },
        Inline::Cite { citations: vec![citation], inlines: vec![Inline::Str("(Doe 2024)".into())] },
        Inline::Code { attr: attr("code-id", &["rust"], &[("data-x", "1")]), text: "fn main() {}".into() },
        Inline::Space,
        Inline::SoftBreak,
        Inline::LineBreak,
        Inline::Math { display: MathType::Inline, text: "x^2".into() },
        Inline::RawInline { format: "html".into(), text: "<b>x</b>".into() },
        Inline::Link {
            attr: attr("link-id", &["c"], &[("k", "v")]),
            inlines: vec![Inline::Str("link".into())],
            target: Target { url: "https://example.com".into(), title: "title".into() },
        },
        Inline::Image {
            attr: attr("image-id", &["c"], &[]),
            inlines: vec![Inline::Str("alt".into())],
            target: Target { url: "img/x.png".into(), title: "img".into() },
        },
        Inline::Note(vec![Block::Para(vec![Inline::Str("note".into())])]),
        Inline::Span { attr: attr("span-id", &["s"], &[("k", "v1"), ("k", "v2")]), inlines: vec![Inline::Str("span".into())] },
    ]
}

fn all_constructors_document() -> PandocDocument {
    let row = Row {
        attr: Attr::default(),
        cells: vec![
            Cell {
                attr: attr("cell", &["c"], &[("k", "v")]),
                alignment: Alignment::Center,
                row_span: 2,
                col_span: 1,
                blocks: vec![Block::Plain(vec![Inline::Str("cell".into())])],
            },
            Cell {
                attr: Attr::default(),
                alignment: Alignment::Default,
                row_span: 1,
                col_span: 2,
                blocks: vec![Block::Para(vec![Inline::Str("wide".into())])],
            },
        ],
    };
    let table = Table {
        attr: attr("table", &["t"], &[("k", "a"), ("k", "b")]),
        caption: Caption {
            short: Some(vec![Inline::Str("short".into())]),
            blocks: vec![Block::Plain(vec![Inline::Str("caption".into())])],
        },
        colspecs: vec![
            ColSpec { alignment: Alignment::Left, width: 0.25 },
            ColSpec { alignment: Alignment::Default, width: 0.0 },
        ],
        head: TableHead { attr: Attr::default(), rows: vec![row.clone()] },
        bodies: vec![TableBody {
            attr: attr("body", &[], &[]),
            row_head_columns: 1,
            head: vec![row.clone()],
            body: vec![row.clone()],
        }],
        foot: TableFoot { attr: Attr::default(), rows: vec![row] },
    };
    let blocks = vec![
        Block::Plain(vec![Inline::Str("plain".into())]),
        Block::Para(inline_set()),
        Block::LineBlock(vec![vec![Inline::Str("line1".into())], vec![Inline::Str("line2".into())]]),
        Block::CodeBlock(attr("block", &["rust"], &[("k", "v1"), ("k", "v2")]), "code".into()),
        Block::RawBlock("html".into(), "<div>raw</div>".into()),
        Block::BlockQuote(vec![Block::Para(vec![Inline::Str("quote".into())])]),
        Block::OrderedList(OrderedList {
            start: 3,
            style: ListNumberStyle::UpperRoman,
            delimiter: ListNumberDelim::TwoParens,
            items: vec![
                vec![Block::Plain(vec![Inline::Str("one".into())])],
                vec![Block::Para(vec![Inline::Str("two".into())])],
            ],
        }),
        Block::BulletList(vec![vec![Block::Plain(vec![Inline::Str("bullet".into())])]]),
        Block::DefinitionList(vec![(
            vec![Inline::Str("term".into())],
            vec![vec![Block::Plain(vec![Inline::Str("definition".into())])]],
        )]),
        Block::Header { level: 2, attr: attr("head", &["h"], &[("k", "v")]), inlines: vec![Inline::Str("Heading".into())] },
        Block::HorizontalRule,
        Block::Table(table),
        Block::Figure {
            attr: attr("figure", &["f"], &[]),
            caption: Caption { short: None, blocks: vec![Block::Plain(vec![Inline::Str("figure caption".into())])] },
            blocks: vec![Block::Para(vec![Inline::Str("figure body".into())])],
        },
        Block::Div {
            attr: attr("div", &["d"], &[("class", "x")]),
            blocks: vec![Block::Para(vec![Inline::Str("div body".into())])],
        },
    ];
    let mut meta: Vec<(String, MetaValue)> = vec![
        ("title".into(), MetaValue::Inlines(vec![Inline::Str("标题".into())])),
        ("map".into(), MetaValue::Map(vec![("nested".into(), MetaValue::Bool(true))])),
        ("list".into(), MetaValue::List(vec![MetaValue::String("a".into()), MetaValue::Bool(false)])),
        ("bool".into(), MetaValue::Bool(true)),
        ("string".into(), MetaValue::String("value".into())),
        ("blocks".into(), MetaValue::Blocks(vec![Block::Para(vec![Inline::Str("meta block".into())])])),
    ];
    // JSON object key ordering is not part of the contract; `serde_json` sorts
    // keys on decode, so the fixture starts sorted to compare structurally.
    meta.sort_by(|left, right| left.0.cmp(&right.0));
    PandocDocument { api_version: API_VERSION, meta, blocks }
}

#[test]
fn codec_round_trips_every_constructor() {
    let document = all_constructors_document();
    let text = encode_text(&document);
    assert_eq!(decode_text(&text), document);
    // A second encode must be byte stable, not just structurally equal.
    assert_eq!(encode_text(&decode_text(&text)), text);
}

#[test]
fn codec_preserves_duplicate_attribute_pairs_and_unicode() {
    let document = PandocDocument {
        api_version: API_VERSION,
        meta: vec![("title".into(), MetaValue::String("中文 😀".into()))],
        blocks: vec![Block::CodeBlock(attr("", &[], &[("k", "a"), ("k", "b"), ("k", "c")]), "表情 😀".into())],
    };
    let decoded = decode_text(&encode_text(&document));
    assert_eq!(decoded, document);
    let Block::CodeBlock(decoded_attr, _) = &decoded.blocks[0]
    else {
        panic!("expected a code block");
    };
    assert_eq!(decoded_attr.pairs, vec![("k".into(), "a".into()), ("k".into(), "b".into()), ("k".into(), "c".into())]);
}

#[test]
fn codec_rejects_unknown_constructors_with_a_pointer() {
    let text = r#"{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Bogus","c":[]}]}"#;
    let error = decode(text.as_bytes(), &PandocBudget::default()).expect_err("unknown block must fail");
    let message = format!("{error}");
    assert!(message.contains("/blocks/0"), "{message}");
    assert!(message.contains("unknown block constructor `Bogus`"), "{message}");

    let text = r#"{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Bogus"}]}]}"#;
    let error = decode(text.as_bytes(), &PandocBudget::default()).expect_err("unknown inline must fail");
    let message = format!("{error}");
    assert!(message.contains("/blocks/0/c/0"), "{message}");
    assert!(message.contains("unknown inline constructor `Bogus`"), "{message}");
}

#[test]
fn codec_rejects_unsupported_versions_and_top_level_fields() {
    let text = r#"{"pandoc-api-version":[1,22,2],"meta":{},"blocks":[]}"#;
    let error = decode(text.as_bytes(), &PandocBudget::default()).expect_err("version must fail");
    let message = format!("{error}");
    assert!(message.contains("/pandoc-api-version"), "{message}");
    assert!(message.contains("unsupported"), "{message}");
    assert!(message.contains("[1, 23, 1]"), "{message}");

    let text = r#"{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[],"extra":1}"#;
    let error = decode(text.as_bytes(), &PandocBudget::default()).expect_err("extra field must fail");
    assert!(format!("{error}").contains("unknown top-level field `extra`"), "{error}");
}

#[test]
fn codec_rejects_illegal_arity_and_nullary_payloads() {
    let text = r#"{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Header","c":[1,["",[],[]]]}]}"#;
    let error = decode(text.as_bytes(), &PandocBudget::default()).expect_err("short header must fail");
    let message = format!("{error}");
    assert!(message.contains("/blocks/0/c"), "{message}");
    assert!(message.contains("expected arity 3, found 2"), "{message}");

    let text = r#"{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Space","c":[]}]}]}"#;
    let error = decode(text.as_bytes(), &PandocBudget::default()).expect_err("nullary payload must fail");
    assert!(format!("{error}").contains("constructor carries no payload"), "{error}");
}

#[test]
fn codec_enforces_byte_depth_and_node_budgets() {
    let document = all_constructors_document();
    let text = encode_text(&document);
    let bytes_budget = PandocBudget { max_bytes: 16, ..PandocBudget::default() };
    let error = decode(text.as_bytes(), &bytes_budget).expect_err("byte budget must fail");
    assert!(format!("{error}").contains("input is"), "{error}");

    let nested = PandocDocument {
        api_version: API_VERSION,
        meta: Vec::new(),
        blocks: vec![Block::BlockQuote(vec![Block::BlockQuote(vec![Block::Para(vec![Inline::Str("x".into())])])])],
    };
    let depth_budget = PandocBudget { max_depth: 1, ..PandocBudget::default() };
    let text = encode_text(&nested);
    let error = decode(text.as_bytes(), &depth_budget).expect_err("depth budget must fail");
    assert!(format!("{error}").contains("nesting depth"), "{error}");
    // Encode enforces the same limit, so an in-memory document cannot walk
    // past `max_depth` just because it never came through decode.
    let error = encode(&nested, &depth_budget).expect_err("encode depth budget must fail");
    assert!(format!("{error}").contains("nesting depth"), "{error}");

    let node_budget = PandocBudget { max_nodes: 3, ..PandocBudget::default() };
    let error = decode(encode_text(&document).as_bytes(), &node_budget).expect_err("node budget must fail");
    assert!(format!("{error}").contains("node count"), "{error}");
}

#[test]
fn codec_refuses_to_encode_an_unknown_version() {
    let document = PandocDocument { api_version: [1, 22, 2], ..PandocDocument::new() };
    let error = encode(&document, &PandocBudget::default()).expect_err("version must fail");
    assert!(format!("{error}").contains("cannot encode unsupported"), "{error}");
}

fn full_fixture() -> String {
    serde_json::json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": {
            "title": {"t": "MetaInlines", "c": [{"t": "Str", "c": "文档"}]},
            "author": {"t": "MetaList", "c": [{"t": "MetaString", "c": "Alice"}]},
            "lang": {"t": "MetaString", "c": "zh"},
            "tags": {"t": "MetaList", "c": [{"t": "MetaString", "c": "a"}, {"t": "MetaString", "c": "b"}]},
            "custom": {"t": "MetaBool", "c": true}
        },
        "blocks": [
            {"t": "Header", "c": [1, ["intro", ["h"], [["k", "v"]]], [{"t": "Str", "c": "Intro"}]]},
            {"t": "Para", "c": [{"t": "Str", "c": "body"}, {"t": "SoftBreak"}, {"t": "Str", "c": "next"}]},
            {"t": "Header", "c": [3, ["", [], []], [{"t": "Str", "c": "Deep"}]]},
            {"t": "Para", "c": [{"t": "Str", "c": "deep body"}]},
            {"t": "Header", "c": [2, ["", [], []], [{"t": "Str", "c": "Second"}]]},
            {"t": "Para", "c": [{"t": "Str", "c": "second body"}]}
        ]
    })
    .to_string()
}

#[test]
fn import_builds_sections_without_inventing_headings_and_preserves_typed_metadata() {
    let graph = import(&full_fixture());
    assert_eq!(graph.metadata.title.as_deref(), Some("文档"));
    assert_eq!(graph.metadata.language.as_deref(), Some("zh"));
    assert_eq!(graph.metadata.authors, vec!["Alice".to_string()]);
    assert_eq!(graph.metadata.tags, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(graph.metadata.properties.len(), 1);
    assert_eq!(graph.metadata.properties[0].key, "custom");
    assert_eq!(graph.metadata.properties[0].value, document_ir::MetadataValue::Boolean(true));
    assert!(
        !graph.coverage.loss.iter().any(|loss| loss.code == "import.pandoc-json.meta_unmapped"),
        "{:?}",
        graph.coverage.loss
    );

    let sections: Vec<(u8, String, usize)> = graph
        .blocks
        .iter()
        .filter_map(|node| match &node.block {
            IrBlock::Section { level, title, children } => {
                Some((*level, title.iter().map(document_ir::Inline::plain_text).collect::<String>(), children.len()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(sections.len(), 3, "{:?}", graph.blocks);
    // The level-3 heading nests directly under the level-1 section; no level-2
    // heading is synthesized, and its authored level stays 3.
    assert_eq!(sections.iter().map(|section| section.0).collect::<Vec<_>>(), vec![1, 3, 2]);

    let exported = export_pandoc_json(&graph).expect("export");
    let blocks = &exported.document.blocks;
    assert_eq!(blocks.len(), 6, "{:?}", blocks);
    let levels: Vec<i64> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Header { level, .. } => Some(*level),
            _ => None,
        })
        .collect();
    assert_eq!(levels, vec![1, 3, 2]);
}

#[test]
fn document_without_headings_round_trips_without_sections() {
    let text = serde_json::json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": {},
        "blocks": [
            {"t": "Para", "c": [{"t": "Str", "c": "one"}]},
            {"t": "Para", "c": [{"t": "Str", "c": "two"}]}
        ]
    })
    .to_string();
    let graph = import(&text);
    assert!(graph.blocks.iter().all(|node| !matches!(node.block, IrBlock::Section { .. })));
    let exported = export_pandoc_json(&graph).expect("export");
    let reimported = import(&exported.to_json_text().expect("json"));
    assert_eq!(graph.blocks, reimported.blocks);
}

#[test]
fn ir_semantic_round_trip_is_stable_after_export() {
    let graph = import(&full_fixture());
    let exported = export_pandoc_json(&graph).expect("export");
    let reimported = import(&exported.to_json_text().expect("json"));
    assert_eq!(graph.metadata, reimported.metadata);
    assert_eq!(graph.blocks, reimported.blocks);
}

#[test]
fn pandoc_blockquote_round_trips_quoted_text() {
    let document = PandocDocument {
        api_version: API_VERSION,
        meta: Vec::new(),
        blocks: vec![Block::BlockQuote(vec![Block::Para(vec![Inline::Str("Quoted line".into())])])],
    };
    let graph = import(&encode_text(&document));
    let children = graph
        .blocks
        .iter()
        .find_map(|node| match &node.block {
            IrBlock::Quote { children } => Some(children.as_slice()),
            _ => None,
        })
        .expect("quote expected");
    assert!(children.iter().any(|child_id| matches!(
        graph.block(*child_id).map(|node| &node.block),
        Some(IrBlock::Paragraph { content })
            if content.iter().any(|inline| matches!(inline, IrInline::Text { text } if text == "Quoted line"))
    )));

    let exported = export_pandoc_json(&graph).expect("export");
    let json = exported.to_json_text().expect("json");
    let decoded = decode_text(&json);
    assert!(matches!(decoded.blocks.as_slice(), [Block::BlockQuote(_)]), "{:?}", decoded.blocks);
    let reimported = import(&json);
    assert_eq!(graph.blocks, reimported.blocks);
}

#[test]
fn adapter_round_trips_the_full_constructor_fixture() {
    let text = encode_text(&all_constructors_document());
    let graph = import(&text);
    assert!(graph.coverage.loss.len() >= 5, "{:?}", graph.coverage.loss);
    let exported = export_pandoc_json(&graph).expect("export");
    let reimported = import(&exported.to_json_text().expect("json"));
    assert_eq!(graph.blocks, reimported.blocks);
    assert_eq!(graph.assets, reimported.assets);
}

#[test]
fn strict_policy_rejects_containers_that_only_have_an_opaque_projection() {
    let text = serde_json::json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": {},
        "blocks": [
            {"t": "Div", "c": [["id", ["c"], [["k", "v"]]], [{"t": "Para", "c": [{"t": "Str", "c": "inside"}]}]]}
        ]
    })
    .to_string();
    let error = import_pandoc_json_bytes_with("strict", text.as_bytes(), &PandocImportOptions::default())
        .expect_err("strict policy must reject a div");
    assert!(format!("{error}").contains("import.pandoc-json.div_payload"), "{error}");

    let graph = import(&text);
    assert!(matches!(graph.blocks[0].block, IrBlock::Opaque { .. }));
    assert!(graph.coverage.loss.iter().any(|loss| loss.code == "import.pandoc-json.div_payload"));

    let exported = export_pandoc_json(&graph).expect("export");
    assert!(matches!(exported.document.blocks[0], Block::Div { .. }));
    let reimported = import(&exported.to_json_text().expect("json"));
    assert_eq!(graph.blocks, reimported.blocks);
}

#[test]
fn images_are_registered_as_unresolved_assets_not_fetched() {
    let text = serde_json::json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": {},
        "blocks": [
            {"t": "Para", "c": [
                {"t": "Image", "c": [["", [], []], [{"t": "Str", "c": "alt"}], ["https://example.com/a.png", ""]]}
            ]}
        ]
    })
    .to_string();
    let graph = import(&text);
    assert_eq!(graph.assets.len(), 1, "{:?}", graph.assets);
    assert_eq!(graph.assets[0].source.as_deref(), Some("https://example.com/a.png"));
    assert_eq!(graph.assets[0].status, SemanticStatus::Unresolved);
    assert!(graph.assets[0].bytes.is_none());
    assert!(graph.coverage.loss.iter().any(|loss| loss.code == "import.pandoc-json.asset_unresolved"));
    assert_eq!(graph.relations.len(), 1, "{:?}", graph.relations);
}

#[test]
fn table_cell_block_content_is_a_recorded_loss_and_round_trips() {
    let text = serde_json::json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": {},
        "blocks": [
            {"t": "Table", "c": [
                ["", [], []],
                [null, []],
                [["AlignDefault", 0.0]],
                [["", [], []], []],
                [[["", [], []], 0, [], [
                    [["", [], []], [
                        [["", [], []], "AlignDefault", 1, 1, [
                            {"t": "Para", "c": [{"t": "Str", "c": "a"}]},
                            {"t": "Para", "c": [{"t": "Str", "c": "b"}]}
                        ]]
                    ]]
                ]]],
                [["", [], []], []]
            ]}
        ]
    })
    .to_string();
    let graph = import(&text);
    assert!(graph.coverage.loss.iter().any(|loss| loss.code == "import.pandoc-json.table_cell_blocks"));
    let exported = export_pandoc_json(&graph).expect("export");
    let reimported = import(&exported.to_json_text().expect("json"));
    assert_eq!(graph.blocks, reimported.blocks);
}

#[test]
fn export_writes_only_the_three_contract_fields() {
    let graph = import(&full_fixture());
    let exported = export_pandoc_json(&graph).expect("export");
    let text = exported.to_json_text().expect("json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("json value");
    let mut keys: Vec<String> = value.as_object().expect("top-level object").keys().cloned().collect();
    keys.sort();
    // Losses, provenance and Notedown ids stay in the graph report sidecar, so
    // the exchange document carries exactly the Pandoc contract fields.
    assert_eq!(keys, vec!["blocks", "meta", "pandoc-api-version"]);
}

#[test]
fn non_consecutive_ordered_list_numbers_are_reported() {
    let mut graph = DocumentGraph::new(document_ir::IdAllocator::default().document_id());
    graph.push_block(IrBlock::List {
        ordered: true,
        items: vec![
            document_ir::ListItem {
                content: vec![document_ir::Inline::Text { text: "one".into() }],
                number: Some(1),
                ..document_ir::ListItem::default()
            },
            document_ir::ListItem {
                content: vec![document_ir::Inline::Text { text: "three".into() }],
                number: Some(3),
                ..document_ir::ListItem::default()
            },
        ],
    });
    let exported = export_pandoc_json(&graph).expect("export");
    assert!(
        exported.losses.iter().any(|loss| loss.code == "export.pandoc-json.list_number"),
        "{}",
        exported.to_json_text().expect("json")
    );
    let Block::OrderedList(list) = &exported.document.blocks[0]
    else {
        panic!("expected an ordered list");
    };
    assert_eq!(list.start, 1);
    assert_eq!(list.items.len(), 2);
}

#[test]
fn unrelated_custom_inline_style_fails_the_export() {
    let mut graph = DocumentGraph::new(document_ir::IdAllocator::default().document_id());
    graph.push_block(IrBlock::Paragraph {
        content: vec![document_ir::Inline::Styled { style: "sparkle".into(), children: vec![] }],
    });
    let error = export_pandoc_json(&graph).expect_err("unknown style must fail");
    assert!(format!("{error}").contains("inline style `sparkle`"), "{error}");
}

#[test]
fn export_reemits_typed_metadata_entries() {
    let graph = import(&full_fixture());
    let exported = export_pandoc_json(&graph).expect("export");
    let custom = exported.document.meta.iter().find(|(key, _)| key == "custom").expect("custom metadata");
    assert!(matches!(custom.1, MetaValue::Bool(true)));

    let restored = import(&exported.to_json_text().expect("json"));
    assert_eq!(restored.metadata.properties[0].key, "custom");
    assert_eq!(restored.metadata.properties[0].value, document_ir::MetadataValue::Boolean(true));
}

#[test]
fn export_writes_numeric_metadata_as_strings_and_reports_loss() {
    let mut graph = DocumentGraph::new(document_ir::IdAllocator::default().document_id());
    graph.metadata.properties = vec![
        document_ir::MetadataEntry { key: "edition".into(), value: document_ir::MetadataValue::Integer(3) },
        document_ir::MetadataEntry { key: "ratio".into(), value: document_ir::MetadataValue::Float(0.25) },
    ];

    let exported = export_pandoc_json(&graph).expect("export");
    assert!(matches!(&exported.document.meta[0].1, MetaValue::String(value) if value == "3"));
    assert!(matches!(&exported.document.meta[1].1, MetaValue::String(value) if value == "0.25"));
    assert!(exported.losses.iter().any(|loss| loss.code == "export.pandoc-json.metadata_number"), "{:?}", exported.losses);
}
