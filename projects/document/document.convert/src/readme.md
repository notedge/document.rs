# document-convert

转换编排：经 `document-formats` 读写，持有 `DocumentGraph`，产出报告与发布物。适配器作者用本 crate；CLI 用户走下游产品。

门面见 [`document`](../document/readme.md)（feature `convert`）。

## Example

```rust
use document_convert::convert_bytes;

let output = convert_bytes("markdown", "html", "note.md", b"# Hi\n".to_vec(), None)
    .expect("markdown → html route is wired");
assert!(output.html.as_deref().unwrap_or("").contains("Hi"));
```

## 路由可用性

`supports_route(from, to)` 由 `document_formats::capabilities()` 的同一张能力表推导：

| 能力状态 | 是否可路由 |
| --- | --- |
| `Ready` | 是 |
| `Partial` | 是。已接线可用，具体文档的缺口以 loss marker / 诊断暴露 |
| `Planned` | 否。尚未接线 |
| `Unavailable` | 否。显式关闭，包括已接线但不可交付的导出端 |
| 表中缺项 | 否。对应 cargo feature 未打开 |

一条路由可用，当且仅当：`from` 是已接线的 reader（`READERS`），`to` 是已接线的 writer（`WRITERS`），
`from` 的 `Import` 状态可路由，且 `to` 的 `Export` 状态可路由。`READERS` / `WRITERS` 只描述接线，
不再单独决定可用性。

`docx` 与 `pdf` 导出为 `Partial`，所以 `→ docx`、`→ pdf` 路由保持可用。保守的 DOCX 导出端遇到
`Math` / `RawHtml` / opaque 块时以 `unsupported` 诊断拒绝，而不是静默丢弃。PDF writer 依次执行
`layout → presentation → renderer-pdf`，保留布局、为 CJK 文本嵌入字形并排版数学。

`Partial` 表示「部分可用」，不削弱路由：路由仍为 `true`，每份文档通过 `coverage.loss` /
`losses[].status` 与诊断信封报告自己的缺口。只有当目标对一般场景完全无法交付时才撤回路由。

`convert_bytes` 以同一谓词作为入口门，路由不可用时返回 `unsupported_format`。

## 下游消费

`panduck-napi` 由上述两个函数直接暴露 `supports_conversion` 与 `supported_routes`，因此这里的 `false`
在 NAPI 表面就是 `false`，无需额外映射。
