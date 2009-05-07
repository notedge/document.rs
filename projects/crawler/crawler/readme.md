# crawler

Rust crawl orchestration facade for Crawler. Wires `crawler.fetch` transports, `crawler.extract` semantic extractors,
and optional `crawler.store` persistence into plan, fetch, crawl, resume, extract, and inspect flows consumed by
`crawler-napi` and tests.

CLI users should install `@notedge/crawler` instead of depending on this crate directly.

## 🧩 Ownership

| Concern                              | Crate             |
|--------------------------------------|-------------------|
| Contracts (`FetchArtifact`, reports) | `crawler.types`   |
| HTTP/file/memory fetch               | `crawler.fetch`   |
| HTML → `document.ir`                 | `crawler.extract` |
| Checkpoints and crawl storage        | `crawler.store`   |
| Node-API surface                     | `crawler-napi`    |

Document semantics live in `document-ir::DocumentGraph`, not in this crate.

## 🔌 Integration

Downstream adapters call the public Rust API to run bounded crawls and extraction. Browser sessions go through a
`BrowserProvider` capability on native paths only.

Submodule re-exports mirror the `document` facade pattern:

| Module | Crate |
|--------|-------|
| `crawler::types` | `crawler.types` |
| `crawler::fetch` | `crawler.fetch` |
| `crawler::semantic` | `crawler.extract` |
| `crawler::store` | `crawler.store` |

Root-level exports remain the orchestration entry points for crawl, extract, and inspect flows.

产品树架构见 [`../readme.md`](../readme.md)。测试入口在本 crate 的 `tests/`。

## ⚠️ Release status

Collection behavior is still validated against fixtures and small scopes. Do not treat politeness, Oak HTML extraction,
or multi-host scheduling as production-complete from this facade alone.

Repository: https://github.com/notedge/document.rs
