# document-ir

文档语义 IR：`DocumentGraph`、块 / 行内、关系、资产、provenance 与显式 `SemanticStatus`。不含解析器、排版或渲染。

下游转换与排版都围绕本图展开；门面见 [`document`](../document/readme.md)。

## Example

```rust
use document_ir::{Block, DocumentGraph, DocumentId, Inline};

let mut graph = DocumentGraph::new(DocumentId(1));
graph.push_block(Block::Paragraph {
    content: vec![Inline::Text { text: "你好".into() }],
});
assert!(graph.validate().is_valid());
```
