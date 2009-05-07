use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

/// HTTP status carried on a fetch artifact.
pub type HttpStatus = u16;

/// One hop in a redirect chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedirectHop {
    /// Request URL for this hop.
    pub url: Url,
    /// Response status for this hop.
    pub status: HttpStatus,
}

/// Where response bytes live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyStorage {
    /// Inline response bytes.
    Inline(Vec<u8>),
    /// Opaque cache reference for out-of-line bytes.
    Reference {
        /// Cache namespace key.
        cache_key: String,
    },
}

impl BodyStorage {
    /// Inline body helper.
    pub fn inline(bytes: impl Into<Vec<u8>>) -> Self {
        Self::Inline(bytes.into())
    }

    /// Return inline bytes when present.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Inline(bytes) => Some(bytes),
            Self::Reference { .. } => None,
        }
    }
}

/// Transport-level provenance for a fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportProvenance {
    /// Served from network transport.
    Network,
    /// Loaded from local file transport.
    File,
    /// Reused from response cache.
    CacheHit,
    /// Injected by tests or harness.
    Test,
}

/// Stable boundary between fetch and extract stages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchArtifact {
    /// Request URL before redirects.
    pub request_url: Url,
    /// Final URL after redirects.
    pub final_url: Url,
    /// Final HTTP status.
    pub status: HttpStatus,
    /// Response headers in wire order keys.
    pub response_headers: BTreeMap<String, String>,
    /// Parsed media type when known.
    pub content_type: Option<String>,
    /// Response body storage.
    pub body: BodyStorage,
    /// Redirect chain excluding the final response when empty.
    pub redirect_chain: Vec<RedirectHop>,
    /// Unix epoch seconds when the body was retrieved.
    pub retrieved_at_epoch: u64,
    /// How the artifact was obtained.
    pub transport_provenance: TransportProvenance,
    /// Cache key for idempotent reuse.
    pub cache_key: String,
}

impl FetchArtifact {
    /// Return inline body bytes when available.
    pub fn body_bytes(&self) -> Option<&[u8]> {
        self.body.as_bytes()
    }
}
