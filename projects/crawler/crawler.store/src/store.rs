use crawler_types::{FetchArtifact, FrontierItem};

use crate::{
    error::Result,
    types::{ClaimToken, PagePhase, PageTxHandle, RunHandle, RunSpec, StoreConfig, StoreDiagnosticReport},
};

/// Store contract implemented by `MemoryStore` and `YydbStore`.
pub trait Store {
    /// Open or create a store backend.
    fn open(config: StoreConfig) -> Result<Self>
    where
        Self: Sized;

    /// Close and reopen the backend, simulating process restart.
    fn reopen(&mut self) -> Result<()>;

    /// Start a crawl run.
    fn begin_run(&mut self, spec: RunSpec) -> Result<RunHandle>;

    /// Enqueue a frontier item.
    fn enqueue(&mut self, item: FrontierItem) -> Result<()>;

    /// Claim the next queued item for a host partition.
    fn claim_next(&mut self, host_key: &str, worker_id: &str) -> Result<Option<ClaimToken>>;

    /// Attempt a competing claim for dual-worker contract cases.
    fn second_worker_claim(&mut self, worker_id: &str) -> Result<()>;

    /// Begin a page transaction bound to the active claim.
    fn begin_page_tx(&mut self, worker_id: &str) -> Result<PageTxHandle>;

    /// Attach a fetch artifact fixture to the active page transaction.
    fn page_tx_put_fetch(&mut self, fixture: &str, bytes: &[u8]) -> Result<()>;

    /// Attach an IR package fixture to the active page transaction.
    fn page_tx_put_ir(&mut self, fixture: &str, bytes: &[u8]) -> Result<()>;

    /// Commit the active page transaction.
    fn commit_page_tx(&mut self) -> Result<()>;

    /// Abort the active page transaction without committing.
    fn abort_page_tx(&mut self) -> Result<()>;

    /// Number of report events appended in the active run.
    fn event_count(&self) -> Result<u64>;

    /// Export a checkpoint view at the current event sequence and return it.
    fn save_checkpoint_view(&mut self) -> Result<u64>;

    /// Read a page phase, or `auto` for the most recent page.
    fn page_phase(&self, page_id: &str) -> Result<PagePhase>;

    /// Store an unreferenced CAS object for orphan reclamation tests.
    fn put_orphan_object(&mut self, bytes: &[u8]) -> Result<()>;

    /// Run read-only consistency probes.
    fn doctor(&self) -> Result<StoreDiagnosticReport>;

    /// Read a cached fetch artifact by cache key.
    fn get_response_cache(&self, cache_key: &str) -> Result<Option<FetchArtifact>>;

    /// Persist a fetch artifact in the response cache namespace.
    fn put_response_cache(&mut self, artifact: &FetchArtifact) -> Result<()>;

    /// Read cached robots document bytes for a site key.
    fn get_robots_cache(&self, site_key: &str) -> Result<Option<Vec<u8>>>;

    /// Store robots document bytes with a time-to-live.
    fn put_robots_cache(&mut self, site_key: &str, body: &[u8], ttl: std::time::Duration) -> Result<()>;
}
