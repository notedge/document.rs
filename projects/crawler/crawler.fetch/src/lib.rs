//! Fetch transports, robots policy, and response caching.

#![deny(missing_docs)]

use std::collections::BTreeMap;

use crawler_types::{BodyStorage, CrawlError, FetchArtifact, RedirectHop, Result, TransportProvenance, now_epoch};
use url::Url;

mod cache;
mod client;
mod disk_cache;
mod file;
mod http;
mod memory;
mod robots;
mod robots_txt;
mod select;

pub use cache::ResponseCache;
pub use client::{FetchClient, HttpStatusClass, classify_http_status, fetch_url};
pub use disk_cache::{DiskResponseCache, LayeredResponseCache};
pub use file::FileTransport;
pub use http::HttpTransport;
pub use memory::{MemoryEntry, MemoryTransport};
pub use robots::RobotsPolicy;
pub use robots_txt::{RobotsGroup, RobotsTxt, path_matches_rule, site_key_for_url};
pub use select::{SeedTransport, transport_for_url};

/// Fetch request passed to a transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    /// Request URL.
    pub url: Url,
    /// Optional request headers.
    pub headers: BTreeMap<String, String>,
}

/// Transport that materializes response bytes without semantic parsing.
pub trait Transport {
    /// Perform one fetch for the request URL.
    fn fetch(&self, request: &FetchRequest) -> Result<FetchArtifact>;
}

/// Build a cache key from the request URL and optional validator.
pub fn cache_key_for(url: &Url, validator: Option<&str>) -> String {
    match validator {
        Some(tag) => format!("{}|etag={tag}", url),
        None => url.to_string(),
    }
}

/// Construct a fetch artifact from raw response parts.
pub fn build_artifact(
    request_url: Url,
    final_url: Url,
    status: u16,
    headers: BTreeMap<String, String>,
    body: BodyStorage,
    redirect_chain: Vec<RedirectHop>,
    provenance: TransportProvenance,
    validator: Option<&str>,
) -> FetchArtifact {
    let content_type = headers.get("content-type").cloned().or_else(|| infer_content_type(&final_url));
    let cache_key = cache_key_for(&request_url, validator);
    FetchArtifact {
        request_url,
        final_url,
        status,
        response_headers: headers,
        content_type,
        body,
        redirect_chain,
        retrieved_at_epoch: now_epoch(),
        transport_provenance: provenance,
        cache_key,
    }
}

fn infer_content_type(url: &Url) -> Option<String> {
    let path = url.path().to_ascii_lowercase();
    if path.ends_with(".html") || path.ends_with(".htm") {
        Some("text/html".into())
    }
    else if path.ends_with(".css") {
        Some("text/css".into())
    }
    else if path.ends_with(".json") {
        Some("application/json".into())
    }
    else {
        None
    }
}

/// Map transport failures into crawl errors.
pub fn map_transport_error(message: impl Into<String>) -> CrawlError {
    let message = message.into();
    log_transport_error(&message);
    let diag = boost_diagnostic::Diagnostic::new(
        crawler_types::diagnostics::code(crawler_types::diagnostics::FETCH_TRANSPORT_FAILED),
        boost_diagnostic::DiagnosticSeverity::Error,
        crawler_types::diagnostics::origin_fetch(),
        boost_diagnostic::Message::new("crawler.diagnostic.transportFailed")
            .with_arg("message", boost_diagnostic::MessageArg::Text(message.clone()))
            .with_fallback(format!("transport failure: {message}")),
    );
    CrawlError::from(diag)
}

#[cfg(feature = "logger")]
fn log_transport_error(message: &str) {
    boost_log::event!(Error, "crawler.transport", message = message);
}

#[cfg(not(feature = "logger"))]
fn log_transport_error(_message: &str) {}
