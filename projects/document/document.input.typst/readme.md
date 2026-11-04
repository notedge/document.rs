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
out of child text. An escape such as `\*` keeps its literal character and does
not shift the spans that follow it.

A source above `MAX_SOURCE_BYTES` (8 MiB) is rejected with
`vutex.input.typst.source_too_large` before parsing. An empty source imports as
a documented lossy document instead of a failure.

## Acceptance map

`19-Typst输入适配合同` 的每条验收都由一个能真跑的用例固定。

| 合同验收 | 用例 |
|---|---|
| 1 最小子集经 `render_svg` 产出对应文本 | `tests/render_smoke/mod.rs` · `minimal_source_reaches_the_rendered_svg` |
| 2 单行动态构造不进 SVG，报告与诊断带精确 `span` | `tests/render_smoke/mod.rs` · `single_line_dynamic_construct_keeps_its_exact_span` |
| 3 Typst 数学进 `report.preserved`，layout 诊断出现 | `tests/render_smoke/mod.rs` · `typst_math_is_preserved_and_layout_reports_the_language_gap` |
| 4 与 Markdown 语义子集的 block 结构可比 | `tests/render_smoke/mod.rs` · `typst_and_markdown_agree_on_the_shared_semantic_subset` |
| 5 平衡多行组一条诊断且前后正文存活，未闭合组拒绝 | `tests/render_smoke/mod.rs` · `balanced_multiline_group_stays_one_opaque_construct`、`unclosed_group_is_rejected` |
| 6 注释与行内 markup 不留下冒充正文的 text 节点，转义保留字面字符且不移动后续 span | `tests/text_nodes/mod.rs` · `comments_and_inline_markup_do_not_masquerade_as_body_text`、`escapes_keep_literal_text_and_do_not_shift_following_spans` |
