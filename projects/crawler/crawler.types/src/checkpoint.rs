use serde::{Deserialize, Serialize};
use url::Url;

use crate::{browser::PageChallengeState, frontier::FrontierItem, request::CrawlRequest, result::CrawlReport};

/// Serializable crawl state for operator resume after a pause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrawlCheckpoint {
    /// Checkpoint schema identifier.
    pub schema_version: String,
    /// Original crawl request.
    pub request: CrawlRequest,
    /// Frontier items not yet processed.
    pub pending_frontier: Vec<FrontierItem>,
    /// Requests consumed before the checkpoint.
    pub requests_used: u32,
    /// Report snapshot at checkpoint time.
    pub report: CrawlReport,
    /// URL that triggered the most recent operator pause, if any.
    pub paused_url: Option<Url>,
    /// Challenge state associated with the pause, if any.
    pub paused_challenge: Option<PageChallengeState>,
    /// Frontier item to retry after operator action.
    pub paused_item: Option<FrontierItem>,
    /// Fingerprint of crawl strategy knobs frozen at checkpoint time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy_fingerprint: Option<String>,
    /// Monotonic attempt identity for crash-safe resume.
    #[serde(default)]
    pub attempt_identity: u32,
}

impl CrawlCheckpoint {
    /// Create a new checkpoint envelope.
    pub fn new(
        request: CrawlRequest,
        pending_frontier: Vec<FrontierItem>,
        requests_used: u32,
        report: CrawlReport,
        paused_url: Option<Url>,
        paused_challenge: Option<PageChallengeState>,
        paused_item: Option<FrontierItem>,
        strategy_fingerprint: Option<String>,
        attempt_identity: u32,
    ) -> Self {
        Self {
            schema_version: crate::diagnostics::SCHEMA_CHECKPOINT.into(),
            request,
            pending_frontier,
            requests_used,
            report,
            paused_url,
            paused_challenge,
            paused_item,
            strategy_fingerprint,
            attempt_identity,
        }
    }
}
