# crawler-fetch

传输层：HTTP / 本地文件 / 内存源，附带响应缓存与 robots 钩子，产出 `FetchArtifact`。

礼貌抓取策略仍属原型强度；生产调度与大型 fixtures 在下游。门面：[`crawler`](../crawler/readme.md)。

## Example

```rust
use crawler_fetch::{FetchClient, FetchRequest, MemoryTransport, RobotsPolicy};
use crawler_types::PolitenessProfile;
use url::Url;

let transport = MemoryTransport::new();
let client = FetchClient::new(transport, RobotsPolicy::from_profile(PolitenessProfile::Developer));
let req = FetchRequest { url: Url::parse("https://example.com/")?, headers: Default::default() };
let _artifact = client.fetch(&req)?;
```
