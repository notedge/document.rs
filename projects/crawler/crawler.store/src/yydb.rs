//! Native YYDB-backed store.
//!
//! Frontier items, claims, page records, report events and run metadata are
//! separate records. Claims use YYDB leases (`claim_lease`) and page commits use
//! a single fenced batch (`commit_with_lease`), so the whole page transaction
//! lands or aborts as one journal snapshot.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crawler_types::{CrawlEvent, FetchArtifact, FrontierItem};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use yydb::{
    Batch, Connection, Error as YydbError, EvictionPolicy, LeaseExpectation, LeaseToken, NamespaceQuota,
    ObjectKind as YydbObjectKind, ObjectRef as YydbObjectRef, ReleaseOutcome as YydbReleaseOutcome,
};

use crate::{
    Store,
    error::{Result, StoreError},
    keyspace,
    session::StoreSession,
    types::{
        CheckpointClaim, CheckpointView, ClaimToken, ObjectKind, ObjectRef, PageAssetBinding, PagePhase, PageTxHandle,
        ReleaseOutcome, RunHandle, RunSpec, StoreConfig, StoreDiagnosticReport, StoreDoctorIssue, StoreDoctorSeverity,
        StoreNamespaceStats,
    },
};

/// Key that held the whole session before native records existed.
///
/// Migration fixture only: [`prepare_store`] imports it once and deletes it.
/// Delete this constant, [`migrate_legacy_session`] and `StoreSession::from_bytes`
/// when the schema bump to 2 lands, because a v2 store can no longer encounter
/// a file without `store/schema_version`.
const LEGACY_SESSION_KEY: &str = "__crawler/store/session";

/// Native store schema version written to `store/schema_version`.
const SCHEMA_VERSION: u32 = 1;

/// Frontier records visited per `scan_prefix` page.
const SCAN_PAGE: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunMeta {
    seed_url: String,
    event_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClaimRecord {
    worker_id: String,
    /// Store-visible monotonic fencing token handed to workers.
    fencing_token: u64,
    /// Upstream lease fencing token, kept because the pinned YYDB revision drops
    /// `__yydb/meta/*` on read and therefore always hands out `1`.
    lease_fencing_token: u64,
    lease_until_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PageRecord {
    item_id: String,
    worker_id: String,
    fencing_token: u64,
    phase: PagePhase,
    fetch_fixture: Option<String>,
    ir_fixture: Option<String>,
    #[serde(default)]
    fetch_ref: Option<ObjectRef>,
    #[serde(default)]
    ir_ref: Option<ObjectRef>,
    #[serde(default)]
    asset_refs: Vec<ObjectRef>,
    #[serde(default)]
    admitted_links: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EventRecord {
    sequence: u64,
    #[serde(default)]
    event: Option<CrawlEvent>,
}

/// Page transaction staged in process memory until one fenced batch commit.
///
/// Nothing is written before commit, so a crash or [`Store::abort_page_tx`] can
/// never leave `ir_validated` visible without `committed`.
#[derive(Debug)]
struct PageTxStaging {
    handle: PageTxHandle,
    claim: ClaimToken,
    lease: LeaseToken,
    fetch: Option<(String, Vec<u8>)>,
    ir: Option<(String, Vec<u8>)>,
    assets: Vec<PageAssetBinding>,
    links: Vec<FrontierItem>,
    events: Vec<CrawlEvent>,
    phase: PagePhase,
}

/// File-backed store using native YYDB records, leases and CAS objects.
pub struct YydbStore {
    path: PathBuf,
    /// File connection, absent while a reopen is between releasing and reacquiring it.
    conn: Option<Connection>,
    active_run_id: Option<String>,
    last_claim: Option<ClaimToken>,
    page_tx: Option<PageTxStaging>,
    claim_lease: Duration,
}

impl YydbStore {
    fn connection(&self) -> Result<&Connection> {
        self.conn.as_ref().ok_or_else(|| StoreError::StoreInvalidState("yydb connection is closed".into()))
    }

    fn read_active_run(conn: &Connection) -> Result<(Option<String>, Option<ClaimToken>)> {
        let Some(bytes) = conn.get(keyspace::active_run()).map_err(map_yydb_error)?
        else {
            return Ok((None, None));
        };
        let run_id =
            String::from_utf8(bytes).map_err(|error| StoreError::StoreCorrupt(format!("active run is not UTF-8: {error}")))?;
        let prefix = keyspace::claim_prefix(&run_id);
        let first = conn.scan_prefix(&prefix, None, 1).map_err(map_yydb_error)?;
        let claim = match first.into_iter().next() {
            Some((key, bytes)) => {
                let record: ClaimRecord = decode(&bytes, "claim")?;
                let item_id = key
                    .strip_prefix(&prefix)
                    .ok_or_else(|| StoreError::StoreCorrupt("claim key does not match its prefix".into()))?
                    .to_owned();
                Some(ClaimToken { item_id, worker_id: record.worker_id, fencing_token: record.fencing_token })
            }
            None => None,
        };
        Ok((Some(run_id), claim))
    }

    fn reload_after_open(&mut self) -> Result<()> {
        let (active_run_id, last_claim) = Self::read_active_run(self.connection()?)?;
        self.active_run_id = active_run_id;
        self.last_claim = last_claim;
        self.page_tx = None;
        Ok(())
    }

    fn read_run_meta(&self, run_id: &str) -> Result<RunMeta> {
        let bytes = self
            .connection()?
            .get(&keyspace::run_meta(run_id))
            .map_err(map_yydb_error)?
            .ok_or_else(|| StoreError::StoreNotFound(format!("run meta {run_id}")))?;
        decode(&bytes, "run meta")
    }

    fn read_claim(&self, claim_key: &str) -> Result<Option<ClaimRecord>> {
        match self.connection()?.get(claim_key).map_err(map_yydb_error)? {
            Some(bytes) => Ok(Some(decode(&bytes, "claim")?)),
            None => Ok(None),
        }
    }

    /// Validate a worker token against the stored claim and rebuild its lease handle.
    ///
    /// The upstream lease record is not trusted for fencing because the pinned
    /// YYDB revision drops `__yydb/meta/*` on read, so the check uses the claim
    /// record this layer wrote at claim time.
    fn active_lease(&self, token: &ClaimToken) -> Result<(String, ClaimRecord, LeaseToken)> {
        let run_id = self.active_run_id.clone().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        let claim_key = keyspace::claim_state(&run_id, &token.item_id);
        let record = self.read_claim(&claim_key)?.ok_or(StoreError::StoreFencingMismatch)?;
        if record.worker_id != token.worker_id || record.fencing_token != token.fencing_token {
            return Err(StoreError::StoreFencingMismatch);
        }
        if record.lease_until_ms <= now_millis() {
            return Err(StoreError::StoreLeaseExpired);
        }
        let lease = LeaseToken {
            key: claim_key.clone(),
            worker_id: record.worker_id.clone(),
            fencing_token: record.lease_fencing_token,
            lease_until: record.lease_until_ms,
        };
        Ok((claim_key, record, lease))
    }

    fn read_page(&self, run_id: &str, page_id: &str) -> Result<Option<PageRecord>> {
        match self.connection()?.get(&keyspace::page_state(run_id, page_id)).map_err(map_yydb_error)? {
            Some(bytes) => Ok(Some(decode(&bytes, "page")?)),
            None => Ok(None),
        }
    }

    /// One pass over page records so claiming does not read the whole state per frontier item.
    fn collect_committed(&self, run_id: &str) -> Result<BTreeSet<String>> {
        let prefix = keyspace::page_prefix(run_id);
        let mut committed = BTreeSet::new();
        let mut after: Option<String> = None;
        loop {
            let page = self.connection()?.scan_prefix(&prefix, after.as_deref(), SCAN_PAGE).map_err(map_yydb_error)?;
            let fetched = page.len();
            for (_, bytes) in &page {
                let record: PageRecord = decode(bytes, "page")?;
                if record.phase == PagePhase::Committed {
                    committed.insert(record.item_id);
                }
            }
            if fetched < SCAN_PAGE {
                return Ok(committed);
            }
            after = page.last().map(|(key, _)| key.clone());
        }
    }

    /// One pass over claim records so claiming is a memory lookup per frontier item.
    fn collect_claims(&self, run_id: &str) -> Result<BTreeMap<String, ClaimRecord>> {
        let prefix = keyspace::claim_prefix(run_id);
        let mut claims = BTreeMap::new();
        let mut after: Option<String> = None;
        loop {
            let page = self.connection()?.scan_prefix(&prefix, after.as_deref(), SCAN_PAGE).map_err(map_yydb_error)?;
            let fetched = page.len();
            for (key, bytes) in &page {
                let item_id = key
                    .strip_prefix(&prefix)
                    .ok_or_else(|| StoreError::StoreCorrupt("claim key does not match its prefix".into()))?
                    .to_owned();
                claims.insert(item_id, decode(bytes, "claim")?);
            }
            if fetched < SCAN_PAGE {
                return Ok(claims);
            }
            after = page.last().map(|(key, _)| key.clone());
        }
    }

    /// Allocate the next run-scoped fencing token through CAS.
    fn allocate_fencing(&self, run_id: &str) -> Result<u64> {
        let conn = self.connection()?;
        let key = keyspace::run_fencing(run_id);
        loop {
            let current = conn.get(&key).map_err(map_yydb_error)?;
            let next =
                current.as_deref().and_then(|bytes| <[u8; 8]>::try_from(bytes).ok()).map(u64::from_le_bytes).unwrap_or(0) + 1;
            let next_bytes = next.to_le_bytes();
            if conn.compare_exchange(&key, current.as_deref(), Some(next_bytes.as_slice())).map_err(map_yydb_error)? {
                return Ok(next);
            }
        }
    }

    /// Claim one frontier item through YYDB leases without touching `last_claim`.
    fn native_claim_next(&self, run_id: &str, host_key: &str, worker_id: &str) -> Result<Option<ClaimToken>> {
        let conn = self.connection()?;
        let now = now_millis();
        let committed = self.collect_committed(run_id)?;
        let claims = self.collect_claims(run_id)?;
        let prefix = keyspace::frontier_prefix(run_id);
        let mut after: Option<String> = None;
        loop {
            let page = conn.scan_prefix(&prefix, after.as_deref(), SCAN_PAGE).map_err(map_yydb_error)?;
            let fetched = page.len();
            for (key, bytes) in &page {
                let item: FrontierItem = decode(bytes, "frontier item")?;
                if item.site_key != host_key {
                    continue;
                }
                let item_id = key
                    .strip_prefix(&prefix)
                    .ok_or_else(|| StoreError::StoreCorrupt("frontier key does not match its prefix".into()))?
                    .to_owned();
                if committed.contains(&item_id) {
                    continue;
                }
                let claim_key = keyspace::claim_state(run_id, &item_id);
                if let Some(existing) = claims.get(&item_id) {
                    if existing.worker_id == worker_id {
                        let active = conn.lease_until_millis(&claim_key).map_err(map_yydb_error)?;
                        if active.is_some_and(|until| until > now) {
                            return Ok(Some(ClaimToken {
                                item_id,
                                worker_id: worker_id.to_owned(),
                                fencing_token: existing.fencing_token,
                            }));
                        }
                    }
                }
                match conn.claim_lease(&claim_key, worker_id, self.claim_lease, LeaseExpectation::AbsentOrExpired) {
                    Ok(lease) => {
                        let fencing = self.allocate_fencing(run_id)?;
                        let record = ClaimRecord {
                            worker_id: worker_id.to_owned(),
                            fencing_token: fencing,
                            lease_fencing_token: lease.fencing_token,
                            lease_until_ms: lease.lease_until,
                        };
                        if let Err(error) = conn.put(&claim_key, encode(&record)?) {
                            let _ = conn.release_lease(&lease, YydbReleaseOutcome::Requeue);
                            return Err(map_yydb_error(error));
                        }
                        return Ok(Some(ClaimToken { item_id, worker_id: worker_id.to_owned(), fencing_token: fencing }));
                    }
                    Err(YydbError::LeaseUnavailable { .. }) => continue,
                    Err(error) => return Err(map_yydb_error(error)),
                }
            }
            if fetched < SCAN_PAGE {
                return Ok(None);
            }
            after = page.last().map(|(key, _)| key.clone());
        }
    }

    fn page_phase_for_auto(&self, run_id: &str) -> Result<PagePhase> {
        if let Some(staging) = &self.page_tx {
            return Ok(staging.phase);
        }
        let claim = self.last_claim.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active claim".into()))?;
        if let Some(page) = self.read_page(run_id, &claim.item_id)? {
            return Ok(page.phase);
        }
        let prefix = keyspace::page_prefix(run_id);
        let pages = self.connection()?.scan_prefix(&prefix, None, SCAN_PAGE).map_err(map_yydb_error)?;
        for (_, bytes) in pages {
            let page: PageRecord = decode(&bytes, "page")?;
            if page.worker_id == claim.worker_id {
                return Ok(page.phase);
            }
        }
        let claim_key = keyspace::claim_state(run_id, &claim.item_id);
        if self.connection()?.get(&claim_key).map_err(map_yydb_error)?.is_some() {
            return Ok(PagePhase::Claimed);
        }
        Err(StoreError::StoreNotFound("auto page".into()))
    }
}

impl Store for YydbStore {
    fn open(config: StoreConfig) -> Result<Self> {
        let path = config.db_path.clone().ok_or_else(|| StoreError::StoreInvalidState("YydbStore requires db_path".into()))?;
        ensure_yydx_layout(&path)?;
        let conn = Connection::open(&path).map_err(map_yydb_error)?;
        prepare_store(&conn)?;
        let mut store = Self {
            path,
            conn: Some(conn),
            active_run_id: None,
            last_claim: None,
            page_tx: None,
            claim_lease: config.claim_lease,
        };
        store.reload_after_open()?;
        Ok(store)
    }

    fn reopen(&mut self) -> Result<()> {
        // YYDB holds its exclusive writer lock for the lifetime of a file connection, so the
        // current handle is released before the replacement is opened. A failed reopen leaves
        // the store without a connection instead of quietly falling back to memory.
        self.conn = None;
        // Uncommitted page staging is process-local and intentionally dropped on restart.
        self.page_tx = None;
        let conn = Connection::open(&self.path).map_err(map_yydb_error)?;
        prepare_store(&conn)?;
        self.conn = Some(conn);
        self.reload_after_open()
    }

    fn begin_run(&mut self, spec: RunSpec) -> Result<RunHandle> {
        let run_id = keyspace::run_id_from_seed(&spec.seed_url);
        let meta = RunMeta { seed_url: spec.seed_url, event_count: 0 };
        let mut batch = Batch::new();
        batch.put(keyspace::run_meta(&run_id), encode(&meta)?);
        batch.put(keyspace::active_run(), run_id.as_bytes());
        batch.commit(self.connection()?).map_err(map_yydb_error)?;
        self.active_run_id = Some(run_id.clone());
        self.last_claim = None;
        self.page_tx = None;
        Ok(RunHandle { run_id })
    }

    fn enqueue(&mut self, item: FrontierItem) -> Result<()> {
        let run_id = self.active_run_id.clone().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        let item_id = keyspace::item_id_from_url(item.request_url.as_str());
        let key = keyspace::frontier_item(&run_id, &item_id);
        self.connection()?.put(key, encode(&item)?).map_err(map_yydb_error)
    }

    fn frontier_item(&self, item_id: &str) -> Result<Option<FrontierItem>> {
        let run_id = self.active_run_id.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        let key = keyspace::frontier_item(run_id, item_id);
        match self.connection()?.get(&key).map_err(map_yydb_error)? {
            Some(bytes) => Ok(Some(decode(&bytes, "frontier item")?)),
            None => Ok(None),
        }
    }

    fn pending_frontier_count(&self) -> Result<u64> {
        let run_id = self.active_run_id.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        let prefix = keyspace::frontier_prefix(run_id);
        let mut total = 0u64;
        let mut after: Option<String> = None;
        loop {
            let page = self.connection()?.scan_prefix(&prefix, after.as_deref(), SCAN_PAGE).map_err(map_yydb_error)?;
            let fetched = page.len();
            total += fetched as u64;
            if fetched < SCAN_PAGE {
                return Ok(total);
            }
            after = page.last().map(|(key, _)| key.clone());
        }
    }

    fn claim_next(&mut self, host_key: &str, worker_id: &str) -> Result<Option<ClaimToken>> {
        let run_id = self.active_run_id.clone().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        let token = self.native_claim_next(&run_id, host_key, worker_id)?;
        if token.is_some() {
            self.last_claim = token.clone();
        }
        Ok(token)
    }

    fn second_worker_claim(&mut self, worker_id: &str) -> Result<()> {
        let run_id = self.active_run_id.clone().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        let prefix = keyspace::claim_prefix(&run_id);
        let first = self.connection()?.scan_prefix(&prefix, None, 1).map_err(map_yydb_error)?;
        let (key, _) = first.into_iter().next().ok_or_else(|| StoreError::StoreInvalidState("no claimed item".into()))?;
        let item_id = key
            .strip_prefix(&prefix)
            .ok_or_else(|| StoreError::StoreCorrupt("claim key does not match its prefix".into()))?
            .to_owned();
        self.last_claim = Some(ClaimToken { item_id, worker_id: worker_id.to_owned(), fencing_token: 0 });
        Ok(())
    }

    fn renew_claim(&mut self, token: &ClaimToken) -> Result<u64> {
        let (claim_key, mut record, lease) = self.active_lease(token)?;
        let now = now_millis();
        let until = now.saturating_add(self.claim_lease.as_millis() as u64);
        self.connection()?.renew_lease(&lease, until).map_err(map_yydb_error)?;
        record.lease_until_ms = until;
        self.connection()?.put(&claim_key, encode(&record)?).map_err(map_yydb_error)?;
        Ok(until)
    }

    fn release_claim(&mut self, token: &ClaimToken, outcome: ReleaseOutcome) -> Result<()> {
        let run_id = self.active_run_id.clone().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        let (claim_key, _record, lease) = self.active_lease(token)?;
        let release = match outcome {
            ReleaseOutcome::Done => YydbReleaseOutcome::Failed { reason: "done".into() },
            ReleaseOutcome::Requeue => YydbReleaseOutcome::Requeue,
            ReleaseOutcome::Pending => YydbReleaseOutcome::Requeue,
        };
        // A page commit already released the upstream lease through
        // `commit_with_lease`, so a later release is a local no-op rather than
        // a fencing failure. The claim record stays the fencing authority.
        match self.connection()?.release_lease(&lease, release) {
            Ok(()) => {}
            Err(YydbError::FencingMismatch { .. } | YydbError::LeaseUnavailable { .. } | YydbError::LeaseExpired { .. }) => {}
            Err(error) => return Err(map_yydb_error(error)),
        }
        let item_key = keyspace::frontier_item(&run_id, &token.item_id);
        let mut batch = Batch::new();
        batch.delete(claim_key);
        match outcome {
            ReleaseOutcome::Done => batch.delete(item_key),
            ReleaseOutcome::Pending => {}
            ReleaseOutcome::Requeue => {
                let bytes = self
                    .connection()?
                    .get(&item_key)
                    .map_err(map_yydb_error)?
                    .ok_or_else(|| StoreError::StoreNotFound(token.item_id.clone()))?;
                let mut item: FrontierItem = decode(&bytes, "frontier item")?;
                item.attempts = item.attempts.saturating_add(1);
                batch.put(item_key, encode(&item)?);
            }
        }
        batch.commit(self.connection()?).map_err(map_yydb_error)?;
        if self.last_claim.as_ref() == Some(token) {
            self.last_claim = None;
        }
        Ok(())
    }

    fn begin_page_tx(&mut self, worker_id: &str) -> Result<PageTxHandle> {
        let claim = self.last_claim.clone().ok_or_else(|| StoreError::StoreInvalidState("no claim token".into()))?;
        if claim.worker_id != worker_id {
            return Err(StoreError::StoreFencingMismatch);
        }
        let run_id = self.active_run_id.clone().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        let claim_key = keyspace::claim_state(&run_id, &claim.item_id);
        let record = self.read_claim(&claim_key)?.ok_or(StoreError::StoreFencingMismatch)?;
        if record.worker_id != worker_id || record.fencing_token != claim.fencing_token {
            return Err(StoreError::StoreFencingMismatch);
        }
        let lease_until = self.connection()?.lease_until_millis(&claim_key).map_err(map_yydb_error)?;
        match lease_until {
            Some(until) if until > now_millis() => {
                let lease = LeaseToken {
                    key: claim_key,
                    worker_id: worker_id.to_owned(),
                    fencing_token: record.lease_fencing_token,
                    lease_until: until,
                };
                let handle = PageTxHandle { page_id: claim.item_id.clone() };
                self.page_tx = Some(PageTxStaging {
                    handle: handle.clone(),
                    claim,
                    lease,
                    fetch: None,
                    ir: None,
                    assets: Vec::new(),
                    links: Vec::new(),
                    events: Vec::new(),
                    phase: PagePhase::Claimed,
                });
                Ok(handle)
            }
            _ => Err(StoreError::StoreLeaseExpired),
        }
    }

    fn page_tx_put_fetch(&mut self, fixture: &str, bytes: &[u8]) -> Result<()> {
        let staging = self.page_tx.as_mut().ok_or_else(|| StoreError::StoreInvalidState("no page tx".into()))?;
        staging.fetch = Some((fixture.to_owned(), bytes.to_vec()));
        staging.phase = PagePhase::Fetched;
        Ok(())
    }

    fn page_tx_put_ir(&mut self, fixture: &str, bytes: &[u8]) -> Result<()> {
        let staging = self.page_tx.as_mut().ok_or_else(|| StoreError::StoreInvalidState("no page tx".into()))?;
        staging.ir = Some((fixture.to_owned(), bytes.to_vec()));
        staging.phase = PagePhase::IrValidated;
        Ok(())
    }

    fn page_tx_bind_asset(&mut self, binding: &PageAssetBinding) -> Result<()> {
        let staging = self.page_tx.as_mut().ok_or_else(|| StoreError::StoreInvalidState("no page tx".into()))?;
        staging.assets.push(binding.clone());
        staging.phase = PagePhase::AssetsBound;
        Ok(())
    }

    fn page_tx_admit_links(&mut self, items: Vec<FrontierItem>) -> Result<()> {
        let staging = self.page_tx.as_mut().ok_or_else(|| StoreError::StoreInvalidState("no page tx".into()))?;
        staging.links.extend(items);
        Ok(())
    }

    fn page_tx_put_event(&mut self, event: &CrawlEvent) -> Result<()> {
        let staging = self.page_tx.as_mut().ok_or_else(|| StoreError::StoreInvalidState("no page tx".into()))?;
        staging.events.push(event.clone());
        Ok(())
    }

    fn commit_page_tx(&mut self) -> Result<()> {
        let staging = self.page_tx.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no page tx".into()))?;
        let run_id = self.active_run_id.clone().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        let conn = self.connection()?;
        let mut batch = Batch::new();
        let fetch = match &staging.fetch {
            Some((_, bytes)) => Some(stage_object(conn, &mut batch, ObjectKind::ResponseBody, None, bytes)?),
            None => None,
        };
        let ir = match &staging.ir {
            Some((_, bytes)) => Some(stage_object(conn, &mut batch, ObjectKind::IrPackage, None, bytes)?),
            None => None,
        };
        let mut assets = Vec::with_capacity(staging.assets.len());
        for binding in &staging.assets {
            let object = stage_object(conn, &mut batch, ObjectKind::AssetBytes, binding.media_type.as_deref(), &binding.bytes)?;
            assets.push((binding, object));
        }
        let meta = self.read_run_meta(&run_id)?;
        let base = meta.event_count;
        let commit_sequence = base + staging.events.len().max(1) as u64;
        let record = PageRecord {
            item_id: staging.claim.item_id.clone(),
            worker_id: staging.claim.worker_id.clone(),
            fencing_token: staging.claim.fencing_token,
            phase: PagePhase::Committed,
            fetch_fixture: staging.fetch.as_ref().map(|(fixture, _)| fixture.clone()),
            ir_fixture: staging.ir.as_ref().map(|(fixture, _)| fixture.clone()),
            fetch_ref: fetch.as_ref().map(|(reference, _)| reference.clone()),
            ir_ref: ir.as_ref().map(|(reference, _)| reference.clone()),
            asset_refs: assets.iter().map(|(_, (reference, _))| reference.clone()).collect(),
            admitted_links: staging.links.iter().map(|item| keyspace::item_id_from_url(item.request_url.as_str())).collect(),
        };
        batch.put(keyspace::page_state(&run_id, &staging.handle.page_id), encode(&record)?);
        attach_object(
            &mut batch,
            keyspace::page_object(&run_id, &staging.handle.page_id, "fetch"),
            fetch.as_ref().map(|(_, object)| object),
        );
        attach_object(
            &mut batch,
            keyspace::page_object(&run_id, &staging.handle.page_id, "ir"),
            ir.as_ref().map(|(_, object)| object),
        );
        for (binding, (_, object)) in &assets {
            let slot = format!("asset-{}", binding.asset_id);
            attach_object(&mut batch, keyspace::page_object(&run_id, &staging.handle.page_id, &slot), Some(object));
        }
        // Link admission lands in the same fenced batch as the page record, so a
        // page can never be visible while its discovered links are missing.
        for item in &staging.links {
            let item_id = keyspace::item_id_from_url(item.request_url.as_str());
            batch.put(keyspace::frontier_item(&run_id, &item_id), encode(item)?);
        }
        // Report events staged by the page commit share the same batch.
        if staging.events.is_empty() {
            batch.put(
                keyspace::report_event(&run_id, commit_sequence),
                encode(&EventRecord { sequence: commit_sequence, event: None })?,
            );
        }
        else {
            for (offset, event) in staging.events.iter().enumerate() {
                let sequence = base + offset as u64 + 1;
                batch.put(
                    keyspace::report_event(&run_id, sequence),
                    encode(&EventRecord { sequence, event: Some(event.clone()) })?,
                );
            }
        }
        batch.put(keyspace::run_meta(&run_id), encode(&RunMeta { seed_url: meta.seed_url, event_count: commit_sequence })?);
        conn.commit_with_lease(&staging.lease, batch).map_err(map_yydb_error)?;
        self.page_tx = None;
        Ok(())
    }

    fn abort_page_tx(&mut self) -> Result<()> {
        if self.page_tx.take().is_none() {
            return Err(StoreError::StoreInvalidState("no page tx".into()));
        }
        Ok(())
    }

    fn event_count(&self) -> Result<u64> {
        let run_id = self.active_run_id.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        Ok(self.read_run_meta(run_id)?.event_count)
    }

    fn save_checkpoint_view(&mut self) -> Result<u64> {
        let run_id = self.active_run_id.clone().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        let view = self.build_checkpoint_view(&run_id)?;
        let key = keyspace::checkpoint_entry(&run_id, view.last_commit_sequence);
        self.connection()?.put(&key, encode(&view)?).map_err(map_yydb_error)?;
        Ok(view.last_commit_sequence)
    }

    fn checkpoint_view(&self) -> Result<Option<CheckpointView>> {
        let run_id = self.active_run_id.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        self.latest_checkpoint_view(run_id)
    }

    fn checkpoint_view_json(&self) -> Result<Option<String>> {
        self.checkpoint_view()?
            .map(|view| serde_json::to_string(&view).map_err(|error| StoreError::StoreCorrupt(error.to_string())))
            .transpose()
    }

    fn page_phase(&self, page_id: &str) -> Result<PagePhase> {
        let run_id = self.active_run_id.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        if page_id == "auto" {
            return self.page_phase_for_auto(run_id);
        }
        if let Some(staging) = &self.page_tx {
            if staging.handle.page_id == page_id {
                return Ok(staging.phase);
            }
        }
        self.read_page(run_id, page_id)?.map(|page| page.phase).ok_or_else(|| StoreError::StoreNotFound(page_id.to_owned()))
    }

    fn page_asset_refs(&self, page_id: &str) -> Result<Vec<String>> {
        let run_id = self.active_run_id.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        self.read_page(run_id, page_id)?
            .map(|page| page.asset_refs.iter().map(|reference| reference.fingerprint.clone()).collect())
            .ok_or_else(|| StoreError::StoreNotFound(page_id.to_owned()))
    }

    fn page_object_refs(&self, page_id: &str) -> Result<Vec<ObjectRef>> {
        let run_id = self.active_run_id.as_ref().ok_or_else(|| StoreError::StoreInvalidState("no active run".into()))?;
        let page = self.read_page(run_id, page_id)?.ok_or_else(|| StoreError::StoreNotFound(page_id.to_owned()))?;
        let mut refs = Vec::with_capacity(2 + page.asset_refs.len());
        refs.extend(page.fetch_ref);
        refs.extend(page.ir_ref);
        refs.extend(page.asset_refs);
        Ok(refs)
    }

    fn put_object(&mut self, kind: ObjectKind, media_type: Option<&str>, bytes: &[u8]) -> Result<ObjectRef> {
        let conn = self.connection()?;
        let mut batch = Batch::new();
        let (reference, _) = stage_object(conn, &mut batch, kind, media_type, bytes)?;
        batch.commit(conn).map_err(map_yydb_error)?;
        Ok(reference)
    }

    fn get_object(&self, fingerprint: &str) -> Result<Option<Vec<u8>>> {
        let Some(reference) = self.object_ref(fingerprint)?
        else {
            return Ok(None);
        };
        let object = YydbObjectRef {
            algo: yydb::HashAlgo::Blake3,
            hash: decode_hash_hex(&reference.fingerprint)?,
            size: reference.size,
            kind: YydbObjectKind::Blob,
        };
        let bytes = self.connection()?.get_object(&object).map_err(map_yydb_error)?;
        Ok(Some(bytes.to_vec()))
    }

    fn object_ref(&self, fingerprint: &str) -> Result<Option<ObjectRef>> {
        match self.connection()?.get(&keyspace::object_meta(fingerprint)).map_err(map_yydb_error)? {
            Some(bytes) => Ok(Some(decode(&bytes, "object metadata")?)),
            None => Ok(None),
        }
    }

    fn put_orphan_object(&mut self, bytes: &[u8]) -> Result<()> {
        self.connection()?.put_chunk(YydbObjectKind::Blob, bytes).map_err(map_yydb_error)?;
        Ok(())
    }

    fn doctor(&self) -> Result<StoreDiagnosticReport> {
        let report = self.connection()?.doctor().map_err(map_yydb_error)?;
        let mut issues = report
            .issues
            .into_iter()
            .map(|issue| StoreDoctorIssue {
                severity: match issue.severity {
                    yydb::DoctorSeverity::Warn => StoreDoctorSeverity::Warn,
                    yydb::DoctorSeverity::Error => StoreDoctorSeverity::Error,
                },
                code: issue.code,
                message: issue.message,
            })
            .collect::<Vec<_>>();
        if report.orphan_object_count > 0 {
            issues.push(StoreDoctorIssue {
                severity: StoreDoctorSeverity::Warn,
                code: "store.doctor.orphan_object".into(),
                message: format!("{} orphan objects are not referenced by committed metadata", report.orphan_object_count),
            });
        }
        let namespaces = report
            .namespaces
            .into_iter()
            .map(|namespace| StoreNamespaceStats {
                prefix: namespace.prefix,
                record_count: namespace.record_count,
                bytes_used: namespace.bytes_used,
                quota_max_bytes: namespace.quota_max_bytes,
                ttl_expired_pending: namespace.ttl_expired_pending,
            })
            .collect();
        // Upstream checkpoint lag resets on every read because `__yydb/meta/*`
        // is dropped by the pinned format, so the store keeps its own
        // `run/{run_id}/checkpoint/{sequence}` records as the source of truth.
        let checkpoint_lag_records = match self.active_run_id.as_ref() {
            Some(run_id) => {
                let events = self.read_run_meta(run_id)?.event_count;
                events.saturating_sub(self.latest_checkpoint_sequence(run_id)?)
            }
            None => 0,
        };
        if checkpoint_lag_records > 0 {
            issues.push(StoreDoctorIssue {
                severity: StoreDoctorSeverity::Warn,
                code: "store.doctor.checkpoint_stale".into(),
                message: format!("checkpoint lags commit by {checkpoint_lag_records} records"),
            });
        }
        Ok(StoreDiagnosticReport {
            orphan_object_count: report.orphan_object_count,
            checkpoint_lag_records,
            namespaces,
            issues,
        })
    }

    fn get_response_cache(&self, cache_key: &str) -> Result<Option<FetchArtifact>> {
        let key = keyspace::response_cache(cache_key);
        let Some(bytes) = self.connection()?.get(&key).map_err(map_yydb_error)?
        else {
            return Ok(None);
        };
        let mut artifact: FetchArtifact =
            serde_json::from_slice(&bytes).map_err(|error| StoreError::StoreCorrupt(error.to_string()))?;
        artifact.transport_provenance = crawler_types::TransportProvenance::CacheHit;
        Ok(Some(artifact))
    }

    fn put_response_cache(&mut self, artifact: &FetchArtifact) -> Result<()> {
        let key = keyspace::response_cache(&artifact.cache_key);
        let bytes = serde_json::to_vec(artifact).map_err(|error| StoreError::StoreCorrupt(error.to_string()))?;
        self.connection()?.put(key, bytes).map_err(map_yydb_error)?;
        Ok(())
    }

    fn get_robots_cache(&self, site_key: &str) -> Result<Option<Vec<u8>>> {
        let key = keyspace::robots_cache(site_key);
        self.connection()?.get(&key).map_err(map_yydb_error)
    }

    fn put_robots_cache(&mut self, site_key: &str, body: &[u8], ttl: std::time::Duration) -> Result<()> {
        let key = keyspace::robots_cache(site_key);
        self.connection()?.put_with_ttl(key, body, ttl).map_err(map_yydb_error)?;
        Ok(())
    }
}

impl YydbStore {
    /// Database path for diagnostics.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Number of CAS objects currently present in this connection.
    pub fn object_count(&self) -> Result<u64> {
        let objects = self.connection()?.objects().list_objects().map_err(map_yydb_error)?;
        Ok(objects.len() as u64)
    }

    /// Delete unreferenced CAS objects and report how many were removed.
    pub fn reclaim_orphans(&mut self) -> Result<u64> {
        let conn = self.connection()?;
        let orphans = conn.scan_orphans("").map_err(map_yydb_error)?;
        let report = conn.reclaim_orphans(&orphans).map_err(map_yydb_error)?;
        Ok(report.reclaimed_objects)
    }

    /// Bound a namespace such as `cache/response/` by bytes and records.
    ///
    /// YYDB evicts least-recently-used records to stay inside the quota, so a
    /// cache write can silently displace an older entry instead of growing.
    pub fn set_namespace_quota(&self, namespace: &str, max_bytes: u64, max_records: u64) -> Result<()> {
        let quota = NamespaceQuota { max_bytes, max_records, eviction_policy: EvictionPolicy::Lru };
        self.connection()?.set_namespace_quota(namespace, quota).map_err(map_yydb_error)
    }

    /// Fold committed records into the main file and reset checkpoint lag.
    pub fn checkpoint(&self) -> Result<()> {
        self.connection()?.checkpoint().map_err(map_yydb_error)
    }

    /// Derive a checkpoint view from the native records at this instant.
    ///
    /// The view is a projection, not a fact source: pages, frontier records,
    /// claims and the event counter stay authoritative.
    fn build_checkpoint_view(&self, run_id: &str) -> Result<CheckpointView> {
        let meta = self.read_run_meta(run_id)?;
        Ok(CheckpointView {
            run_id: run_id.to_owned(),
            seed_url: meta.seed_url,
            last_commit_sequence: meta.event_count,
            committed_pages: self.collect_committed(run_id)?.len() as u64,
            pending_frontier: self.pending_frontier_count()?,
            open_claims: self
                .collect_claims(run_id)?
                .into_iter()
                .map(|(item_id, claim)| CheckpointClaim {
                    item_id,
                    worker_id: claim.worker_id,
                    fencing_token: claim.fencing_token,
                    lease_until_ms: claim.lease_until_ms,
                })
                .collect(),
        })
    }

    /// Latest exported view, chosen by `last_commit_sequence`.
    fn latest_checkpoint_view(&self, run_id: &str) -> Result<Option<CheckpointView>> {
        let conn = self.connection()?;
        let prefix = keyspace::checkpoint_prefix(run_id);
        let mut after: Option<String> = None;
        let mut latest: Option<CheckpointView> = None;
        loop {
            let page = conn.scan_prefix(&prefix, after.as_deref(), SCAN_PAGE).map_err(map_yydb_error)?;
            let fetched = page.len();
            for (_, bytes) in &page {
                let view: CheckpointView = decode(bytes, "checkpoint")?;
                if latest.as_ref().map_or(true, |current| current.last_commit_sequence < view.last_commit_sequence) {
                    latest = Some(view);
                }
            }
            if fetched < SCAN_PAGE {
                return Ok(latest);
            }
            after = page.last().map(|(key, _)| key.clone());
        }
    }

    fn latest_checkpoint_sequence(&self, run_id: &str) -> Result<u64> {
        Ok(self.latest_checkpoint_view(run_id)?.map_or(0, |view| view.last_commit_sequence))
    }
}

/// Write one payload into the CAS and stage its metadata record when it is new.
///
/// The CAS write itself already deduplicates on the blake3 digest, so the
/// metadata record and its attachment are only queued for a fingerprint the
/// store has not seen before.
fn stage_object(
    conn: &Connection,
    batch: &mut Batch,
    kind: ObjectKind,
    media_type: Option<&str>,
    bytes: &[u8],
) -> Result<(ObjectRef, YydbObjectRef)> {
    let object = conn.put_chunk(YydbObjectKind::Blob, bytes).map_err(map_yydb_error)?;
    let fingerprint = object.hash_hex();
    let meta_key = keyspace::object_meta(&fingerprint);
    let reference = match conn.get(&meta_key).map_err(map_yydb_error)? {
        Some(bytes) => decode(&bytes, "object metadata")?,
        None => {
            let reference = ObjectRef {
                fingerprint: fingerprint.clone(),
                size: object.size,
                media_type: media_type.map(str::to_owned),
                kind,
            };
            batch.put(meta_key, encode(&reference)?);
            batch.attach_object(&object, keyspace::object_blob(&fingerprint));
            reference
        }
    };
    Ok((reference, object))
}

/// Decode a 32-byte blake3 hex digest back into raw bytes.
fn decode_hash_hex(fingerprint: &str) -> Result<[u8; 32]> {
    if fingerprint.len() != 64 {
        return Err(StoreError::StoreCorrupt(format!("object fingerprint is not 64 hex chars: {fingerprint}")));
    }
    let mut hash = [0u8; 32];
    for (index, chunk) in fingerprint.as_bytes().chunks(2).enumerate() {
        let text = std::str::from_utf8(chunk).map_err(|error| StoreError::StoreCorrupt(error.to_string()))?;
        hash[index] =
            u8::from_str_radix(text, 16).map_err(|error| StoreError::StoreCorrupt(format!("object fingerprint: {error}")))?;
    }
    Ok(hash)
}

fn attach_object(batch: &mut Batch, key: String, object: Option<&YydbObjectRef>) {
    if let Some(object) = object {
        batch.attach_object(object, key);
    }
}

/// Object payloads persist only in the `.yydx` layout.
///
/// A `.yydb` single-file store keeps response bodies in a process-local CAS that
/// is gone after the process exits, so opening one for writes would look
/// healthy while silently losing every object written by a previous run.
/// Reject it with an explicit migration path instead of degrading quietly.
fn ensure_yydx_layout(path: &Path) -> Result<()> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("yydx") => Ok(()),
        Some("yydb") => {
            let stem = path.file_stem().and_then(|name| name.to_str()).unwrap_or("crawler");
            let target = path.with_file_name(format!("{stem}.yydx"));
            Err(StoreError::StoreInvalidState(format!(
                "legacy single-file store {} is read-only and cannot be opened for writes. Object payloads in a `.yydb` file \
                 were process-local and cannot be recovered. Export records with the previous build, then run against {}",
                path.display(),
                target.display()
            )))
        }
        _ => Err(StoreError::StoreInvalidState(format!(
            "crawler store path must end with `.yydx` so object payloads get a persistent `<stem>-objects/` root: {}",
            path.display()
        ))),
    }
}

/// Ensure the native schema marker exists and import the legacy session blob once.
fn prepare_store(conn: &Connection) -> Result<()> {
    match conn.get(keyspace::schema_version()).map_err(map_yydb_error)? {
        Some(bytes) => {
            let found = bytes
                .get(..4)
                .and_then(|head| <[u8; 4]>::try_from(head).ok())
                .map(u32::from_le_bytes)
                .ok_or_else(|| StoreError::StoreCorrupt("store schema version record is malformed".into()))?;
            if found > SCHEMA_VERSION {
                return Err(StoreError::StoreCorrupt(format!(
                    "store schema version {found} is newer than supported {SCHEMA_VERSION}"
                )));
            }
            if found < SCHEMA_VERSION {
                migrate_legacy_session(conn)?;
            }
            Ok(())
        }
        None => migrate_legacy_session(conn),
    }
}

/// One-way import of the pre-native session blob into separate records.
///
/// Claims are not migrated: their lease tokens live outside the blob, so the
/// frontier items become reclaimable instead of carrying dead fencing tokens.
/// Non-committed pages are dropped so a half page transaction stays invisible.
fn migrate_legacy_session(conn: &Connection) -> Result<()> {
    let mut changes: Vec<(String, Option<Vec<u8>>)> = Vec::new();
    if let Some(bytes) = conn.get(LEGACY_SESSION_KEY).map_err(map_yydb_error)? {
        let session = StoreSession::from_bytes(&bytes)?;
        let (active_run_id, runs) = session.legacy_export();
        for run in runs {
            let meta = RunMeta { seed_url: run.seed_url, event_count: run.event_count };
            changes.push((keyspace::run_meta(&run.run_id), Some(encode(&meta)?)));
            for item in run.frontier {
                let item_id = keyspace::item_id_from_url(item.request_url.as_str());
                changes.push((keyspace::frontier_item(&run.run_id, &item_id), Some(encode(&item)?)));
            }
            for page in run.pages {
                if page.phase != PagePhase::Committed {
                    continue;
                }
                let record = PageRecord {
                    item_id: page.item_id,
                    worker_id: page.worker_id,
                    fencing_token: page.fencing_token,
                    phase: page.phase,
                    fetch_fixture: page.fetch_fixture,
                    ir_fixture: page.ir_fixture,
                    fetch_ref: None,
                    ir_ref: None,
                    asset_refs: Vec::new(),
                    admitted_links: Vec::new(),
                };
                changes.push((keyspace::page_state(&run.run_id, &page.page_id), Some(encode(&record)?)));
            }
        }
        if let Some(run_id) = active_run_id {
            changes.push((keyspace::active_run().to_owned(), Some(run_id.into_bytes())));
        }
        changes.push((LEGACY_SESSION_KEY.to_owned(), None));
    }
    changes.push((keyspace::schema_version().to_owned(), Some(SCHEMA_VERSION.to_le_bytes().to_vec())));
    conn.write_batch(&changes).map_err(map_yydb_error)
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|error| StoreError::StoreCorrupt(error.to_string()))
}

fn decode<T: DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|error| StoreError::StoreCorrupt(format!("{what}: {error}")))
}

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn map_yydb_error(error: YydbError) -> StoreError {
    match error {
        YydbError::LeaseUnavailable { .. } => StoreError::StoreConflict,
        YydbError::LeaseExpired { .. } => StoreError::StoreLeaseExpired,
        YydbError::FencingMismatch { .. } => StoreError::StoreFencingMismatch,
        YydbError::CasConflict { .. } => StoreError::StoreConflict,
        YydbError::QuotaExceeded { .. } => StoreError::StoreQuotaExceeded,
        YydbError::ObjectNotFound { hash_hex } => StoreError::StoreNotFound(hash_hex),
        other => StoreError::StoreCorrupt(other.to_string()),
    }
}
