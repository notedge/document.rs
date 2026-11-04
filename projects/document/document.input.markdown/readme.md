# `document.input.markdown`

Markdown input adapter. It selects the `document.formats` Markdown import
profile, returns the resulting `document.ir::DocumentGraph`, and normalizes the
import loss into VuTeX's shared conversion report and diagnostics.

The crate owns no Markdown parser and no document schema of its own. Parsing and
semantic lowering stay in `document.formats`, so a second syntax model cannot
drift from the one the other input adapters share.

`import_source` returns the graph with a validated conversion report.
`import_file` reads one Markdown file. `render_svg` runs the same import through
layout, display presentation, and SVG output. Diagram fences stay owned by the
host: register their lowered scenes before rendering.

YAML front matter is metadata, not document content. The importer keeps it as an
unsupported opaque block with its real source span, so `import_source` fails
closed with `vutex.input.markdown.opaque_block` instead of rendering the YAML as
horizontal rules and body text.

A source above `MAX_SOURCE_BYTES` (8 MiB) is rejected with
`vutex.input.markdown.source_too_large` before parsing. An empty source imports
as a documented lossy document instead of a failure.

## Acceptance map

`20-Markdown输入适配合同` 的每条验收都由一个能真跑的用例固定。

| 合同验收 | 用例 |
|---|---|
| 1 最小子集经 `render_svg` 产出标题、Mermaid 场景与公式轮廓 | `tests/markdown_pipeline/mod.rs` · `markdown_reaches_the_rendered_svg` |
| 2 定义列表等降级只产生 `Warning`，文档仍可渲染 | `tests/markdown_pipeline/mod.rs` · `lossy_markdown_is_reported_with_a_structured_code` |
| 3 `Block::Opaque` 进入 `report.opaque` 与 `Info` 诊断 | `tests/markdown_pipeline/mod.rs` · `opaque_blocks_are_reported_instead_of_dropped` |
| 4 `Unsupported` loss 或 opaque 块在导入阶段返回 `Err` | `src/lib.rs` · `unsupported_loss_fails_closed`、`unsupported_opaque_block_fails_closed` |
| 5 表格、引用、嵌套列表、任务列表进图并进 SVG，带对齐标记只告警 | `tests/markdown_pipeline/mod.rs` · `table_quote_nested_list_and_task_list_reach_the_rendered_svg`、`aligned_table_only_warns_and_still_renders` |
| 6 front matter 返回 `Err` 且 `span` 覆盖 `---` 到 `---` | `tests/fail_closed/mod.rs` · `front_matter_fails_closed_with_span` |

## 构造覆盖

合同验收之外，下列构造的闭环由用例固定。

| 构造 | 用例 |
|---|---|
| 有序列表 | `tests/markdown_pipeline/mod.rs` · `ordered_lists_reach_the_rendered_svg` |
| 链接与图片 | `tests/markdown_pipeline/mod.rs` · `links_and_images_reach_the_rendered_svg` |
| 代码围栏与行内代码 | `tests/markdown_pipeline/mod.rs` · `fenced_code_and_inline_code_reach_the_rendered_svg` |
| 转义标记保留字面文本 | `tests/markdown_pipeline/mod.rs` · `escaped_markup_reaches_the_rendered_svg_as_literal_text` |
| 多级引用 | `tests/markdown_pipeline/mod.rs` · `nested_quotes_reach_the_rendered_svg` |
| HTML 块登记而不进页面 | `tests/markdown_pipeline/mod.rs` · `html_blocks_are_reported_instead_of_reaching_the_rendered_svg` |
| autolink 与行内原始 HTML、脚注正文与引用 | `tests/markdown_pipeline/mod.rs` · `autolinks_and_raw_html_do_not_disappear_without_a_report`、`footnote_bodies_and_references_do_not_disappear_without_a_report` |
