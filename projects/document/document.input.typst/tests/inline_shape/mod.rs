use document_ir::{Block, Inline};

fn inline_shape(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Text { text } => format!("Text({text:?})"),
            Inline::Styled { style, children } => format!("Styled({style},{})", inline_shape(children)),
            Inline::InlineCode { text } => format!("Code({text:?})"),
            Inline::Link { display, .. } => format!("Link({})", inline_shape(display)),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn block_shape(block: &Block) -> String {
    match block {
        Block::Section { level, title, children } => {
            format!("Section level={level} children={} title={}", children.len(), inline_shape(title))
        }
        Block::Paragraph { content } => format!("Paragraph {}", inline_shape(content)),
        Block::List { ordered, items } => {
            format!("List ordered={ordered} items={} first={}", items.len(), inline_shape(&items[0].content))
        }
        Block::Code { language, content } => format!("Code language={language:?} text={content:?}"),
        other => format!("{other:?}"),
    }
}

#[test]
fn probe_structure() {
    let typst = "= Heading\n\nParagraph with *bold* and _em_ and `code`.\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n";
    let markdown = "# Heading\n\nParagraph with **bold** and *em* and `code`.\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n";
    let typst_graph = document_input_typst::import_source("a.typ", typst).expect("typst").graph;
    let markdown_graph = document_formats::import::markdown::import_markdown_bytes("a.md", markdown).expect("markdown");
    for block in &typst_graph.blocks {
        println!("TYPST {}", block_shape(&block.block));
    }
    for block in &markdown_graph.blocks {
        println!("MARKDOWN {}", block_shape(&block.block));
    }
}
