use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_types::{CrawlError, Result, diagnostics as pd};
use url::Url;

use super::{FetchArtifact, FetchRequest, FileTransport, HttpTransport, Transport};

/// Built-in transports selected from a seed URL scheme.
#[derive(Debug, Clone)]
pub enum SeedTransport {
    /// Local `file://` pages and fixtures.
    File(FileTransport),
    /// Remote `http://` and `https://` pages.
    Http(HttpTransport),
}

impl Transport for SeedTransport {
    fn fetch(&self, request: &FetchRequest) -> Result<FetchArtifact> {
        match self {
            Self::File(transport) => transport.fetch(request),
            Self::Http(transport) => transport.fetch(request),
        }
    }
}

/// Pick the default transport for a parsed seed URL.
pub fn transport_for_url(url: &Url) -> Result<SeedTransport> {
    match url.scheme() {
        "file" => Ok(SeedTransport::File(FileTransport::new())),
        "http" | "https" => Ok(SeedTransport::Http(HttpTransport::new())),
        other => {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::URL_OUT_OF_POLICY),
                DiagnosticSeverity::Error,
                pd::origin_fetch(),
                Message::new("crawler.diagnostic.seedScheme")
                    .with_arg("scheme", MessageArg::Text(other.to_string()))
                    .with_arg("url", MessageArg::Text(url.to_string()))
                    .with_fallback(format!("unsupported seed scheme `{other}` for `{url}`")),
            );
            Err(CrawlError::from(diag))
        }
    }
}
