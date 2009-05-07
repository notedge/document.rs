# document

Document 门面：按 feature 打开 `document::*` 模块，再导出对应 `document-*` crate——同一类型，无重复包装。

产品树架构（包图、主链、资产边界）见上级 [`../readme.md`](../readme.md)。

## Features

默认 feature **为空**。按需打开：

| Feature | 模块 | 底层 crate |
|---|---|---|
| `ir` | `document::ir` | `document.ir` |
| `formats` | `document::formats` | `document.formats`（无格式 feature） |
| `types` | `document::types` | `document.types` |
| `diagnostic` | `document::diagnostic` | `document.diagnostic` |
| `convert` | `document::convert` | `document.convert` |
| `compose` / `style` / `text` / `presentation` / `layout` | 同名模块 | 对应 `document.*` |
| `renderer` / `renderer-{svg,pdf,html,raster}` | `document::renderer*` | 对应 renderer crate |
| `input-{markdown,tex,typst}` | `document::input::*` | 对应 input crate |
| `markdown` / `notedown` / `docx` / … | 经 `formats` | `document.formats` + 语言 feature |

`formats` 与各格式 feature 会连带打开 `ir`。

## Example

```toml
[dependencies]
document = { version = "0.1", default-features = false, features = ["ir", "markdown", "docx"] }
```

```rust
use document::ir::DocumentGraph;
use document::formats::import::markdown;
```
