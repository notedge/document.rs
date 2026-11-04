# crawler-types

采集合同：`FetchArtifact`、frontier、抽取结果、事件与报告。语义正文不在这里——落在 `document-ir::DocumentGraph`。

诊断 namespace 为 `crawler`；schema 形如 `crawler.report/v0`。通常经门面 [`crawler`](../crawler/readme.md) 使用。

## Example

```rust
use crawler_types::{CrawlRequest, canonicalize_request_url};

let url = canonicalize_request_url("https://Example.com/a").expect("absolute http(s) URL");
let _ = CrawlRequest::from_seed(url.as_str()).expect("seedable URL");
```
