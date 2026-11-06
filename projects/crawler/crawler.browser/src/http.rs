use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use boost_http::{HttpTransport, HttpTransportConfig, Method, Request, Transport};
use crawler_types::{BrowserSnapshot, CrawlError, Result, diagnostics as pd};
use url::Url;

/// Default maximum HTML bytes a browser worker returns for one snapshot.
///
/// The worker enforces this cap before serializing the snapshot.
pub const DEFAULT_SNAPSHOT_DOCUMENT_BUDGET_BYTES: usize = 8 * 1024 * 1024;

/// Default maximum bytes accepted for the serialized snapshot envelope.
///
/// This sits above [`DEFAULT_SNAPSHOT_DOCUMENT_BUDGET_BYTES`] so JSON escaping
/// and metadata cannot make a valid snapshot look oversized.
pub const DEFAULT_SNAPSHOT_TRANSFER_BUDGET_BYTES: usize = 16 * 1024 * 1024;

/// Remote browser worker that returns `BrowserSnapshot` JSON over HTTP.
#[derive(Debug, Clone)]
pub struct HttpBrowserProvider {
    /// Worker base URL, for example `http://127.0.0.1:9333/`.
    endpoint: Url,
    /// Profile label recorded in captured snapshots.
    profile_id: String,
    /// Maximum serialized snapshot bytes accepted from the worker.
    max_snapshot_bytes: usize,
}

impl HttpBrowserProvider {
    /// Build a provider for the given worker base URL.
    pub fn new(endpoint: Url) -> Self {
        Self { endpoint, profile_id: "http-browser".into(), max_snapshot_bytes: DEFAULT_SNAPSHOT_TRANSFER_BUDGET_BYTES }
    }

    /// Override the profile label stored on fetched snapshots.
    pub fn with_profile_id(mut self, profile_id: impl Into<String>) -> Self {
        self.profile_id = profile_id.into();
        self
    }

    /// Override the maximum serialized snapshot bytes accepted from the worker.
    pub fn with_max_snapshot_bytes(mut self, max_snapshot_bytes: usize) -> Self {
        self.max_snapshot_bytes = max_snapshot_bytes;
        self
    }

    /// Build the worker snapshot URL for a page.
    pub fn snapshot_url(&self, page_url: &Url) -> Url {
        let mut endpoint = self.endpoint.clone();
        endpoint.query_pairs_mut().append_pair("url", page_url.as_str());
        endpoint
    }
}

impl crate::provider::BrowserProvider for HttpBrowserProvider {
    fn capture_snapshot(&self, url: &Url) -> Result<BrowserSnapshot> {
        let request_url = self.snapshot_url(url);
        let transport = HttpTransport::with_config(HttpTransportConfig {
            max_body_bytes: self.max_snapshot_bytes,
            max_redirects: 4,
            ..HttpTransportConfig::default()
        });
        let request = Request::new(Method::Get, request_url);
        let response = transport.send(&request).map_err(|error| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::BROWSER_UNAVAILABLE),
                DiagnosticSeverity::Error,
                pd::origin_browser(),
                Message::new("crawler.diagnostic.browserWorkerRequestFailed")
                    .with_arg("error", MessageArg::Text(error.to_string()))
                    .with_fallback(format!("browser worker request failed: {error}")),
            );
            CrawlError::from(diag)
        })?;
        let status = response.status;
        if !(200..300).contains(&status) {
            let worker_code = response.headers.get("x-pandark-code").map(str::to_string);
            let fallback = match worker_code.as_deref() {
                Some(code) => format!("browser worker returned status {status} ({code})"),
                None => format!("browser worker returned status {status}"),
            };
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::BROWSER_WORKER_STATUS),
                DiagnosticSeverity::Error,
                pd::origin_browser(),
                Message::new("crawler.diagnostic.browserWorkerHttpStatus")
                    .with_arg("status", MessageArg::I64(status as i64))
                    .with_arg("workerCode", MessageArg::Text(worker_code.unwrap_or_default()))
                    .with_fallback(fallback),
            );
            return Err(CrawlError::from(diag));
        }
        let snapshot: BrowserSnapshot = serde_json::from_slice(&response.body).map_err(|error| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::BROWSER_SNAPSHOT_INVALID),
                DiagnosticSeverity::Error,
                pd::origin_browser(),
                Message::new("crawler.diagnostic.browserWorkerJsonInvalid")
                    .with_arg("error", MessageArg::Text(error.to_string()))
                    .with_fallback(format!("browser worker JSON invalid: {error}")),
            );
            CrawlError::from(diag)
        })?;
        Ok(snapshot)
    }
}
