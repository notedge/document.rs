# crawler-browser

Browser snapshot plumbing for the Crawler stack.

The crate depends on `crawler-types` and `crawler-extract` only. It owns the
replaceable `BrowserProvider` capability, fixture and HTTP worker providers,
snapshot JSON I/O, and the challenge-aware extraction path. It never depends on
the crawl orchestrator, so the orchestrator selects a provider instead of
owning one.
