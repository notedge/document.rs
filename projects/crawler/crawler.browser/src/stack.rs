use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_types::{BrowserSnapshot, CrawlError, Result, diagnostics as pd};
use url::Url;

use crate::{
    http::HttpBrowserProvider,
    provider::{BrowserProvider, FixtureBrowserProvider},
    snapshot_io::{load_fixture_provider_from_dirs, parse_browser_dirs},
};

/// Ordered browser providers used during crawl fallback.
#[derive(Debug, Default)]
pub struct BrowserProviderStack {
    fixture: Option<FixtureBrowserProvider>,
    http: Option<HttpBrowserProvider>,
}

impl BrowserProviderStack {
    /// Empty stack with no providers configured.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a fixture directory provider.
    pub fn with_fixture(mut self, fixture: FixtureBrowserProvider) -> Self {
        self.fixture = Some(fixture);
        self
    }

    /// Attach an HTTP worker provider.
    pub fn with_http(mut self, http: HttpBrowserProvider) -> Self {
        self.http = Some(http);
        self
    }

    /// Whether any provider is configured.
    pub fn is_empty(&self) -> bool {
        self.fixture.is_none() && self.http.is_none()
    }
}

/// Build a browser provider stack from CLI-style configuration strings.
pub fn build_browser_stack(fixture_dirs: Option<&str>, http_endpoint: Option<&str>) -> Result<BrowserProviderStack> {
    let mut stack = BrowserProviderStack::new();
    if let Some(spec) = fixture_dirs {
        let dirs = parse_browser_dirs(spec);
        if !dirs.is_empty() {
            stack = stack.with_fixture(load_fixture_provider_from_dirs(&dirs)?);
        }
    }
    if let Some(endpoint) = http_endpoint {
        let url = Url::parse(endpoint).map_err(|error| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::URL_INVALID),
                DiagnosticSeverity::Error,
                pd::origin_browser(),
                Message::new("crawler.diagnostic.browserEndpointInvalid")
                    .with_arg("endpoint", MessageArg::Text(endpoint.to_string()))
                    .with_arg("error", MessageArg::Text(error.to_string()))
                    .with_fallback(format!("invalid browser endpoint `{endpoint}`: {error}")),
            );
            CrawlError::from(diag)
        })?;
        stack = stack.with_http(HttpBrowserProvider::new(url));
    }
    Ok(stack)
}

impl BrowserProvider for BrowserProviderStack {
    fn capture_snapshot(&self, url: &Url) -> Result<BrowserSnapshot> {
        let mut last_error = None;
        if let Some(fixture) = &self.fixture {
            match fixture.capture_snapshot(url) {
                Ok(snapshot) => return Ok(snapshot),
                Err(error) => last_error = Some(error),
            }
        }
        if let Some(http) = &self.http {
            return http.capture_snapshot(url);
        }
        Err(last_error.unwrap_or_else(|| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::BROWSER_UNAVAILABLE),
                DiagnosticSeverity::Error,
                pd::origin_browser(),
                Message::new("crawler.diagnostic.browserSnapshotUnavailable")
                    .with_arg("url", MessageArg::Text(url.to_string()))
                    .with_fallback(format!("no browser snapshot available for `{url}`")),
            );
            CrawlError::from(diag)
        }))
    }
}
