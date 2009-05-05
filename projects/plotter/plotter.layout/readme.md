# `plotter.layout`

2D 面板布局与图/树布局：

- 分面：`layout_facet_panels` / `PanelRect`
- 图：`CircularLayout` / `GridLayout` / `LayeredLayout` / `ForceLayout`
- 树：`TidyTreeLayout` / `RadialTreeLayout`

输出 `LayoutResult`（位置、面板和边路由），不生成 Scene 节点，也不管理 GPU 资源。布局输入是已整理的数据和约束，输出可被 grammar
编译阶段消费。

```sh
cargo test -p apollo-layout
```
