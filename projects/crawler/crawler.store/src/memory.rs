use crawler_types::{FrontierItem, site_key};
use url::Url;

use crate::{
    Store,
    error::{Result, StoreError},
    session::StoreSession,
    types::{ClaimToken, PagePhase, PageTxHandle, ReleaseOutcome, RunHandle, RunSpec, StoreConfig, StoreDiagnosticReport},
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

    fn page_phase(&self, page_id: &str) -> Result<PagePhase> {
        self.session.page_phase(page_id)
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
