use std::collections::BTreeMap;

use crawler_types::{BodyStorage, FetchArtifact, Result, TransportProvenance};
use url::Url;

use super::{FetchRequest, Transport, build_artifact, map_transport_error};

/// In-memory transport for tests and offline fixtures.
#[derive(Debug, Clone, Default)]
pub struct MemoryTransport {
    entries: BTreeMap<String, MemoryEntry>,
    redirects: BTreeMap<String, String>,
}

/// Stored response body and metadata.
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    /// HTTP status.
    pub status: u16,
    /// Response headers.
    pub headers: BTreeMap<String, String>,
    /// Body bytes.
    pub body: Vec<u8>,
}

impl MemoryTransport {
    /// Empty memory transport.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a response for an exact URL string key.
    pub fn insert(&mut self, url: impl Into<String>, entry: MemoryEntry) -> &mut Self {
        self.entries.insert(url.into(), entry);
        self
    }

    /// Register redirect from one URL string to another.
    pub fn redirect(&mut self, from: impl Into<String>, to: impl Into<String>) -> &mut Self {
        self.redirects.insert(from.into(), to.into());
        self
    }
}

impl Transport for MemoryTransport {
    fn fetch(&self, request: &FetchRequest) -> Result<FetchArtifact> {
        let mut request_url = request.url.clone();
        let mut redirect_chain = Vec::new();
        for _ in 0..8 {
            let key = request_url.to_string();
            if let Some(target) = self.redirects.get(&key) {
                redirect_chain.push(crawler_types::RedirectHop { url: request_url.clone(), status: 302 });
                request_url = Url::parse(target).map_err(|error| map_transport_error(error.to_string()))?;
                continue;
            }

            if let Some(entry) = self.entries.get(&key) {
                return Ok(build_artifact(
                    request.url.clone(),
                    request_url,
                    entry.status,
                    entry.headers.clone(),
                    BodyStorage::inline(entry.body.clone()),
                    redirect_chain,
                    TransportProvenance::Test,
                    None,
                ));
            }
            break;
        }

        Err(map_transport_error(format!("memory transport has no entry for `{}`", request.url)))
    }
}
