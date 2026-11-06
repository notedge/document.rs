//! 冒烟断言：注释、转义与行内 markup 不留下冒充正文的 text 节点。

use document_compose::Severity;
use document_input_typst::import_source;
use document_ir::{Block, DocumentGraph, Inline};

fn collect_text(graph: &DocumentGraph, block: &Block, texts: &mut Vec<String>) {
    fn walk(inlines: &[Inline], texts: &mut Vec<String>) {
        for inline in inlines {
            match inline {
                Inline::Text { text } => texts.push(text.clone()),
                Inline::Styled { children, .. } | Inline::Attributed { children, .. } => walk(children, texts),
                Inline::Link { display, .. } => walk(display, texts),
                Inline::Image { alt, .. } => walk(alt, texts),
                Inline::Note { display: Some(display), .. } | Inline::Citation { display: Some(display), .. } => {
                    walk(display, texts);
                }
                _ => {}
            }
        }
    }

    match block {
        Block::Section { title, .. } => walk(title, texts),
        Block::Paragraph { content } => walk(content, texts),
        Block::List { items, .. } => {
            for item in items {
                walk(&item.content, texts);
            }
        }
        Block::Table { head, body, foot, .. } => {
            for row in head.iter().chain(body).chain(foot) {
                for cell in &row.cells {
                    for child in &cell.children {
                        if let Some(node) = graph.block(*child) {
                            collect_text(graph, &node.block, texts);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn collect_styles(block: &Block, styles: &mut Vec<(String, Vec<String>)>) {
    fn texts_in(inlines: &[Inline], texts: &mut Vec<String>) {
        for inline in inlines {
            match inline {
                Inline::Text { text } => texts.push(text.clone()),
                Inline::Styled { children, .. } | Inline::Attributed { children, .. } => texts_in(children, texts),
                _ => {}
            }
        }
    }

    fn walk(inlines: &[Inline], styles: &mut Vec<(String, Vec<String>)>) {
        for inline in inlines {
            match inline {
                Inline::Styled { style, children } => {
                    let mut texts = Vec::new();
                    texts_in(children, &mut texts);
                    styles.push((style.clone(), texts));
                    walk(children, styles);
                }
                Inline::Attributed { children, .. } => walk(children, styles),
                Inline::Link { display, .. } => walk(display, styles),
                _ => {}
            }
        }
    }

    if let Block::Paragraph { content } = block {
        walk(content, styles);
    }
}

#[test]
fn comments_and_inline_markup_do_not_masquerade_as_body_text() {
    let source = "\
= Text nodes

// COMMENT_LINE_MUST_VANISH
Body after the line comment.

Before /* COMMENT_BLOCK_MUST_VANISH */ after.

*bold* and _italic_ stay styled.
";
    let imported = import_source("virtual://text-nodes.typ", source).expect("the source must import");

    let mut texts = Vec::new();
    let mut styles = Vec::new();
    for node in &imported.graph.blocks {
        collect_text(&imported.graph, &node.block, &mut texts);
        collect_styles(&node.block, &mut styles);
    }

    assert!(!texts.iter().any(|text| text.contains("COMMENT_LINE_MUST_VANISH")), "line comments must vanish: {texts:#?}");
    assert!(!texts.iter().any(|text| text.contains("COMMENT_BLOCK_MUST_VANISH")), "block comments must vanish: {texts:#?}");
    assert!(!texts.iter().any(|text| text.contains("//")), "comment markers must not reach the ir: {texts:#?}");
    assert!(!texts.iter().any(|text| text.contains("/*") || text.contains("*/")), "{texts:#?}");
    assert_eq!(texts.iter().filter(|text| text == &"Body after the line comment.").count(), 1, "{texts:#?}");

    let bold = styles.iter().find(|(style, _)| style == "bold").expect("bold markup must lower to a styled inline");
    assert_eq!(bold.1, ["bold"], "bold delimiters must not reach the children: {styles:#?}");
    let italic = styles.iter().find(|(style, _)| style == "italic").expect("italic markup must lower to a styled inline");
    assert_eq!(italic.1, ["italic"], "italic delimiters must not reach the children: {styles:#?}");
    assert!(!texts.iter().any(|text| text.contains("*bold*") || text.contains("_italic_")), "{texts:#?}");

    assert!(
        imported.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error),
        "plain text, comments, and markup must not raise an error: {:#?}",
        imported.diagnostics
    );
}

#[test]
fn escapes_keep_literal_text_and_do_not_shift_following_spans() {
    for (source, expected) in
        [("\\*a\n\n#set page(width: 1cm)\n", "*a"), ("\\*中文后文\n\n#set page(width: 1cm)\n", "*中文后文")]
    {
        let imported = import_source("virtual://escape.typ", source).expect("escapes must import");

        let mut texts = Vec::new();
        for node in &imported.graph.blocks {
            collect_text(&imported.graph, &node.block, &mut texts);
        }
        let joined = texts.join("");
        assert!(joined.contains(expected), "{source:?} must keep {expected:?}: {texts:#?}");
        assert!(!joined.contains('\\'), "no lone backslash may reach the ir: {texts:#?}");
        assert!(
            imported.diagnostics.iter().all(|diagnostic| diagnostic.code != "import.typst.escape_unsupported"),
            "the escape rejection must be gone: {:#?}",
            imported.diagnostics
        );

        let dynamic = imported
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "import.typst.dynamic_content")
            .expect("the dynamic line must be reported");
        let span = dynamic.span.expect("the dynamic line must keep its authored range");
        let authored = &source[span.start as usize..span.end as usize];
        assert!(authored.starts_with("#set page(width: 1cm)"), "{source:?} shifted the following span: {authored:?}");
    }
}
