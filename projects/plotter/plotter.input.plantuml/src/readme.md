# plotter-input-plantuml

PlantUML 前端：Oak AST → [`DiagramSpec`](../plotter.diagram/readme.md) / scene。

接受 class / sequence 文档化子集（`@startuml`/`@enduml`、`/' ... '/` 块注释、类声明与六种关系、participant/actor、
四种消息箭头、activation）；类关键字保留为标签前缀（`«interface»`/`«abstract»`/`«enum»`）。其余拒绝并指向源字节范围。
不拼 scene 或 SVG。

产品树说明见 [`../readme.md`](../readme.md)。
