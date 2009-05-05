# `apollo-grammar`

声明式 Grammar of Graphics 层，描述“数据如何成为图”。

```text
ColumnTable | GridData | GraphData | TreeData → PlotSpec → Scene
```

编译链路是 `validate → stat → scale → layout → scene`，其中图和树布局委托给 `plotter.layout`。编译结果是后端无关的 Scene
IR，不直接调用 CPU、SVG 或 WGPU。

本 crate 拥有 `PlotSpec`、mapping、layer、geom、stat、scale、coordinate、facet 和 theme 的公共定义与编译入口，不拥有渲染后端。

```sh
cargo test -p apollo-grammar
```
