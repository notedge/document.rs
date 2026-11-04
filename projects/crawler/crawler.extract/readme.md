# crawler-extract

语义抽取：把 `FetchArtifact`（通常是 HTML）降到 `document-ir::DocumentGraph`。

门面再导出为 `crawler::semantic`。见 [`crawler`](../crawler/readme.md)。

## Example

```rust
use crawler_extract::{Extractor, HtmlExtractor};

let extractors: [&dyn Extractor; 1] = [&HtmlExtractor];
```

把 fetch 阶段产出的 `crawler_fetch::FetchArtifact` 连同 `ExtractBudget` / `ExtractContext` 传给
`run_extract`；完整编排优先用门面 `crawler::run_extract`。
