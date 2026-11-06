use std::collections::BTreeMap;

use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_types::{BrowserSnapshot, CrawlError, Result, diagnostics as pd};
use url::Url;

/// Conceptual browser provider capability. Implementations live outside `crawler` core transport.
pub trait BrowserProvider {
    /// Capture or retrieve a browser snapshot for the given URL.
    fn capture_snapshot(&self, url: &Url) -> Result<BrowserSnapshot>;
}

fn boundary_error(key: &'static str, args: &[(&'static str, String)], fallback: String) -> CrawlError {
    let mut message = Message::new(key).with_fallback(fallback);
    for (name, value) in args {
        message = message.with_arg(*name, MessageArg::Text(value.clone()));
    }
    CrawlError::from(boost_diagnostic::Diagnostic::new(
        pd::code(pd::BROWSER_SNAPSHOT_INVALID),
        DiagnosticSeverity::Error,
        pd::origin_browser(),
        message,
    ))
}

/// Reject a snapshot that violates browser boundary rules.
///
/// A cross-origin frame can never carry DOM bytes, and a closed shadow root is
/// unreadable. A provider that reports either would be smuggling content across
/// a boundary the browser does not allow, so the snapshot fails closed.
pub fn validate_browser_boundary(snapshot: &BrowserSnapshot) -> Result<()> {
    let page_origin = snapshot.final_url.origin();
    for frame in &snapshot.boundary.frames {
        let same_origin = frame.url.origin() == page_origin;
        if frame.same_origin != same_origin {
            return Err(boundary_error(
                "crawler.diagnostic.browserFrameOriginMismatch",
                &[("url", frame.url.to_string()), ("page", snapshot.final_url.to_string())],
                format!("frame `{}` origin does not match its `same_origin` flag", frame.url),
            ));
        }
        if !same_origin && frame.document_html.is_some() {
            return Err(boundary_error(
                "crawler.diagnostic.browserCrossOriginFrameDom",
                &[("url", frame.url.to_string())],
                format!("cross-origin frame `{}` must not expose DOM bytes", frame.url),
            ));
        }
    }
    for root in &snapshot.boundary.shadow_roots {
        if !root.open && root.document_html.is_some() {
            return Err(boundary_error(
                "crawler.diagnostic.browserClosedShadowRootDom",
                &[("host", root.host.clone())],
                format!("closed shadow root `{}` must not expose DOM bytes", root.host),
            ));
        }
    }
    Ok(())
}

/// In-memory browser provider for tests and offline fixtures.
#[derive(Debug, Clone, Default)]
pub struct FixtureBrowserProvider {
    snapshots: BTreeMap<String, BrowserSnapshot>,
    failures: BTreeMap<String, CrawlError>,
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

    /// Register a capture failure for the given URL.
    ///
    /// Used by crash fixtures: the next capture of that URL returns the stored
    /// error instead of a snapshot, so the crawl can prove it fails closed.
    pub fn insert_failure(&mut self, url: &Url, error: CrawlError) {
        self.failures.insert(url.to_string(), error);
    }
}

impl BrowserProvider for FixtureBrowserProvider {
    fn capture_snapshot(&self, url: &Url) -> Result<BrowserSnapshot> {
        if let Some(error) = self.failures.get(url.as_str()) {
            return Err(error.clone());
        }
        let snapshot = self.snapshots.get(url.as_str()).cloned().ok_or_else(|| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::BROWSER_UNAVAILABLE),
                DiagnosticSeverity::Error,
                pd::origin_browser(),
                Message::new("crawler.diagnostic.fixtureSnapshotMissing")
                    .with_arg("url", MessageArg::Text(url.to_string()))
                    .with_fallback(format!("no fixture snapshot for `{url}`")),
            );
            CrawlError::from(diag)
        })?;
        validate_browser_boundary(&snapshot)?;
        Ok(snapshot)
    }
}
