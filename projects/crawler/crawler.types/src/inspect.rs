use serde::{Deserialize, Serialize};
use url::Url;

use crate::{browser::PageChallengeState, challenge::ChallengeOutcome, extract::ProbeResult, frontier::LinkCandidate};

/// Inspect report for probe, link discovery, and extract planning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectReport {
    /// Report schema identifier.
    pub schema_version: String,
    /// Source URL under inspection.
    pub source_url: Url,
    /// Requested inspect stage.
    pub stage: InspectStage,
    /// Extractor probe outcome.
    pub probe: ProbeResult,
    /// Link candidates discovered without committing IR.
    pub discovered_links: Vec<LinkCandidate>,
    /// Response media type when known.
    pub content_type: Option<String>,
    /// Artifact body size in bytes when known.
    pub body_bytes: u64,
    /// Browser challenge state when inspecting a snapshot.
    pub challenge_state: Option<PageChallengeState>,
    /// Challenge policy outcome when a non-normal state is detected.
    pub challenge_outcome: Option<ChallengeOutcome>,
}

/// Inspect stage requested by CLI or API callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectStage {
    /// Probe extractors and media type only.
    Probe,
    /// Discover links without extraction.
    Links,
    /// Build an extract plan without writing IR.
    ExtractPlan,
}

impl InspectReport {
    /// Empty inspect report for a source URL.
    pub fn empty(source_url: Url, stage: InspectStage) -> Self {
        Self {
            schema_version: crate::diagnostics::SCHEMA_INSPECT.into(),
            source_url,
            stage,
            probe: ProbeResult { status: crate::extract::ProbeStatus::Unsupported, extractor: None },
            discovered_links: Vec::new(),
            content_type: None,
            body_bytes: 0,
            challenge_state: None,
            challenge_outcome: None,
        }
    }
}
