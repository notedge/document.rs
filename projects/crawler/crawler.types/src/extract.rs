use document_ir::{DocumentGraph, LossMarker};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{artifact::FetchArtifact, frontier::LinkCandidate};

/// Budget for extractor probe and lowering work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractBudget {
    /// Maximum bytes considered during probe.
    pub max_probe_bytes: u64,
    /// Maximum wall time in milliseconds for extract.
    pub max_extract_ms: u64,
}

impl Default for ExtractBudget {
    fn default() -> Self {
        Self { max_probe_bytes: 256 * 1024, max_extract_ms: 30_000 }
    }
}

/// Extractor probe status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    /// Extractor can handle this artifact.
    Supported,
    /// Media type or envelope unsupported.
    Unsupported,
    /// Probe failed before extract.
    Failed,
}

/// Result of an extractor probe call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResult {
    /// Probe outcome.
    pub status: ProbeStatus,
    /// Selected extractor label for reports.
    pub extractor: Option<String>,
}

/// Input to semantic extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractInput<'a> {
    /// Fetch artifact to extract from.
    pub artifact: &'a FetchArtifact,
    /// Extraction budget.
    pub budget: ExtractBudget,
}

/// Extractor runtime context.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtractContext {
    /// Optional page identity override.
    pub page_identity: Option<String>,
}

/// Semantic extraction status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractStatus {
    /// Extraction completed with a validated graph.
    Complete,
    /// Graph produced with semantic loss markers.
    Partial,
    /// Extractor cannot handle this artifact.
    Unsupported,
    /// Extraction failed.
    Failed,
}

/// Result of semantic extraction.
#[derive(Debug, Clone)]
pub struct ExtractResult {
    /// Extraction status.
    pub status: ExtractStatus,
    /// Source page URL.
    pub source_url: Url,
    /// Extracted semantic document when available.
    pub document: Option<DocumentGraph>,
    /// Discovered links for frontier expansion.
    pub discovered_links: Vec<LinkCandidate>,
    /// Semantic loss markers.
    pub losses: Vec<LossMarker>,
    /// Extractor label for reports.
    pub extractor: String,
}

/// Outcome of semantic extraction into Notedown IR.
#[derive(Debug, Clone)]
pub struct ExtractOutcome {
    /// Source page URL.
    pub source_url: Url,
    /// Extracted semantic document.
    pub document: DocumentGraph,
}
