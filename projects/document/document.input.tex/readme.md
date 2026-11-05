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

A source above `MAX_SOURCE_BYTES` (8 MiB) is rejected with
`vutex.input.tex.source_too_large` before the frontend starts reading. An empty
source imports as a documented lossy document instead of a failure.

本仓不携带字体字节。数学用例在运行期从宿主资产包读取
`LatinModernMath-Regular.otf`（默认取同级 `VuTeX/projects/vutex.rs/fonts/`，
可用 `VUTEX_FONT_DIR` 覆盖），再经 `install_math_fonts` 注入。资产包缺失时用例
直接失败并给出路径，不跳过。

字体是环境相关的产品级差异：宿主安装字体后公式排成轮廓，SVG 不再出现公式源码；
未安装时 `vutex.layout.math.not_typeset` 为 `Error`，公式源码作为文本保留。适配器
自身既不内置也不安装字体，两种环境都在合同可见范围内。

## Acceptance map

`21-TeX输入适配合同` 的每条验收都由一个能真跑的用例固定。

| 合同验收 | 用例 |
|---|---|
| 1 最小文档经 `render_svg` 产出 SVG，`unsupported` 为空且无 `Error` | `tests/render_smoke/mod.rs` · `minimal_source_reaches_the_rendered_svg` |
| 2 未知环境导入成功，`unsupported` 有码，诊断 `span` 覆盖唯一出现 | `tests/fail_closed/mod.rs` · `unknown_environment_is_reported_with_its_source_span` |
| 3 外来数学语言进 `unsupported` 与 `Error`，导入不中断 | `tests/fail_closed/mod.rs` · `foreign_math_language_fails_closed`、`tex_math_without_a_language_tag_stays_accepted` |
| 4 空文档 `parsed.considered == 0`，`empty_document` 进 `lossy`，无诊断 | `tests/fail_closed/mod.rs` · `empty_source_keeps_document_level_losses_unpositioned` |
| 5 解析失败返回 `Err`，码 `parse_failed`，不产出图 | `tests/fail_closed/mod.rs` · `rejected_source_returns_a_structured_error` |
| 6 渲染器拒绝经 `From<Diagnostic>` 透传时码与 `severity` 不变 | `tests/fail_closed/mod.rs` · `renderer_rejection_keeps_its_own_diagnostic_code` |
| 7 表格、公式编号损失、脚注、citation、label/ref 闭环 | `tests/render_smoke/mod.rs` · `structures_reach_the_rendered_svg` |

验收 6 的边界：SVG 渲染器只拒绝画布尺寸非正的图形，而 TeX 入口不接收宿主
注册的 scene，所以该拒绝在 `render_svg` 的正常路径上不可达。用例固定的是
适配层唯一的契约——渲染器 `Diagnostic` 不被改名地转成 `TexInputError`。

## 构造覆盖

合同验收之外，下列构造的实际处置由用例固定。

| 构造 | 用例 |
|---|---|
| 嵌套 `itemize`：内层列表作为外层条目的 `children` 保留，两级文本都进 SVG | `tests/constructs/mod.rs` · `nested_lists_keep_the_inner_level_as_a_child` |
| `\section*`：不编号，`import.tex.starred_command` 进 `lossy` 与 `Warning` | `tests/constructs/mod.rs` · `starred_section_stays_unnumbered_and_is_reported` |
| `\href{url}{text}`：映射为 `Inline::Link` 外部 URI，SVG 输出 `<a href>`，命令不漏进正文 | `tests/constructs/mod.rs` · `href_becomes_an_external_link` |
| `\cite{a,b}` 与 `\cite[p.~3]{c}`：每个 key 各出一条 `[@key]` 标记，可选说明无 IR 载体并登记 `import.tex.citation_note` | `tests/constructs/mod.rs` · `citations_keep_every_key_and_ignore_the_optional_note` |
| 脚注正文挂在引用 `Inline::Note` 上，不摊平进段落文本，`import.tex.footnote_body` 仍登记 | `tests/constructs/mod.rs` · `footnote_body_is_carried_on_the_reference_not_the_flow` |
| metadata `\title` / `\author`、`\textbf` / `\emph` / `\texttt`、`verbatim`、`enumerate` 进页面 | `tests/constructs/mod.rs` · `metadata_verbatim_inline_styles_and_lists_reach_the_page` |
| `input-contracts.json` 的每条 claimed supported 用其 `sample` 实跑导入与渲染（`\label` 必须绑定前序块，`\ref` 才解析），清单不能比实现乐观 | `tests/contracts/mod.rs` · `every_supported_construct_is_importable_and_renderable` |
