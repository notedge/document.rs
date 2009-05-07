use serde::{Deserialize, Serialize};
use url::Url;

use crate::url::now_epoch;

/// Why a frontier item was discovered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    /// Seed URL supplied by the user.
    Seed,
    /// Anchor navigation link.
    NavigationLink {
        /// Source page URL.
        from: Url,
    },
    /// Sitemap or manifest entry.
    Manifest,
    /// Recovered from checkpoint.
    Checkpoint,
}

/// Relation kind for discovered links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkRelation {
    /// May enter frontier for crawl continuation.
    Navigation,
    /// Asset download candidate.
    Asset,
    /// Canonical identity hint.
    Canonical,
    /// External reference for reporting only.
    External,
}

/// Candidate link emitted by discovery, not yet admitted to frontier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkCandidate {
    /// Target URL string before canonicalization.
    pub target: String,
    /// Relation classification.
    pub relation: LinkRelation,
    /// Optional source span label for reports.
    pub source_hint: Option<String>,
}

/// One schedulable frontier item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontierItem {
    /// Canonical request URL used for deduplication.
    pub request_url: Url,
    /// Where this item was discovered.
    pub discovery: DiscoverySource,
    /// Depth from seed.
    pub depth: u32,
    /// Politeness partition key.
    pub site_key: String,
    /// Lower values run first.
    pub priority: i32,
    /// Attempt count including retries.
    pub attempts: u32,
    /// Unix epoch when discovered.
    pub discovered_at_epoch: u64,
}

impl FrontierItem {
    /// Build a seed frontier item at depth zero.
    pub fn seed(request_url: Url, site_key: String) -> Self {
        Self {
            request_url,
            discovery: DiscoverySource::Seed,
            depth: 0,
            site_key,
            priority: 0,
            attempts: 0,
            discovered_at_epoch: now_epoch(),
        }
    }
}

/// Why frontier admission rejected an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionDecision {
    /// Item may proceed to fetch lease.
    Admitted,
    /// Rejected by policy with reason code.
    Rejected {
        /// Stable machine-readable reason.
        reason: String,
    },
}
