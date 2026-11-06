//! Crawler store contract and backends.

#![deny(missing_docs)]

mod error;
pub mod keyspace;
mod memory;
#[cfg(feature = "yydb")]
mod response_cache;
mod session;
mod store;
mod types;

#[cfg(feature = "yydb")]
mod yydb;

pub use error::{Result, StoreError};
pub use memory::{MemoryStore, frontier_item_from_contract};
pub use store::Store;
pub use types::{
    CheckpointClaim, CheckpointView, ClaimToken, DEFAULT_CLAIM_LEASE, PageAssetBinding, PagePhase, PageTxHandle,
    ReleaseOutcome, RunHandle, RunSpec, StoreConfig, StoreDiagnosticReport, StoreDoctorIssue, StoreDoctorSeverity,
    StoreNamespaceStats,
};

#[cfg(feature = "yydb")]
pub use response_cache::YydbResponseCache;
#[cfg(feature = "yydb")]
pub use yydb::YydbStore;
