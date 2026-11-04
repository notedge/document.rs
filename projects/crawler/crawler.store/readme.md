# crawler-store

采集持久化：`MemoryStore` 与可选 `YydbStore`，覆盖响应缓存、checkpoint 与页面事务合同。

复杂合同 JSON / doctor 套件在 **下游**产品仓；本仓测试只留冒烟。门面：[`crawler`](../crawler/readme.md)。

## Example

```rust
use crawler_store::{MemoryStore, Store, StoreConfig};

let mut store = MemoryStore::open(StoreConfig::memory()).expect("memory store opens");
store.reopen().expect("reopen keeps session contract");
```

响应缓存、robots 缓存与 checkpoint 均在 `Store` trait 上，由具体后端决定持久化方式。
