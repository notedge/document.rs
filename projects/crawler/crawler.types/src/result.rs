use boost_diagnostic::{DiagnosticEnvelope, DiagnosticSet};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::events::EventLog;

/// Per-page crawl status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlPageStatus {
    /// Fetch and extraction succeeded.
    Extracted,
    /// Fetch succeeded but extraction failed or was skipped.
    FetchOnly,
    /// Page failed to fetch or extract.
    Failed,
    /// Skipped by budget, robots, or policy.
    Skipped,
}

/// How a crawl run ended with respect to its request budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CrawlBudgetStatus {
    /// Frontier drained within budget.
    #[default]
    Completed,
    /// Request budget ran out with frontier items still pending.
    Exhausted,
    /// Run paused for operator action before finishing.
    Cancelled,
}

/// High-level crawl report aligned with `crawler.report/v2`.
///
/// Every run-level [`boost_diagnostic::Diagnostic`] emitted during the crawl
/// is carried in [`Self::diagnostics`] as a structured envelope so CLI /
/// N-API consumers can surface stable codes and typed args without parsing
/// free-form strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrawlReport {
    /// Report schema identifier (`crawler.report/v2`).
    pub schema_version: String,
    /// Primary seed URL.
    pub seed: Url,
    /// Pages extracted successfully.
    pub extracted: u32,
    /// Pages that failed.
    pub failed: u32,
    /// Pages skipped by policy or budget.
    pub skipped: u32,
    /// Pages paused awaiting operator action.
    pub paused: u32,
    /// Pages committed to persistent storage.
    pub committed: u32,
    /// How the run ended with respect to its request budget.
    pub budget_status: CrawlBudgetStatus,
    /// Frontier items still pending when the budget was exhausted.
    pub remaining_frontier: u32,
    /// Structured event log snapshot.
    pub events: EventLog,
    /// Run-level diagnostic envelope (never absent — empty set if clean).
    pub diagnostics: DiagnosticEnvelope,
}

impl CrawlReport {
    /// Empty report for a seed.
    pub fn empty(seed: Url) -> Self {
        Self {
            schema_version: crate::diagnostics::SCHEMA_REPORT.to_string(),
            seed,
            extracted: 0,
            failed: 0,
            skipped: 0,
            paused: 0,
            committed: 0,
            budget_status: CrawlBudgetStatus::Completed,
            remaining_frontier: 0,
            events: EventLog::new(),
            diagnostics: DiagnosticEnvelope::from_set(&DiagnosticSet::new()),
        }
    }

    /// Collect a run-level diagnostic into the report envelope.
    pub fn push_diagnostic(&mut self, diagnostic: boost_diagnostic::Diagnostic) {
        self.diagnostics.diagnostics.push(diagnostic);
    }
}
