use std::time::{SystemTime, UNIX_EPOCH};

use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use url::Url;

use crate::{
    diagnostics as pd,
    error::{CrawlError, Result},
};

/// Canonicalize a request URL for frontier deduplication.
///
/// Rules are intentionally conservative: scheme and host are lowercased,
/// default ports removed, path segments normalized, and trailing slashes on
/// directory-like paths preserved. Query strings are kept intact because they
/// may affect resource identity.
pub fn canonicalize_request_url(raw: &str) -> Result<Url> {
    let parsed = Url::parse(raw).map_err(|error| {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::URL_INVALID),
            DiagnosticSeverity::Error,
            pd::origin_fetch(),
            Message::new("crawler.diagnostic.urlParse")
                .with_arg("input", MessageArg::Text(raw.to_string()))
                .with_arg("error", MessageArg::Text(error.to_string()))
                .with_fallback(format!("invalid URL `{raw}`: {error}")),
        );
        CrawlError::from(diag)
    })?;
    canonicalize_parsed_url(parsed)
}

/// Maximum request URL length accepted before admission.
pub const MAX_URL_LENGTH: usize = 8192;

/// Canonicalize an already parsed URL.
pub fn canonicalize_parsed_url(mut url: Url) -> Result<Url> {
    if url.scheme() != "http" && url.scheme() != "https" && url.scheme() != "file" {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::URL_OUT_OF_POLICY),
            DiagnosticSeverity::Error,
            pd::origin_fetch(),
            Message::new("crawler.diagnostic.urlScheme")
                .with_arg("scheme", MessageArg::Text(url.scheme().to_string()))
                .with_fallback(format!("unsupported URL scheme `{}`", url.scheme())),
        );
        return Err(CrawlError::from(diag));
    }

    if url.as_str().len() > MAX_URL_LENGTH {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::URL_OUT_OF_POLICY),
            DiagnosticSeverity::Error,
            pd::origin_fetch(),
            Message::new("crawler.diagnostic.urlTooLong")
                .with_arg("max_bytes", MessageArg::U64(MAX_URL_LENGTH as u64))
                .with_arg("actual_bytes", MessageArg::U64(url.as_str().len() as u64))
                .with_fallback(format!("URL exceeds maximum length {} bytes (got {})", MAX_URL_LENGTH, url.as_str().len())),
        );
        return Err(CrawlError::from(diag));
    }

    // Fragments never leave the client; strip to avoid frontier duplicate entries.
    url.set_fragment(None);

    let host = url
        .host_str()
        .ok_or_else(|| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::URL_INVALID),
                DiagnosticSeverity::Error,
                pd::origin_fetch(),
                Message::new("crawler.diagnostic.urlMissingHost")
                    .with_arg("url", MessageArg::Text(url.to_string()))
                    .with_fallback(format!("URL must include a host")),
            );
            CrawlError::from(diag)
        })?
        .to_ascii_lowercase();

    url.set_host(Some(&host)).map_err(|error| {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::URL_INVALID),
            DiagnosticSeverity::Error,
            pd::origin_fetch(),
            Message::new("crawler.diagnostic.urlHostNormalize")
                .with_arg("host", MessageArg::Text(host))
                .with_arg("error", MessageArg::Text(error.to_string()))
                .with_fallback(format!("failed to normalize URL host: {error}")),
        );
        CrawlError::from(diag)
    })?;

    if (url.scheme() == "http" && url.port() == Some(80)) || (url.scheme() == "https" && url.port() == Some(443)) {
        let _ = url.set_port(None);
    }

    let mut path = url.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    while path.contains("//") {
        path = path.replace("//", "/");
    }
    url.set_path(&path);

    Ok(url)
}

/// Site key used for politeness partitioning.
pub fn site_key(url: &Url) -> String {
    let host = url.host_str().unwrap_or("unknown");
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// Current unix epoch seconds.
pub fn now_epoch() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs()).unwrap_or(0)
}
