# `document.input.typst`

Typst input adapter for VuTeX. It selects the Notedown Typst lowering profile, projects
the resulting `document.ir` document graph into a `document.compose` conversion report, and
orchestrates the layout, presentation, and SVG render chain.

It owns no Typst parser and no document IR. Parsing stays in `oak-typst` and lowering
stays in `document.formats`, so Typst cannot grow a second style system or a second
typesetting kernel.

```rust,ignore
let rendered = document_input_typst::render_svg("main.typ", "= Title\n\nHello *world*.\n")?;
println!("{}", rendered.svg);
```

Comments are dropped without leaving text nodes, and inline markup delimiters stay
out of child text. Escape sequences (`\*` and friends) are rejected with
`vutex.input.typst.escape_unsupported` until the upstream span defect is fixed,
see the limitation section below.

A source above `MAX_SOURCE_BYTES` (8 MiB) is rejected with
`vutex.input.typst.source_too_large` before parsing. An empty source imports as
a documented lossy document instead of a failure.

## 已知限制：`oak-typst` 转义与多字节 span

当前 `oak-typst` 修订无法把转义序列交给适配层而不损坏正文。最小复现：

```typst
= 标题

转义 \* 中文后文
```

- 现在看到：`\*` 被拆成一条只含 `\` 的 text 条目，`*` 丢失，紧随其后的
  text 条目按字节偏移整体后移。源里有中文时，偏移落到字符中间或跨到下一字，
  `SourceSpan` 与源字节索引不一致。
- 期望行为：`\*` 产出字面 `*`，其后 text 的起点正好等于转义对之后的字节
  位置，多字节字符的 span 按 UTF-8 字节对齐源。
- 接管位置：`document.formats/src/import/typst.rs` 的 `escape_text` 读源回填
  被保护字符并记录 `import.typst.escape_unsupported`，
  `document.input.typst/src/adapter.rs` 的 `import_source` 再把这条 loss 投影
  成 `Err`。`tests/text_nodes/mod.rs` · `escape_sequences_are_rejected_before_any_fake_text`
  固定了当前拒绝行为。
- 上游修好后：删除 `escape_text`、适配层的投影、`ESCAPE_UNSUPPORTED` 常量与
  对应用例，不保留兼容开关。

修上游属于 `oak-typst` 的线，不在本仓补 parser 轮子。

## Acceptance map

`19-Typst输入适配合同` 的每条验收都由一个能真跑的用例固定。

| 合同验收 | 用例 |
|---|---|
| 1 最小子集经 `render_svg` 产出对应文本 | `tests/render_smoke/mod.rs` · `minimal_source_reaches_the_rendered_svg` |
| 2 单行动态构造不进 SVG，报告与诊断带精确 `span` | `tests/render_smoke/mod.rs` · `single_line_dynamic_construct_keeps_its_exact_span` |
| 3 Typst 数学进 `report.preserved`，layout 诊断出现 | `tests/render_smoke/mod.rs` · `typst_math_is_preserved_and_layout_reports_the_language_gap` |
| 4 与 Markdown 语义子集的 block 结构可比 | `tests/render_smoke/mod.rs` · `typst_and_markdown_agree_on_the_shared_semantic_subset` |
| 5 平衡多行组一条诊断且前后正文存活，未闭合组拒绝 | `tests/render_smoke/mod.rs` · `balanced_multiline_group_stays_one_opaque_construct`、`unclosed_group_is_rejected` |
| 6 注释与行内 markup 不留下冒充正文的 text 节点，转义序列被拒绝 | `tests/text_nodes/mod.rs` · `comments_and_inline_markup_do_not_masquerade_as_body_text`、`escape_sequences_are_rejected_before_any_fake_text` |
