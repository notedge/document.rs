# `plotter.renderer.wgpu`

可选的 WGPU / WebGPU 后端，把 Scene IR 绘制到 GPU 目标或离屏 RGBA8 缓冲。当前包含折线、坐标轴等基础路径，适合与 CPU
reference 做结果对照。

默认不进入 `apollo` 核心依赖，通过 `apollo` 的 `wgpu` feature 启用。没有可用 GPU 时应由上层选择 CPU 或 SVG 降级。

```sh
cargo test -p apollo-backend-wgpu
cargo test -p apollo --features wgpu
```
