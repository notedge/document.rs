use crawler_types::{CrawlEvent, FrontierItem, PageChallengeState, site_key};
use url::Url;

use crate::{
    Store,
    error::{Result, StoreError},
    session::StoreSession,
    types::{
        BrowserSessionRecord, BrowserSnapshotInput, BrowserSnapshotRecord, ChallengePauseRecord, CheckpointView, ClaimToken,
        ObjectKind, ObjectRef, PageAssetBinding, PagePhase, PageTxHandle, ReleaseOutcome, RunHandle, RunSpec, StoreConfig,
        StoreDiagnosticReport,
    },
};

/// In-memory `Store` backend for deterministic contract tests.
#[derive(Debug, Clone)]
pub struct MemoryStore {
    session: StoreSession,
}

impl Store for MemoryStore {
    fn open(config: StoreConfig) -> Result<Self> {
        Ok(Self { session: StoreSession::new(config.claim_lease) })
    }

    fn reopen(&mut self) -> Result<()> {
        self.session.reopen()
    }

    fn begin_run(&mut self, spec: RunSpec) -> Result<RunHandle> {
        self.session.begin_run(spec)
    }

    fn enqueue(&mut self, item: FrontierItem) -> Result<()> {
        self.session.enqueue(item)
    }

    fn frontier_item(&self, item_id: &str) -> Result<Option<FrontierItem>> {
        self.session.frontier_item(item_id)
    }

    fn pending_frontier_count(&self) -> Result<u64> {
        self.session.pending_frontier_count()
    }

    fn claim_next(&mut self, host_key: &str, worker_id: &str) -> Result<Option<ClaimToken>> {
        self.session.claim_next(host_key, worker_id)
    }

    fn second_worker_claim(&mut self, worker_id: &str) -> Result<()> {
        self.session.second_worker_claim(worker_id)
    }

    fn renew_claim(&mut self, token: &ClaimToken) -> Result<u64> {
        self.session.renew_claim(token)
    }

    fn release_claim(&mut self, token: &ClaimToken, outcome: ReleaseOutcome) -> Result<()> {
        self.session.release_claim(token, outcome)
    }

    fn begin_page_tx(&mut self, worker_id: &str) -> Result<PageTxHandle> {
        self.session.begin_page_tx(worker_id)
    }

    fn page_tx_put_fetch(&mut self, fixture: &str, bytes: &[u8]) -> Result<()> {
        self.session.page_tx_put_fetch(fixture, bytes)
    }

    fn page_tx_put_ir(&mut self, fixture: &str, bytes: &[u8]) -> Result<()> {
        self.session.page_tx_put_ir(fixture, bytes)
    }

    fn page_tx_bind_asset(&mut self, binding: &PageAssetBinding) -> Result<()> {
        self.session.page_tx_bind_asset(binding)
    }

    fn page_tx_admit_links(&mut self, items: Vec<FrontierItem>) -> Result<()> {
        self.session.page_tx_admit_links(items)
    }

    fn page_tx_put_event(&mut self, event: &CrawlEvent) -> Result<()> {
        self.session.page_tx_put_event(event)
    }

    fn commit_page_tx(&mut self) -> Result<()> {
        self.session.commit_page_tx()
    }

    fn abort_page_tx(&mut self) -> Result<()> {
        self.session.abort_page_tx()
    }

    fn event_count(&self) -> Result<u64> {
        self.session.event_count()
    }

    fn save_checkpoint_view(&mut self) -> Result<u64> {
        self.session.save_checkpoint_view()
    }

    fn checkpoint_view(&self) -> Result<Option<CheckpointView>> {
        self.session.checkpoint_view()
    }

    fn checkpoint_view_json(&self) -> Result<Option<String>> {
        self.session
            .checkpoint_view()?
            .map(|view| serde_json::to_string(&view).map_err(|error| StoreError::StoreCorrupt(error.to_string())))
            .transpose()
    }

    fn page_phase(&self, page_id: &str) -> Result<PagePhase> {
        self.session.page_phase(page_id)
    }

    fn page_asset_refs(&self, page_id: &str) -> Result<Vec<String>> {
        self.session.page_asset_refs(page_id)
    }

    fn put_object(&mut self, kind: ObjectKind, media_type: Option<&str>, bytes: &[u8]) -> Result<ObjectRef> {
        self.session.insert_object(kind, media_type, bytes)
    }

    fn get_object(&self, fingerprint: &str) -> Result<Option<Vec<u8>>> {
        self.session.get_object(fingerprint)
    }

    fn object_ref(&self, fingerprint: &str) -> Result<Option<ObjectRef>> {
        self.session.object_ref(fingerprint)
    }

    fn page_object_refs(&self, page_id: &str) -> Result<Vec<ObjectRef>> {
        self.session.page_object_refs(page_id)
    }

    fn put_orphan_object(&mut self, _bytes: &[u8]) -> Result<()> {
        Ok(())
    }

    fn doctor(&self) -> Result<StoreDiagnosticReport> {
        let checkpoint_lag_records = self.session.checkpoint_lag();
        let mut issues = Vec::new();
        if checkpoint_lag_records > 0 {
            issues.push(crate::StoreDoctorIssue {
                severity: crate::StoreDoctorSeverity::Warn,
                code: "store.doctor.checkpoint_stale".into(),
                message: format!("checkpoint lags commit by {checkpoint_lag_records} records"),
            });
        }
        Ok(StoreDiagnosticReport { checkpoint_lag_records, issues, ..StoreDiagnosticReport::default() })
    }

    fn get_response_cache(&self, cache_key: &str) -> Result<Option<crawler_types::FetchArtifact>> {
        self.session.get_response_cache(cache_key)
    }

    fn put_response_cache(&mut self, artifact: &crawler_types::FetchArtifact) -> Result<()> {
        self.session.put_response_cache(artifact)
    }

    fn get_robots_cache(&self, site_key: &str) -> Result<Option<Vec<u8>>> {
        self.session.get_robots_cache(site_key)
    }

    fn put_robots_cache(&mut self, site_key: &str, body: &[u8], ttl: std::time::Duration) -> Result<()> {
        self.session.put_robots_cache(site_key, body, ttl)
    }

    fn put_browser_snapshot(&mut self, input: BrowserSnapshotInput) -> Result<BrowserSnapshotRecord> {
        self.session.put_browser_snapshot(input)
    }

    fn get_browser_snapshot(&self, snapshot_key: &str) -> Result<Option<BrowserSnapshotRecord>> {
        self.session.get_browser_snapshot(snapshot_key)
    }

    fn record_challenge_pause(
        &mut self,
        item_id: &str,
        page_url: &str,
        state: PageChallengeState,
    ) -> Result<ChallengePauseRecord> {
        self.session.record_challenge_pause(item_id, page_url, state)
    }

    fn challenge_pause(&self, item_id: &str) -> Result<Option<ChallengePauseRecord>> {
        self.session.challenge_pause(item_id)
    }

    fn put_browser_session(&mut self, record: &BrowserSessionRecord) -> Result<()> {
        self.session.put_browser_session(record)
    }

    fn browser_session(&self) -> Result<Option<BrowserSessionRecord>> {
        self.session.browser_session()
    }
}

/// Build a [`FrontierItem`] from contract JSON fields.
pub fn frontier_item_from_contract(canonical_request_url: &str, depth: u32) -> Result<FrontierItem> {
    let request_url = Url::parse(canonical_request_url).map_err(|error| StoreError::StoreCorrupt(error.to_string()))?;
    let site = site_key(&request_url);
    Ok(FrontierItem {
        request_url,
        discovery: crawler_types::DiscoverySource::Seed,
        depth,
        site_key: site,
        priority: 0,
        attempts: 0,
        discovered_at_epoch: crawler_types::now_epoch(),
    })
}
