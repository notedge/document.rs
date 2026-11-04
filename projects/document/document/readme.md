# document

Document 门面：按 feature 打开 `document::*`，再导出对应 `document-*` crate——同一类型，无重复包装。

产品树架构见 [`../readme.md`](../readme.md)；仓库总览见根 [`../../../readme.md`](../../../readme.md)。

## Features

默认 feature **为空**，按需打开：

| Feature                                                  | 模块                  | 底层               |
|----------------------------------------------------------|-----------------------|--------------------|
| `ir`                                                     | `document::ir`        | `document-ir`      |
| `formats`                                                | `document::formats`   | `document-formats` |
| `types` / `diagnostic` / `convert`                       | 同名                  | 对应 crate         |
| `compose` / `style` / `text` / `presentation` / `layout` | 同名                  | 排版链             |
| `renderer` / `renderer-{svg,pdf,html,raster}`            | `document::renderer*` | 渲染后端           |
| `input-{markdown,tex,typst}`                             | `document::input::*`  | 输入适配           |
| `markdown` / `notedown` / `docx` / …                     | 经 `formats`          | 格式语言 feature   |

`formats` 与各格式 feature 会连带打开 `ir`。

## Example

```toml
document = { path = "projects/document/document", default-features = false, features = ["ir", "markdown"] }
```

```rust
use document::formats::import::markdown::import_markdown_bytes;

let graph = import_markdown_bytes("note.md", "# 标题\n\n正文。")?;
assert!(!graph.blocks.is_empty());
```
