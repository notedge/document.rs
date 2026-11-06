use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crawler_types::{FetchArtifact, FrontierItem, TransportProvenance};
use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, StoreError},
    keyspace::{item_id_from_url, run_id_from_seed},
    types::{ClaimToken, PagePhase, PageTxHandle, RunHandle, RunSpec},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ClaimRecord {
    worker_id: String,
    fencing_token: u64,
    lease_until_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PageRecord {
    item_id: String,
    worker_id: String,
    fencing_token: u64,
    phase: PagePhase,
    fetch_fixture: Option<String>,
    ir_fixture: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RunState {
    seed_url: String,
    frontier: BTreeMap<String, FrontierItem>,
    claims: BTreeMap<String, ClaimRecord>,
    pages: BTreeMap<String, PageRecord>,
    event_count: u64,
    next_fencing: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RobotsCacheEntry {
    body: Vec<u8>,
    expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionSnapshot {
    runs: BTreeMap<String, RunState>,
    active_run_id: Option<String>,
    #[serde(default)]
    response_cache: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    robots_cache: BTreeMap<String, RobotsCacheEntry>,
}

/// One committed page exposed by the legacy session migration path.
#[derive(Debug, Clone)]
pub(crate) struct LegacyPage {
    pub(crate) page_id: String,
    pub(crate) item_id: String,
    pub(crate) worker_id: String,
    pub(crate) fencing_token: u64,
    pub(crate) phase: PagePhase,
    pub(crate) fetch_fixture: Option<String>,
    pub(crate) ir_fixture: Option<String>,
}

/// One run exposed by the legacy session migration path.
#[derive(Debug, Clone)]
pub(crate) struct LegacyRun {
    pub(crate) run_id: String,
    pub(crate) seed_url: String,
    pub(crate) event_count: u64,
    pub(crate) frontier: Vec<FrontierItem>,
    pub(crate) pages: Vec<LegacyPage>,
}

/// Shared store session state used by memory and YYDB backends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoreSession {
    runs: BTreeMap<String, RunState>,
    active_run_id: Option<String>,
    #[serde(default)]
    response_cache: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    robots_cache: BTreeMap<String, RobotsCacheEntry>,
    #[serde(default)]
    claim_lease: Duration,
    snapshot: SessionSnapshot,
    last_claim: Option<ClaimToken>,
    active_page_tx: Option<PageTxHandle>,
}

impl StoreSession {
    pub(crate) fn new(claim_lease: Duration) -> Self {
        Self {
            runs: BTreeMap::new(),
            active_run_id: None,
            response_cache: BTreeMap::new(),
            robots_cache: BTreeMap::new(),
            claim_lease,
            snapshot: SessionSnapshot {
                runs: BTreeMap::new(),
                active_run_id: None,
                response_cache: BTreeMap::new(),
                robots_cache: BTreeMap::new(),
            },
            last_claim: None,
            active_page_tx: None,
        }
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(|error| StoreError::StoreCorrupt(error.to_string()))
    }

    fn now_millis() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
    }

    pub(crate) fn persist_snapshot(&mut self) {
        self.snapshot = SessionSnapshot {
            runs: self.runs.clone(),
            active_run_id: self.active_run_id.clone(),
            response_cache: self.response_cache.clone(),
            robots_cache: self.robots_cache.clone(),
        };
    }

    pub(crate) fn reopen(&mut self) -> Result<()> {
        self.runs = self.snapshot.runs.clone();
        self.active_run_id = self.snapshot.active_run_id.clone();
        self.response_cache = self.snapshot.response_cache.clone();
        self.robots_cache = self.snapshot.robots_cache.clone();
        self.last_claim = None;
        self.active_page_tx = None;
        if let Some(run_id) = &self.active_run_id {
            if let Some(run) = self.runs.get(run_id) {
                if let Some((item_id, claim)) = run.claims.iter().next() {
                    self.last_claim = Some(ClaimToken {
                        item_id: item_id.clone(),
                        worker_id: claim.worker_id.clone(),
                        fencing_token: claim.fencing_token,
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn begin_run(&mut self, spec: RunSpec) -> Result<RunHandle> {
        let run_id = run_id_from_seed(&spec.seed_url);
        let run = RunState {
            seed_url: spec.seed_url,
            frontier: BTreeMap::new(),
            claims: BTreeMap::new(),
            pages: BTreeMap::new(),
            event_count: 0,
            next_fencing: 0,
        };
        self.runs.insert(run_id.clone(), run);
        self.active_run_id = Some(run_id.clone());
        self.last_claim = None;
        self.active_page_tx = None;
        self.persist_snapshot();
        Ok(RunHandle { run_id })
    }

    pub(crate) fn enqueue(&mut self, item: FrontierItem) -> Result<()> {
        let run = self.active_run_mut()?;
        let item_id = item_id_from_url(item.request_url.as_str());
        run.frontier.insert(item_id, item);
        self.persist_snapshot();
        Ok(())
    }

    pub(crate) fn claim_next(&mut self, host_key: &str, worker_id: &str) -> Result<Option<ClaimToken>> {
        let now = Self::now_millis();
        let lease_millis = self.claim_lease.as_millis() as u64;
        let run = self.active_run_mut()?;
        for (item_id, item) in &run.frontier {
            if item.site_key != host_key {
                continue;
            }
            // A committed page is terminal for its frontier item and must not be re-claimed.
            if run.pages.get(item_id).is_some_and(|page| page.phase == PagePhase::Committed) {
                continue;
            }
            if let Some(claim) = run.claims.get(item_id) {
                if claim.lease_until_ms > now {
                    if claim.worker_id == worker_id {
                        let token = ClaimToken {
                            item_id: item_id.clone(),
                            worker_id: worker_id.to_owned(),
                            fencing_token: claim.fencing_token,
                        };
                        self.last_claim = Some(token.clone());
                        return Ok(Some(token));
                    }
                    continue;
                }
                // Expired claims are reclaimable with a fresh fencing token.
            }
            run.next_fencing += 1;
            let token =
                ClaimToken { item_id: item_id.clone(), worker_id: worker_id.to_owned(), fencing_token: run.next_fencing };
            run.claims.insert(
                item_id.clone(),
                ClaimRecord {
                    worker_id: worker_id.to_owned(),
                    fencing_token: run.next_fencing,
                    lease_until_ms: now + lease_millis,
                },
            );
            self.last_claim = Some(token.clone());
            self.persist_snapshot();
            return Ok(Some(token));
        }
        Ok(None)
    }

    pub(crate) fn second_worker_claim(&mut self, worker_id: &str) -> Result<()> {
        let run = self.active_run()?;
        let item_id =
            run.claims.keys().next().cloned().ok_or_else(|| StoreError::StoreInvalidState("no claimed item".into()))?;
        self.last_claim = Some(ClaimToken { item_id, worker_id: worker_id.to_owned(), fencing_token: 0 });
        Ok(())
    }

    pub(crate) fn begin_page_tx(&mut self, worker_id: &str) -> Result<PageTxHandle> {
        let claim = self.last_claim.clone().ok_or_else(|| StoreError::StoreInvalidState("no claim token".into()))?;
        if claim.worker_id != worker_id {
            return Err(StoreError::StoreFencingMismatch);
        }
        let run = self.active_run_mut()?;
        let active = run.claims.get(&claim.item_id).ok_or(StoreError::StoreFencingMismatch)?;
        if active.worker_id != worker_id || active.fencing_token != claim.fencing_token {
            return Err(StoreError::StoreFencingMismatch);
        }
        if active.lease_until_ms <= Self::now_millis() {
            return Err(StoreError::StoreLeaseExpired);
        }
        let page_id = claim.item_id.clone();
        run.pages.insert(
            page_id.clone(),
            PageRecord {
                item_id: claim.item_id.clone(),
                worker_id: worker_id.to_owned(),
                fencing_token: claim.fencing_token,
                phase: PagePhase::Claimed,
                fetch_fixture: None,
                ir_fixture: None,
            },
        );
        let handle = PageTxHandle { page_id };
        self.active_page_tx = Some(handle.clone());
        self.persist_snapshot();
        Ok(handle)
    }

    pub(crate) fn page_tx_put_fetch(&mut self, fixture: &str, bytes: &[u8]) -> Result<()> {
        let _ = bytes;
        let page_id =
            self.active_page_tx.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no page tx".into()))?.page_id.clone();
        let run = self.active_run_mut()?;
        let page = run.pages.get_mut(&page_id).ok_or_else(|| StoreError::StoreNotFound(page_id.clone()))?;
        page.fetch_fixture = Some(fixture.to_owned());
        page.phase = PagePhase::Fetched;
        self.persist_snapshot();
        Ok(())
    }

    pub(crate) fn page_tx_put_ir(&mut self, fixture: &str, bytes: &[u8]) -> Result<()> {
        let _ = bytes;
        let page_id =
            self.active_page_tx.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no page tx".into()))?.page_id.clone();
        let run = self.active_run_mut()?;
        let page = run.pages.get_mut(&page_id).ok_or_else(|| StoreError::StoreNotFound(page_id.clone()))?;
        page.ir_fixture = Some(fixture.to_owned());
        page.phase = PagePhase::IrValidated;
        self.persist_snapshot();
        Ok(())
    }

    pub(crate) fn commit_page_tx(&mut self) -> Result<()> {
        let page_id =
            self.active_page_tx.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no page tx".into()))?.page_id.clone();
        let run = self.active_run_mut()?;
        let claim = run.claims.get(&page_id).ok_or(StoreError::StoreFencingMismatch)?;
        if claim.lease_until_ms <= Self::now_millis() {
            return Err(StoreError::StoreLeaseExpired);
        }
        let page = run.pages.get_mut(&page_id).ok_or_else(|| StoreError::StoreNotFound(page_id.clone()))?;
        page.phase = PagePhase::Committed;
        run.event_count += 1;
        self.active_page_tx = None;
        self.persist_snapshot();
        Ok(())
    }

    pub(crate) fn abort_page_tx(&mut self) -> Result<()> {
        let page_id =
            self.active_page_tx.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no page tx".into()))?.page_id.clone();
        let run = self.active_run_mut()?;
        run.pages.remove(&page_id);
        self.active_page_tx = None;
        self.persist_snapshot();
        Ok(())
    }

    pub(crate) fn event_count(&self) -> Result<u64> {
        Ok(self.active_run()?.event_count)
    }

    pub(crate) fn page_phase(&self, page_id: &str) -> Result<PagePhase> {
        if page_id == "auto" {
            return self.page_phase_for_auto();
        }
        let run = self.active_run()?;
        run.pages.get(page_id).map(|page| page.phase).ok_or_else(|| StoreError::StoreNotFound(page_id.to_owned()))
    }

    fn active_run(&self) -> Result<&RunState> {
        let run_id = self.active_run_id.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        self.runs.get(run_id).ok_or_else(|| StoreError::StoreNotFound(run_id.clone()))
    }

    fn active_run_mut(&mut self) -> Result<&mut RunState> {
        let run_id = self.active_run_id.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?.clone();
        self.runs.get_mut(&run_id).ok_or_else(|| StoreError::StoreNotFound(run_id))
    }

    fn page_phase_for_auto(&self) -> Result<PagePhase> {
        if let Ok(page) = self.page_for_auto() {
            return Ok(page.phase);
        }
        let claim = self.last_claim.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active claim".into()))?;
        let run = self.active_run()?;
        if run.claims.contains_key(&claim.item_id) {
            return Ok(PagePhase::Claimed);
        }
        Err(StoreError::StoreNotFound("auto page".into()))
    }

    fn page_for_auto(&self) -> Result<&PageRecord> {
        let run = self.active_run()?;
        let claim = self.last_claim.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active claim".into()))?;
        run.pages
            .values()
            .find(|page| page.item_id == claim.item_id)
            .or_else(|| run.pages.values().find(|page| page.worker_id == claim.worker_id))
            .ok_or_else(|| StoreError::StoreNotFound("auto page".into()))
    }

    pub(crate) fn get_response_cache(&self, cache_key: &str) -> Result<Option<FetchArtifact>> {
        let Some(bytes) = self.response_cache.get(cache_key)
        else {
            return Ok(None);
        };
        let mut artifact: FetchArtifact =
            serde_json::from_slice(bytes).map_err(|error| StoreError::StoreCorrupt(error.to_string()))?;
        artifact.transport_provenance = TransportProvenance::CacheHit;
        Ok(Some(artifact))
    }

    pub(crate) fn put_response_cache(&mut self, artifact: &FetchArtifact) -> Result<()> {
        let bytes = serde_json::to_vec(artifact).map_err(|error| StoreError::StoreCorrupt(error.to_string()))?;
        self.response_cache.insert(artifact.cache_key.clone(), bytes);
        self.persist_snapshot();
        Ok(())
    }

    pub(crate) fn get_robots_cache(&self, site_key: &str) -> Result<Option<Vec<u8>>> {
        let Some(entry) = self.robots_cache.get(site_key)
        else {
            return Ok(None);
        };
        if entry.expires_at_ms.is_some_and(|deadline| deadline <= Self::now_millis()) {
            return Ok(None);
        }
        Ok(Some(entry.body.clone()))
    }

    pub(crate) fn put_robots_cache(&mut self, site_key: &str, body: &[u8], ttl: Duration) -> Result<()> {
        let expires_at_ms = Self::now_millis().saturating_add(ttl.as_millis() as u64);
        self.robots_cache
            .insert(site_key.to_string(), RobotsCacheEntry { body: body.to_vec(), expires_at_ms: Some(expires_at_ms) });
        self.persist_snapshot();
        Ok(())
    }

    /// Read-only view used by the one-way legacy session blob migration.
    ///
    /// Active claims are intentionally dropped: their lease tokens live outside the
    /// blob, so migrated items become reclaimable instead of carrying dead fencing.
    pub(crate) fn legacy_export(&self) -> (Option<String>, Vec<LegacyRun>) {
        let runs = self
            .runs
            .iter()
            .map(|(run_id, run)| LegacyRun {
                run_id: run_id.clone(),
                seed_url: run.seed_url.clone(),
                event_count: run.event_count,
                frontier: run.frontier.values().cloned().collect(),
                pages: run
                    .pages
                    .iter()
                    .map(|(page_id, page)| LegacyPage {
                        page_id: page_id.clone(),
                        item_id: page.item_id.clone(),
                        worker_id: page.worker_id.clone(),
                        fencing_token: page.fencing_token,
                        phase: page.phase,
                        fetch_fixture: page.fetch_fixture.clone(),
                        ir_fixture: page.ir_fixture.clone(),
                    })
                    .collect(),
            })
            .collect();
        (self.active_run_id.clone(), runs)
    }
}
