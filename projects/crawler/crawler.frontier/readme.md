# crawler-frontier

Frontier queue and navigation link admission for the Crawler stack.

The crate depends on `crawler-types` only. It owns bounded in-memory
scheduling and link expansion, and it exposes [`FrontierSink`] so a caller can
replace the queue with another implementation without changing the contract.
