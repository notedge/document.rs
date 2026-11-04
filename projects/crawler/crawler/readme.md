# crawler

Crawler 编排门面：串联 `crawler-fetch` / `crawler-extract` / `crawler-store`，并再导出合同类型。根 API 提供
`plan_crawl` / `run_crawl` / `resume_crawl_from_checkpoint` / `run_extract` / `inspect_artifact`，浏览器能力走原生
`BrowserProvider`。

终端用户应使用下游 `@notedge/crawler` 等产品包装；应用集成可直接依赖本 crate。架构见 [`../readme.md`](../readme.md)。

## 再导出

| 模块                | 底层              |
|---------------------|-------------------|
| `crawler::types`    | `crawler-types`   |
| `crawler::fetch`    | `crawler-fetch`   |
| `crawler::semantic` | `crawler-extract` |
| `crawler::store`    | `crawler-store`   |

## Example

```rust
use crawler::plan_crawl;
use crawler::types::CrawlRequest;

let request = CrawlRequest::from_seed("https://example.com/").expect("seed parses");
let plan = plan_crawl(request).expect("crawl plan");
assert!(!plan.frontier.is_empty());
```

## 状态说明

礼貌抓取、多宿主调度等行为仍以冒烟与下游 fixtures 为准；不要仅凭本门面推断生产完备性。
