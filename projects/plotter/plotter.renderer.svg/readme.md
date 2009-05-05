# `plotter.renderer.svg`

可选（由 `apollo` 默认启用）的 SVG 矢量后端，将已准备好的 Scene IR 导出为静态 SVG。适合文档、服务端生成和无 GPU 环境。

本 crate 不拥有图形语法、数据统计、布局或 GPU 生命周期，只负责把场景节点转换为 SVG 元素。

```sh
cargo test -p apollo-backend-svg
```
