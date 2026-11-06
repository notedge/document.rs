use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Default lease granted to a frontier claim.
pub const DEFAULT_CLAIM_LEASE: Duration = Duration::from_secs(30);

/// Store open parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreConfig {
    /// File path for file-backed backends such as `YydbStore`.
    pub db_path: Option<std::path::PathBuf>,
    /// Lease duration granted when a worker claims a frontier item.
    pub claim_lease: Duration,
}

impl StoreConfig {
    /// Memory-oriented defaults.
    pub fn memory() -> Self {
        Self { db_path: None, claim_lease: DEFAULT_CLAIM_LEASE }
    }

    /// YYDB file path for integration tests and production.
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
}

/// In-flight page transaction handle.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageTxHandle {
    /// Page record identifier.
    pub page_id: String,
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
            Self::Committed => "committed",
        }
    }
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
