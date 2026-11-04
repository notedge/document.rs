# plotter-input-mermaid

Mermaid 前端：Oak AST → [`DiagramSpec`](../plotter.diagram/readme.md) / scene。

flowchart / class / sequence / state / ER 子集降到 diagram 管道；不组装 SVG。解析错误、未支持的 sequence 语句（含
`->>+` 激活简写），以及 state / ER 里上游会丢弃的语句都以源字节范围拒绝，布局诊断受 Oak AST 范围限制时可能没有 token 级主标签。

state 图把状态与迁移降成节点与有向边，`[*]` 按作用域生成圆形伪状态，复合状态的每个并发区域降成一个簇框。ER 图把实体与
属性降成带多行标签的节点，两端 Crow's Foot 基数（`||` / `o|` / `|{` / `o{`）写进关系标签，非识别关系用虚线样式并由布局报告近似。

产品树说明见 [`../readme.md`](../readme.md)。
