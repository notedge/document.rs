# plotter-input-dot

Graphviz DOT 前端：Oak AST → [`DiagramSpec`](../plotter.diagram) / scene。

只接受文档化白名单（`digraph`/`graph`、`rankdir`、一层 `subgraph cluster_*`、`node`/`edge` 默认属性（含子图内作用域）、边标签与 compass port）；其余以结构化诊断拒绝。`style=rounded` 映射到圆角节点，`filled` 与虚线类样式记 `approximated` 警告并指向源字节范围。不拼 scene 节点或 SVG。

产品树说明见 [`../readme.md`](../readme.md)。
