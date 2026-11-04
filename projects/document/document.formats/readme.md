# document-formats

围绕 `document-ir` 的多格式 import / export。解析与降级在此完成；编排入口在 `document-convert`，产品门面是
[`document`](../document/readme.md)。

默认 feature 含 `markdown`。按语言打开 `notedown` / `docx` / `html` / `pdf` / `epub` / `tex` / `typst` 等。

## Example

```rust
use document_formats::import::markdown::import_markdown_bytes;
use document_formats::export::markdown::export_markdown;

let graph = import_markdown_bytes("note.md", "# 标题\n\n正文。")?;
let md = export_markdown(&graph)?;
assert!(md.contains("标题"));
```
