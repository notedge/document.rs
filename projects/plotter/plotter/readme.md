# `apollo`

Apollo 绘图引擎的稳定公共 Rust 门面。它重新导出数据、图形语法、Scene IR、布局和渲染器的常用类型，便于应用只依赖一个入口。

默认 feature 是 `svg`。启用 `wgpu` 后可以使用 WGPU/WebGPU 后端。facade 不新增独立语义，也不隐藏各层的职责边界。

```sh
cargo test -p apollo
cargo test -p apollo --features wgpu
```
