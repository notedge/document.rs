# plotter-types

Plotter 共享原语：几何 / 颜色 / 变换 / 诊断；GoG 侧 ID 与诊断码在 `plotter_types::plot`（原 apollo-types，**无**单独
`plotter.core`）。

一般依赖门面 [`plotter`](../plotter/readme.md) 即可；只有需要收窄依赖面时才直接用本 crate。

## Example

```rust
use plotter_types::{Color, Point, Rect};

let frame = Rect::new(0.0, 0.0, 320.0, 240.0);
let _ = (frame, Color::rgba(26, 51, 77, 255), Point::new(1.0, 2.0));
```
