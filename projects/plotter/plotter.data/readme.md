# `plotter.data`

Apollo 的数据容器层，负责把外部数据整理成可验证、可复用的绘图输入。

提供 `ColumnTable` 列式数据、`GridData` 规则网格、`GraphData` 图数据和 `TreeData` 树数据，并处理列类型、缺失值、行视图和批次边界。这里不定义
geom、scale、坐标系，也不包含渲染器或数学求值逻辑。

```sh
cargo test -p apollo-data
```
