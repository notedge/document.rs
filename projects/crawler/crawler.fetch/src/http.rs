use std::{collections::BTreeMap, time::Duration};

use boost_diagnostic::{Diagnostic, DiagnosticSeverity, Message, MessageArg};
use boost_http::{
    Headers, HttpTransport as RequestHttpTransport, HttpTransportConfig, Method, Request, RequestError,
    Transport as RequestTransport,
};
use crawler_types::{BodyStorage, CrawlError, RedirectHop, Result, TransportProvenance};

use super::{FetchRequest, Transport, build_artifact};
use crawler_types::diagnostics as pd;

fn headers_to_map(headers: &Headers) -> BTreeMap<String, String> {
    headers.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
}

/// Default HTTP response body budget (4 MiB).
///
/// This deliberately sits below the browser snapshot cap so a page rejected by
/// the HTTP tier can still be delivered by the render tier.
pub const DEFAULT_FETCH_BODY_BUDGET_BYTES: usize = 4 * 1024 * 1024;

/// Network transport for `http://` and `https://` URLs.
#[derive(Debug, Clone)]
pub struct HttpTransport {
    /// Maximum response body bytes to read.
    pub max_body_bytes: usize,
    /// Maximum redirect hops to follow.
    pub max_redirects: usize,
    /// Per-request timeout.
    pub timeout: Duration,
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self { max_body_bytes: DEFAULT_FETCH_BODY_BUDGET_BYTES, max_redirects: 8, timeout: Duration::from_secs(30) }
    }
}

impl HttpTransport {
    /// Conservative defaults aligned with crawl budgets.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Transport for HttpTransport {
    fn fetch(&self, request: &FetchRequest) -> Result<crawler_types::FetchArtifact> {
        let scheme = request.url.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(make_transport_diag(
                pd::FETCH_TRANSPORT_FAILED,
                &format!("http transport cannot fetch `{}`", request.url),
            ));
        }

        let max_redirects = u32::try_from(self.max_redirects)
            .map_err(|_| make_transport_diag(pd::FETCH_TRANSPORT_FAILED, "redirect budget exceeds supported range"))?;
        let transport = RequestHttpTransport::with_config(HttpTransportConfig {
            max_body_bytes: self.max_body_bytes,
            max_redirects,
            timeout: self.timeout,
            ..HttpTransportConfig::default()
        });
        let mut http_request = Request::new(Method::Get, request.url.clone());
        for (name, value) in &request.headers {
            http_request.set_header(name, value);
        }
        let response = transport.send(&http_request).map_err(map_http_request_error)?;
        let headers = headers_to_map(&response.headers);
        let etag = headers.get("etag").cloned();
        let redirect_chain =
            response.redirects.into_iter().map(|redirect| RedirectHop { url: redirect.url, status: redirect.status }).collect();
        Ok(build_artifact(
            request.url.clone(),
            response.url,
            response.status,
            headers,
            BodyStorage::inline(response.body),
            redirect_chain,
            TransportProvenance::Network,
            etag.as_deref(),
        ))
    }
}

// ─── boost_http RequestError → CrawlError 分型 ──────────────────────────────

/// Map a `boost_http::RequestError` into a structured [`CrawlError`] with the
/// correct stable diagnostic code for timeout / TLS / redirect / body-budget /
/// generic transport failures.
fn map_http_request_error(error: RequestError) -> CrawlError {
    match error {
        RequestError::Unsupported(msg) => make_transport_diag(pd::FETCH_TRANSPORT_FAILED, &msg),
        RequestError::Transport(msg) => classify_transport_msg(&msg),
        RequestError::Body(io_err) => make_transport_diag(pd::FETCH_DECODE_FAILED, &format!("body read failed: {io_err}")),
        RequestError::BodyTooLarge { limit } => {
            make_transport_diag_u64(pd::FETCH_BODY_BUDGET, "response body exceeded configured budget", limit as u64)
        }
        RequestError::HeaderTooLarge { limit } => {
            make_transport_diag_u64(pd::FETCH_BODY_BUDGET, "response headers exceeded configured budget", limit as u64)
        }
    }
}

/// Sub-classify a `RequestError::Transport` inner message into timeout /
/// redirect-limit / TLS-failed / catch-all transport-failed.
fn classify_transport_msg(msg: &str) -> CrawlError {
    let lower = msg.to_ascii_lowercase();
    if lower.contains("timeout") || lower.contains("deadline") || lower.contains("timed out") {
        make_transport_diag(pd::FETCH_TIMEOUT, msg)
    }
    else if lower.contains("redirect chain exceeded") || lower.contains("redirect loop exhausted") {
        make_transport_diag(pd::FETCH_REDIRECT_LIMIT, msg)
    }
    else if lower.contains("tls") || lower.contains("certificate") || lower.contains("handshake") {
        make_transport_diag(pd::FETCH_TLS_FAILED, msg)
    }
    else {
        make_transport_diag(pd::FETCH_TRANSPORT_FAILED, msg)
    }
}

/// Build a [`CrawlError`] with a string `message` arg, routed to the given
/// stable `DiagnosticCode` under the `fetch` origin.
fn make_transport_diag(code: &'static str, message: &str) -> CrawlError {
    let key = diagnostic_key_for(code);
    let diag = Diagnostic::new(
        pd::code(code),
        DiagnosticSeverity::Error,
        pd::origin_fetch(),
        Message::new(&format!("crawler.diagnostic.{key}"))
            .with_arg("message", MessageArg::Text(message.to_string()))
            .with_fallback(message.to_string()),
    );
    CrawlError::from(diag)
}

/// Build a [`CrawlError`] with a u64 numeric arg, routed to the given stable
/// `DiagnosticCode` under the `fetch` origin.
fn make_transport_diag_u64(code: &'static str, message: &str, value: u64) -> CrawlError {
    let key = diagnostic_key_for(code);
    let diag = Diagnostic::new(
        pd::code(code),
        DiagnosticSeverity::Error,
        pd::origin_fetch(),
        Message::new(&format!("crawler.diagnostic.{key}"))
            .with_arg("limit", MessageArg::U64(value))
            .with_fallback(format!("{message}: {value} bytes")),
    );
    CrawlError::from(diag)
}

/// Derive a short message key suffix from a stable diagnostic code.
fn diagnostic_key_for(code: &'static str) -> &'static str {
    // Map full code → short key for message ID.
    match code {
        pd::FETCH_TIMEOUT => "fetchTimeout",
        pd::FETCH_TLS_FAILED => "fetchTlsFailed",
        pd::FETCH_REDIRECT_LIMIT => "fetchRedirectLimit",
        pd::FETCH_BODY_BUDGET => "fetchBodyBudget",
        pd::FETCH_DECODE_FAILED => "fetchDecodeFailed",
        pd::FETCH_TRANSPORT_FAILED => "fetchTransportFailed",
        _ => "fetchError",
    }
}
