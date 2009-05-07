# crawler-store

Persistence adapter for Crawler crawls: `Store` trait, keyspace encoding, in-memory storage, and optional `YydbStore`
integration.

Checkpoints, committed page indexes, and crawl metadata flow through this crate so `crawler resume` can continue a
paused run.

## 🗄 Implementations

| Backend       | Use                                                       |
|---------------|-----------------------------------------------------------|
| `MemoryStore` | Tests, ephemeral runs, doctor checks                      |
| `YydbStore`   | Durable local storage when `yydb` is enabled in the build |

## 🔑 Keyspace

Keys encode crawl identity, frontier state, and artifact references in a stable layout shared with diagnostics. Do not
invent parallel key formats in adapters—extend the trait instead.

## 🔌 Integration

`crawler` writes checkpoints when crawl options request them. Node users pass `--checkpoint-out` or `--checkpoint-dir`
on the CLI; the store layout is opaque to npm consumers but required for `resume`.
