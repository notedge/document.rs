use std::collections::BTreeMap;

use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_types::{BrowserSnapshot, CrawlError, Result, diagnostics as pd};
use url::Url;

/// Conceptual browser provider capability. Implementations live outside `crawler` core transport.
pub trait BrowserProvider {
    /// Capture or retrieve a browser snapshot for the given URL.
    fn capture_snapshot(&self, url: &Url) -> Result<BrowserSnapshot>;
}

/// In-memory browser provider for tests and offline fixtures.
#[derive(Debug, Clone, Default)]
pub struct FixtureBrowserProvider {
    snapshots: BTreeMap<String, BrowserSnapshot>,
}

impl FixtureBrowserProvider {
    /// Empty fixture provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a snapshot keyed by final URL string.
    pub fn insert(&mut self, snapshot: BrowserSnapshot) {
        self.snapshots.insert(snapshot.final_url.to_string(), snapshot.clone());
        if snapshot.requested_url != snapshot.final_url {
            self.snapshots.insert(snapshot.requested_url.to_string(), snapshot);
        }
    }
}

impl BrowserProvider for FixtureBrowserProvider {
    fn capture_snapshot(&self, url: &Url) -> Result<BrowserSnapshot> {
        self.snapshots.get(url.as_str()).cloned().ok_or_else(|| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::BROWSER_UNAVAILABLE),
                DiagnosticSeverity::Error,
                pd::origin_browser(),
                Message::new("crawler.diagnostic.fixtureSnapshotMissing")
                    .with_arg("url", MessageArg::Text(url.to_string()))
                    .with_fallback(format!("no fixture snapshot for `{url}`")),
            );
            CrawlError::from(diag)
        })
    }
}
