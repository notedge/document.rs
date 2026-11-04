# `document.input.tex`

TeX/LaTeX input profile. The syntax frontend stays outside VuTeX and the
document model owns the lowering, so this crate only selects the profile and
reports which constructs reached document semantics.

A construct the profile cannot map is recorded as an explicit conversion
status with a diagnostic code and a source span, never silently dropped or
replaced by an approximation.

`render_svg` runs the same import and then the shared layout, presentation, and
SVG render chain, so a host gets a rendered document and the layout diagnostics
from one call.

```rust,ignore
let rendered = document_input_tex::render_svg(
    "main.tex",
    "\\begin{document}\n\\section{Title}\nHello.\n\\end{document}\n",
)?;
println!("{}", rendered.svg);
```

## Acceptance map

`21-TeX输入适配合同` 的每条验收都由一个能真跑的用例固定。

| 合同验收 | 用例 |
|---|---|
| 1 最小文档经 `render_svg` 产出 SVG，`unsupported` 为空且无 `Error` | `tests/render_smoke/mod.rs` · `minimal_source_reaches_the_rendered_svg` |
| 2 未知环境导入成功，`unsupported` 有码，诊断 `span` 覆盖唯一出现 | `tests/fail_closed/mod.rs` · `unknown_environment_is_reported_with_its_source_span` |
| 3 外来数学语言进 `unsupported` 与 `Error`，导入不中断 | `tests/fail_closed/mod.rs` · `foreign_math_language_fails_closed`、`tex_math_without_a_language_tag_stays_accepted` |
| 4 空文档 `parsed.considered == 0`，`empty_document` 进 `lossy`，无诊断 | `tests/fail_closed/mod.rs` · `empty_source_keeps_document_level_losses_unpositioned` |
| 5 解析失败返回 `Err`，码 `parse_failed`，不产出图 | `tests/fail_closed/mod.rs` · `rejected_source_returns_a_structured_error` |
| 6 输出阶段拒绝时诊断码原样透传 | `tests/fail_closed/mod.rs` · `renderer_rejection_keeps_its_own_diagnostic_code` |

验收 6 的边界：SVG 渲染器只拒绝画布尺寸非正的图形，而 TeX 入口不接收宿主
注册的 scene，所以该拒绝在 `render_svg` 的正常路径上不可达。用例固定的是
适配层唯一的契约——渲染器 `Diagnostic` 不被改名地转成 `TexInputError`。
