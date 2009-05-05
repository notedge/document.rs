# `apollo-scene`

后端无关的 Scene IR，是 CPU、SVG 和 GPU 后端共享的绘制合同。

场景使用 arena 和稳定节点 ID，包含 2D 节点、3D 网格与点云、相机、视口、资源描述和交互区域。它还提供投影、射线转换和 `pick_at`
拾取能力。Scene IR 不包含图形语法、数据处理、CAS 表达式或设备对象，渲染器只能消费已经生成的场景。

```sh
cargo test -p apollo-scene
```
