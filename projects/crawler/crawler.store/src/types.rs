use std::time::Duration;

use crawler_types::{CrawlRequest, PageChallengeState};
use serde::{Deserialize, Serialize};

/// Default lease granted to a frontier claim.
pub const DEFAULT_CLAIM_LEASE: Duration = Duration::from_secs(30);

/// Store open parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreConfig {
    /// File path for file-backed backends such as `YydbStore`.
    ///
    /// `YydbStore` requires a `*.yydx` path: object payloads live in the sibling
    /// `<stem>-objects/` CAS root, which a single-file `.yydb` store cannot keep
    /// across restarts.
    pub db_path: Option<std::path::PathBuf>,
    /// Lease duration granted when a worker claims a frontier item.
    pub claim_lease: Duration,
}

impl StoreConfig {
    /// Memory-oriented defaults.
    pub fn memory() -> Self {
        Self { db_path: None, claim_lease: DEFAULT_CLAIM_LEASE }
    }

    /// YYDB `.yydx` path for integration tests and production.
    pub fn yydb(path: impl Into<std::path::PathBuf>) -> Self {
        Self { db_path: Some(path.into()), claim_lease: DEFAULT_CLAIM_LEASE }
    }

    /// Override the claim lease, mainly for lease expiry contract cases.
    pub fn with_claim_lease(mut self, claim_lease: Duration) -> Self {
        self.claim_lease = claim_lease;
        self
    }
}

/// Parameters for starting a crawl run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSpec {
    /// Seed URL for the run.
    pub seed_url: String,
    /// Original crawl request, persisted so a resume can rebuild depth,
    /// budget, scope and politeness without a JSON checkpoint.
    #[serde(default)]
    pub request: Option<CrawlRequest>,
    /// Strategy fingerprint frozen at run start.
    ///
    /// A resume with a different fingerprint is rejected instead of silently
    /// continuing under changed crawl semantics.
    #[serde(default)]
    pub strategy_fingerprint: Option<String>,
}

impl RunSpec {
    /// Start a run from a seed URL without persisting a request.
    pub fn new(seed_url: impl Into<String>) -> Self {
        Self { seed_url: seed_url.into(), request: None, strategy_fingerprint: None }
    }
}

/// State reconstructed from store records when a run resumes.
///
/// The view is never the source of truth: committed pages, frontier records
/// and claims stay authoritative and are re-read from the store on resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeState {
    /// Run being resumed.
    pub run_id: String,
    /// Seed URL recorded at run start.
    pub seed_url: String,
    /// Original crawl request when the run persisted one.
    #[serde(default)]
    pub request: Option<CrawlRequest>,
    /// Strategy fingerprint recorded at run start.
    #[serde(default)]
    pub strategy_fingerprint: Option<String>,
    /// Last committed report event sequence.
    pub last_commit_sequence: u64,
    /// Pages already committed in this run.
    pub committed_pages: u64,
    /// Frontier records still queued after expired claims were reclaimed.
    pub pending_frontier: u64,
    /// Host partitions with queued frontier records, used to rebuild the
    /// in-process host scheduler on resume.
    #[serde(default)]
    pub pending_hosts: Vec<String>,
    /// Claims that stay open after resume.
    pub open_claims: Vec<CheckpointClaim>,
}

/// Active run identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunHandle {
    /// Stable run identifier.
    pub run_id: String,
}

/// Lease token issued when a worker claims a frontier item.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClaimToken {
    /// Frontier item identifier.
    pub item_id: String,
    /// Worker that holds the claim.
    pub worker_id: String,
    /// Monotonic fencing token.
    pub fencing_token: u64,
}

/// What happens to a claimed frontier item when its worker releases the lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseOutcome {
    /// The item is finished and leaves the frontier.
    Done,
    /// The attempt failed and the item returns to pending with a higher attempt count.
    Requeue,
    /// The attempt did not run, so the item returns to pending unchanged.
    Pending,
}

/// In-flight page transaction handle.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageTxHandle {
    /// Page record identifier.
    pub page_id: String,
}

/// One open claim recorded in a checkpoint view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointClaim {
    /// Frontier item held by the claim.
    pub item_id: String,
    /// Worker that holds the claim.
    pub worker_id: String,
    /// Store-visible fencing token.
    pub fencing_token: u64,
    /// Lease expiry in epoch milliseconds.
    pub lease_until_ms: u64,
}

/// Derived checkpoint view exported from the store records.
///
/// A checkpoint is never the source of truth: the view is recomputed from
/// frontier, claim and page records, and JSON is only an export artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointView {
    /// Active run identity.
    #[serde(default)]
    pub run_id: String,
    /// Seed URL of the run.
    #[serde(default)]
    pub seed_url: String,
    /// Last committed report event sequence.
    #[serde(default, alias = "sequence")]
    pub last_commit_sequence: u64,
    /// Pages already committed in this run.
    #[serde(default)]
    pub committed_pages: u64,
    /// Frontier records still queued or claimed.
    #[serde(default)]
    pub pending_frontier: u64,
    /// Claims that are still open at view time.
    #[serde(default)]
    pub open_claims: Vec<CheckpointClaim>,
}

/// Page transaction lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PagePhase {
    /// Item claimed but no page transaction started.
    Claimed,
    /// Fetch artifact stored.
    Fetched,
    /// IR package stored.
    IrValidated,
    /// Document assets bound to the page transaction.
    AssetsBound,
    /// Page transaction committed.
    Committed,
}

impl PagePhase {
    /// Stable contract string used by store contract assertions.
    pub fn as_contract_str(self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::Fetched => "fetched",
            Self::IrValidated => "ir_validated",
            Self::AssetsBound => "assets_bound",
            Self::Committed => "committed",
        }
    }
}

/// Semantic kind of one content-addressed object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    /// Raw HTTP response body.
    ResponseBody,
    /// Robots document bytes.
    RobotsBody,
    /// Browser archive such as MHTML, HAR or a screenshot bundle.
    BrowserArchive,
    /// Downloaded asset bytes.
    AssetBytes,
    /// Serialized IR package.
    IrPackage,
    /// Attachment bytes outside a page asset.
    Attachment,
}

/// Content-addressed object handle stored in the ObjectStore.
///
/// The fingerprint is the blake3 hex digest of the payload and is used for
/// cross-page and cross-run deduplication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRef {
    /// Blake3 hex digest of the payload.
    pub fingerprint: String,
    /// Payload size in bytes.
    pub size: u64,
    /// Media type when the caller recorded one.
    #[serde(default)]
    pub media_type: Option<String>,
    /// Semantic kind of the payload.
    pub kind: ObjectKind,
}

/// How an authenticated browser session may persist across runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSessionPersistence {
    /// Destroy the browser context after the run.
    None,
    /// Keep encrypted ephemeral session material only for the current host.
    EncryptedEphemeral,
    /// Persist only an external vault handle and expiry metadata.
    ExternalVault,
}

/// Persisted browser session reference.
///
/// Only an external vault handle may be stored. Cookie jars, tokens and
/// passwords never enter this record or any object payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSessionRecord {
    /// Session persistence strategy.
    pub persistence: BrowserSessionPersistence,
    /// Opaque handle in the host vault, never secret material itself.
    #[serde(default)]
    pub vault_session_ref: Option<String>,
    /// Epoch milliseconds when the referenced session stops being reusable.
    #[serde(default)]
    pub expires_at_ms: Option<u64>,
    /// Origins the session may be reused for.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

/// One browser snapshot archive waiting to be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserSnapshotInput {
    /// Stable cache key for the snapshot.
    pub snapshot_key: String,
    /// Page URL the snapshot was captured for.
    pub page_url: String,
    /// MHTML, HAR or screenshot archive bytes already redacted by the browser layer.
    pub archive: Vec<u8>,
    /// Archive media type when the provider recorded one.
    pub media_type: Option<String>,
    /// Provenance label such as `browser` or `cache_hit`.
    pub transport_provenance: String,
    /// Unix epoch seconds when the snapshot was captured.
    pub captured_at_epoch: u64,
    /// Optional cache lifetime.
    pub ttl: Option<Duration>,
    /// External vault handle referenced by the snapshot, never a cookie or token.
    pub vault_session_ref: Option<String>,
}

/// Browser snapshot cache record. The archive itself lives in the ObjectStore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSnapshotRecord {
    /// Stable cache key for the snapshot.
    pub snapshot_key: String,
    /// Page URL the snapshot was captured for.
    pub page_url: String,
    /// Content-addressed archive object.
    pub snapshot_ref: ObjectRef,
    /// Provenance label such as `browser` or `cache_hit`.
    pub transport_provenance: String,
    /// Unix epoch seconds when the snapshot was captured.
    pub captured_at_epoch: u64,
    /// Epoch milliseconds when the cache entry stops being served.
    #[serde(default)]
    pub ttl_until_ms: Option<u64>,
    /// External vault handle referenced by the snapshot, never secret material.
    #[serde(default)]
    pub vault_session_ref: Option<String>,
}

/// Challenge pause recorded in run state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengePauseRecord {
    /// Frontier item that is waiting for operator input.
    pub item_id: String,
    /// Page URL that raised the challenge.
    pub page_url: String,
    /// Detected challenge state.
    pub state: PageChallengeState,
    /// Epoch milliseconds when the pause was recorded.
    pub paused_at_ms: u64,
}

/// One document asset staged inside a page transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageAssetBinding {
    /// Asset identity inside the document graph.
    pub asset_id: String,
    /// Logical asset fingerprint declared by the extractor.
    ///
    /// Storage content-addresses the payload separately, so equal bytes dedupe
    /// even when the extractor declares different logical identities.
    pub fingerprint: String,
    /// Media type when the extractor recorded one.
    pub media_type: Option<String>,
    /// Asset payload staged with the page commit.
    pub bytes: Vec<u8>,
}

/// Doctor finding severity exposed to Crawler core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreDoctorSeverity {
    /// Non-fatal inconsistency.
    Warn,
    /// Blocking inconsistency.
    Error,
}

impl StoreDoctorSeverity {
    /// Parse contract assertion severity strings.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    /// Stable contract string.
    pub fn as_contract_str(self) -> &'static str {
        match self {
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// One store-level doctor finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreDoctorIssue {
    /// Finding severity.
    pub severity: StoreDoctorSeverity,
    /// Stable diagnostic code.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

/// Per-namespace usage exposed to the store doctor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreNamespaceStats {
    /// Namespace prefix such as `cache/response/`.
    pub prefix: String,
    /// Records under the prefix.
    pub record_count: u64,
    /// Payload bytes under the prefix.
    pub bytes_used: u64,
    /// Configured byte quota, when one was set for the prefix.
    pub quota_max_bytes: Option<u64>,
    /// TTL records past expiry but not yet evicted.
    pub ttl_expired_pending: u64,
}

/// Read-only store diagnostic report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreDiagnosticReport {
    /// CAS objects without metadata references.
    pub orphan_object_count: u64,
    /// Commits not yet folded into the main file by a checkpoint.
    pub checkpoint_lag_records: u64,
    /// Namespace usage snapshots reported by the backend.
    pub namespaces: Vec<StoreNamespaceStats>,
    /// Findings requiring attention.
    pub issues: Vec<StoreDoctorIssue>,
}
