# plotter-diagram

只做 `DiagramSpec → plan → scene`。Mermaid / DOT / PlantUML 前端在 `plotter-input-*`，不进本 crate。

门面：[`plotter`](../plotter/readme.md)。产品树说明见 [`../readme.md`](../readme.md)。

## Example

```rust
use plotter_diagram::compile::{LayoutOptions, compile};
use plotter_diagram::spec::{DiagramNode, DiagramSpec, Direction, NodeShape};

let mut spec = DiagramSpec::new(Direction::TopDown);
spec.nodes.push(DiagramNode {
    id: "A".into(),
    label: Some("Start".into()),
    shape: NodeShape::Box,
    ports: Vec::new(),
    cluster: None,
});
let plan = compile(&spec, &LayoutOptions::default());
assert!(!plan.nodes.is_empty());
```
