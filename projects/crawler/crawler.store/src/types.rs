use serde::{Deserialize, Serialize};

/// Store open parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreConfig {
    /// File path for file-backed backends such as `YydbStore`.
    pub db_path: Option<std::path::PathBuf>,
}

impl StoreConfig {
    /// Memory-oriented defaults.
    pub fn memory() -> Self {
        Self { db_path: None }
    }

    /// YYDB file path for integration tests and production.
    pub fn yydb(path: impl Into<std::path::PathBuf>) -> Self {
        Self { db_path: Some(path.into()) }
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

/// Read-only store diagnostic report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreDiagnosticReport {
    /// CAS objects without metadata references.
    pub orphan_object_count: u64,
    /// Findings requiring attention.
    pub issues: Vec<StoreDoctorIssue>,
}
