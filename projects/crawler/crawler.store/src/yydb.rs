use std::path::{Path, PathBuf};

use yydb::{Connection, ObjectKind};

use crate::{
    Store,
    error::{Result, StoreError},
    keyspace,
    session::StoreSession,
    types::{
        ClaimToken, PagePhase, PageTxHandle, RunHandle, RunSpec, StoreConfig, StoreDiagnosticReport, StoreDoctorIssue,
        StoreDoctorSeverity,
    },
};
use crawler_types::{FetchArtifact, FrontierItem};

const SESSION_KEY: &str = "__crawler/store/session";

/// File-backed store using embedded `yydb::Connection`.
pub struct YydbStore {
    path: PathBuf,
    /// File connection, absent while a reopen is between releasing and reacquiring it.
    conn: Option<Connection>,
    session: StoreSession,
}

impl YydbStore {
    fn connection(&self) -> Result<&Connection> {
        self.conn.as_ref().ok_or_else(|| StoreError::StoreInvalidState("yydb connection is closed".into()))
    }

    fn load_session(conn: &Connection) -> Result<Option<StoreSession>> {
        match conn.get(SESSION_KEY).map_err(map_yydb_error)? {
            Some(bytes) => Ok(Some(StoreSession::from_bytes(&bytes)?)),
            None => Ok(None),
        }
    }

    fn persist(&mut self) -> Result<()> {
        self.connection()?.put(SESSION_KEY, &self.session.to_bytes()?).map_err(map_yydb_error)
    }
}

impl Store for YydbStore {
    fn open(config: StoreConfig) -> Result<Self> {
        let path = config.db_path.clone().ok_or_else(|| StoreError::StoreInvalidState("YydbStore requires db_path".into()))?;
        let conn = Connection::open(&path).map_err(map_yydb_error)?;
        let session = Self::load_session(&conn)?.unwrap_or_else(StoreSession::new);
        Ok(Self { path, conn: Some(conn), session })
    }

    fn reopen(&mut self) -> Result<()> {
        // YYDB holds its exclusive writer lock for the lifetime of a file connection, so the
        // current handle is released before the replacement is opened. A failed reopen leaves
        // the store without a connection instead of quietly falling back to memory.
        self.conn = None;
        let conn = Connection::open(&self.path).map_err(map_yydb_error)?;
        self.conn = Some(conn);
        if let Some(bytes) = self.connection()?.get(SESSION_KEY).map_err(map_yydb_error)? {
            self.session = StoreSession::from_bytes(&bytes)?;
        }
        self.session.reopen()?;
        self.persist()?;
        Ok(())
    }

    fn begin_run(&mut self, spec: RunSpec) -> Result<RunHandle> {
        let handle = self.session.begin_run(spec)?;
        self.persist()?;
        Ok(handle)
    }

    fn enqueue(&mut self, item: FrontierItem) -> Result<()> {
        self.session.enqueue(item)?;
        self.persist()?;
        Ok(())
    }

    fn claim_next(&mut self, host_key: &str, worker_id: &str) -> Result<Option<ClaimToken>> {
        let token = self.session.claim_next(host_key, worker_id)?;
        self.persist()?;
        Ok(token)
    }

    fn second_worker_claim(&mut self, worker_id: &str) -> Result<()> {
        self.session.second_worker_claim(worker_id)
    }

    fn begin_page_tx(&mut self, worker_id: &str) -> Result<PageTxHandle> {
        let handle = self.session.begin_page_tx(worker_id)?;
        self.persist()?;
        Ok(handle)
    }

    fn page_tx_put_fetch(&mut self, fixture: &str, bytes: &[u8]) -> Result<()> {
        self.session.page_tx_put_fetch(fixture, bytes)?;
        self.persist()?;
        Ok(())
    }

    fn page_tx_put_ir(&mut self, fixture: &str, bytes: &[u8]) -> Result<()> {
        self.session.page_tx_put_ir(fixture, bytes)?;
        self.persist()?;
        Ok(())
    }

    fn commit_page_tx(&mut self) -> Result<()> {
        self.session.commit_page_tx()?;
        self.persist()?;
        Ok(())
    }

    fn abort_page_tx(&mut self) -> Result<()> {
        self.session.abort_page_tx()?;
        self.persist()?;
        Ok(())
    }

    fn event_count(&self) -> Result<u64> {
        self.session.event_count()
    }

    fn page_phase(&self, page_id: &str) -> Result<PagePhase> {
        self.session.page_phase(page_id)
    }

    fn put_orphan_object(&mut self, bytes: &[u8]) -> Result<()> {
        self.connection()?.put_chunk(ObjectKind::Blob, bytes).map_err(map_yydb_error)?;
        Ok(())
    }

    fn doctor(&self) -> Result<StoreDiagnosticReport> {
        let report = self.connection()?.doctor().map_err(map_yydb_error)?;
        let mut issues = Vec::new();
        if report.orphan_object_count > 0 {
            issues.push(StoreDoctorIssue {
                severity: StoreDoctorSeverity::Warn,
                code: "store.doctor.orphan_object".into(),
                message: format!("{} orphan objects are not referenced by committed metadata", report.orphan_object_count),
            });
        }
        Ok(StoreDiagnosticReport { orphan_object_count: report.orphan_object_count, issues })
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
}

fn map_yydb_error(error: yydb::Error) -> StoreError {
    StoreError::StoreCorrupt(error.to_string())
}
