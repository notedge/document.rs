# crawler-types

Shared crawl and extraction contracts for Crawler. Defines the shapes adapters and stores agree on before any HTTP
request or HTML parse runs.

Semantic document content is **not** owned here—it lives in `notedown-ir::DocumentGraph`.

## 📋 Core types

| Type            | Role                                           |
|-----------------|------------------------------------------------|
| `FetchArtifact` | Normalized fetch result passed to extractors   |
| `FrontierItem`  | Planned or queued crawl target                 |
| `ExtractResult` | Extraction outcome and document handle         |
| `CrawlEvent`    | Progress and state transitions during a run    |
| `CrawlReport`   | Serializable counters, diagnostics, and status |

Policies for robots, challenges, and budgets are expressed as types in this crate and enforced in `crawler.fetch` /
`crawler`.

## 🔌 Who depends on this

- `crawler.fetch` — produces `FetchArtifact`
- `crawler.extract` — consumes artifacts, emits `ExtractResult`
- `crawler.store` — persists crawl keys and checkpoints
- `crawler-napi` — serializes reports to JSON for Node

Keep breaking changes rare: npm and CLI reports deserialize these contracts.
