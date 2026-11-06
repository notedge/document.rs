use crawler_types::{CrawlEvent, FetchArtifact, FrontierItem, PageChallengeState};

use crate::{
    error::Result,
    types::{
        BrowserSessionRecord, BrowserSnapshotInput, BrowserSnapshotRecord, ChallengePauseRecord, CheckpointView, ClaimToken,
        ObjectKind, ObjectRef, PageAssetBinding, PagePhase, PageTxHandle, ReleaseOutcome, ResumeState, RunHandle, RunSpec,
        StoreConfig, StoreDiagnosticReport,
    },
};

/// Store contract implemented by `MemoryStore` and `YydbStore`.
pub trait Store {
    /// Open or create a store backend.
    fn open(config: StoreConfig) -> Result<Self>
    where
        Self: Sized;

    /// Close and reopen the backend, simulating process restart.
    fn reopen(&mut self) -> Result<()>;

    /// Switch the active run to a persisted run and reclaim expired claims.
    ///
    /// The returned state is a derived view: committed pages, frontier records
    /// and the claim ledger remain authoritative.
    fn resume(&mut self, run_id: &str) -> Result<ResumeState>;

    /// Active run identifier, when a run has been started or resumed.
    fn active_run_id(&self) -> Result<Option<String>>;

    /// Start a crawl run.
    fn begin_run(&mut self, spec: RunSpec) -> Result<RunHandle>;

    /// Enqueue a frontier item.
    fn enqueue(&mut self, item: FrontierItem) -> Result<()>;

    /// Read one frontier item by identifier.
    fn frontier_item(&self, item_id: &str) -> Result<Option<FrontierItem>>;

    /// Count frontier items still queued or claimed in the active run.
    fn pending_frontier_count(&self) -> Result<u64>;

    /// Claim the next queued item for a host partition.
    fn claim_next(&mut self, host_key: &str, worker_id: &str) -> Result<Option<ClaimToken>>;

    /// Attempt a competing claim for dual-worker contract cases.
    fn second_worker_claim(&mut self, worker_id: &str) -> Result<()>;

    /// Extend the active claim lease and return the new expiry in epoch milliseconds.
    fn renew_claim(&mut self, token: &ClaimToken) -> Result<u64>;

    /// Release the active claim as done, requeued, or pending, rejecting stale fencing.
    fn release_claim(&mut self, token: &ClaimToken, outcome: ReleaseOutcome) -> Result<()>;

    /// Begin a page transaction bound to the active claim.
    fn begin_page_tx(&mut self, worker_id: &str) -> Result<PageTxHandle>;

    /// Attach a fetch artifact fixture to the active page transaction.
    fn page_tx_put_fetch(&mut self, fixture: &str, bytes: &[u8]) -> Result<()>;

    /// Attach an IR package fixture to the active page transaction.
    fn page_tx_put_ir(&mut self, fixture: &str, bytes: &[u8]) -> Result<()>;

    /// Bind one document asset to the active page transaction.
    fn page_tx_bind_asset(&mut self, binding: &PageAssetBinding) -> Result<()>;

    /// Admit discovered navigation links with the active page commit.
    fn page_tx_admit_links(&mut self, items: Vec<FrontierItem>) -> Result<()>;

    /// Stage one report event for the active page commit.
    fn page_tx_put_event(&mut self, event: &CrawlEvent) -> Result<()>;

    /// Commit the active page transaction.
    fn commit_page_tx(&mut self) -> Result<()>;

    /// Abort the active page transaction without committing.
    fn abort_page_tx(&mut self) -> Result<()>;

    /// Number of report events appended in the active run.
    fn event_count(&self) -> Result<u64>;

    /// Export a checkpoint view at the current event sequence and return its sequence.
    fn save_checkpoint_view(&mut self) -> Result<u64>;

    /// Read the last saved checkpoint view.
    fn checkpoint_view(&self) -> Result<Option<CheckpointView>>;

    /// Export the last saved checkpoint view as JSON for diagnostics only.
    fn checkpoint_view_json(&self) -> Result<Option<String>>;

    /// Read a page phase, or `auto` for the most recent page.
    fn page_phase(&self, page_id: &str) -> Result<PagePhase>;

    /// Read the asset fingerprints bound to a committed page.
    fn page_asset_refs(&self, page_id: &str) -> Result<Vec<String>>;

    /// Store large bytes in the content-addressed object store, deduplicating by fingerprint.
    fn put_object(&mut self, kind: ObjectKind, media_type: Option<&str>, bytes: &[u8]) -> Result<ObjectRef>;

    /// Read object bytes back by fingerprint.
    fn get_object(&self, fingerprint: &str) -> Result<Option<Vec<u8>>>;

    /// Read object metadata by fingerprint.
    fn object_ref(&self, fingerprint: &str) -> Result<Option<ObjectRef>>;

    /// Read the object refs attached to a page in fetch, IR, then asset order.
    fn page_object_refs(&self, page_id: &str) -> Result<Vec<ObjectRef>>;

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

    /// Store a redacted browser snapshot archive and its cache record.
    ///
    /// The archive goes into the ObjectStore as [`ObjectKind::BrowserArchive`].
    /// Archives carrying credential plaintext are rejected before any write.
    fn put_browser_snapshot(&mut self, input: BrowserSnapshotInput) -> Result<BrowserSnapshotRecord>;

    /// Read a browser snapshot cache record by key.
    fn get_browser_snapshot(&self, snapshot_key: &str) -> Result<Option<BrowserSnapshotRecord>>;

    /// Record a challenge pause in the active run state and append its report event.
    fn record_challenge_pause(
        &mut self,
        item_id: &str,
        page_url: &str,
        state: PageChallengeState,
    ) -> Result<ChallengePauseRecord>;

    /// Read the challenge pause recorded for one frontier item.
    fn challenge_pause(&self, item_id: &str) -> Result<Option<ChallengePauseRecord>>;

    /// Persist the active run's browser session reference.
    ///
    /// Only an external vault handle may be stored, never a cookie or token.
    fn put_browser_session(&mut self, record: &BrowserSessionRecord) -> Result<()>;

    /// Read the active run's browser session reference.
    fn browser_session(&self) -> Result<Option<BrowserSessionRecord>>;
}
