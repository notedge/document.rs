use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    extract::{ExtractResult, ExtractStatus},
    frontier::{AdmissionDecision, LinkCandidate},
};

/// Crawl lifecycle event kinds for reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CrawlEvent {
    /// Link or seed discovered.
    Discovered {
        /// Target URL.
        url: Url,
        /// Depth from seed.
        depth: u32,
    },
    /// Frontier admission result.
    Admitted {
        /// Canonical request URL.
        url: Url,
        /// Admission decision.
        decision: AdmissionDecision,
    },
    /// Fetch produced an artifact.
    Fetched {
        /// Final response URL.
        url: Url,
        /// HTTP status.
        status: u16,
        /// Whether response came from cache.
        cache_hit: bool,
    },
    /// A browser snapshot was used instead of the HTTP result.
    BrowserFallback {
        /// Request URL that failed over HTTP or yielded too little content.
        url: Url,
        /// Why the crawl invoked the renderer.
        trigger: BrowserFallbackTrigger,
        /// Original HTTP fetch error message when the trigger was a fetch failure.
        fetch_error: Option<String>,
        /// Browser engine label from the snapshot provider.
        browser_engine: String,
        /// Extracted text characters observed on the HTTP artifact before fallback.
        observed_text_chars: Option<u64>,
        /// Content-bearing IR blocks observed on the HTTP artifact before fallback.
        observed_content_blocks: Option<u32>,
    },
    /// The browser provider failed while rendering a fallback page.
    BrowserFallbackFailed {
        /// Request URL whose render fallback failed.
        url: Url,
        /// Why the crawl invoked the renderer.
        trigger: BrowserFallbackTrigger,
        /// Stable diagnostic code produced by the provider failure.
        diagnostic_code: String,
        /// Provider-native code such as `pandark.browser.snapshot-too-large`.
        provider_code: Option<String>,
        /// Human-readable provider failure message.
        reason: String,
    },
    /// Extractor finished.
    Extracted {
        /// Source URL.
        url: Url,
        /// Extract status.
        status: ExtractStatus,
        /// Extractor label.
        extractor: String,
    },
    /// Page transaction committed.
    Committed {
        /// Page identity key.
        page_identity: String,
    },
    /// Item skipped by policy or budget.
    Skipped {
        /// URL or item key.
        url: Url,
        /// Machine-readable reason.
        reason: String,
    },
    /// Stage failed.
    Failed {
        /// URL or item key.
        url: Url,
        /// Machine-readable reason.
        reason: String,
    },
    /// Challenge requires operator action before the page can proceed.
    PausedForOperator {
        /// Request URL awaiting operator input.
        url: Url,
        /// Detected challenge state.
        challenge_state: crate::browser::PageChallengeState,
    },
}

/// Why the crawl invoked the browser renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserFallbackTrigger {
    /// HTTP fetch failed before an artifact could be extracted.
    FetchFailure,
    /// HTTP fetch succeeded but the extracted document was empty or too thin.
    LowYield,
}

/// In-memory event log for reports and tests.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLog {
    events: Vec<CrawlEvent>,
}

impl EventLog {
    /// Empty event log.
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Append an event.
    pub fn push(&mut self, event: CrawlEvent) {
        self.events.push(event);
    }

    /// Immutable view of events.
    pub fn events(&self) -> &[CrawlEvent] {
        &self.events
    }

    /// Count events matching a predicate.
    pub fn count<F>(&self, predicate: F) -> u32
    where
        F: Fn(&CrawlEvent) -> bool,
    {
        self.events.iter().filter(|event| predicate(event)).count() as u32
    }
}

/// Pending page transaction before commit.
#[derive(Debug, Clone)]
pub struct PageTransaction {
    /// Stable page identity key.
    pub page_identity: String,
    /// Extract result pending validation.
    pub extract: ExtractResult,
    /// Link candidates discovered on the page.
    pub discovered_links: Vec<LinkCandidate>,
}
