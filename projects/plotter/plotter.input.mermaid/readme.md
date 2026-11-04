# plotter-input-mermaid

Mermaid 前端：Oak AST → [`DiagramSpec`](../plotter.diagram) / scene。

flowchart / class / sequence 子集降到 diagram 管道；不组装 SVG。解析错误与未支持的 sequence 语句（含 `->>+` 激活简写）都以源字节范围拒绝，布局诊断受 Oak AST 范围限制时可能没有 token 级主标签。

产品树说明见 [`../readme.md`](../readme.md)。
