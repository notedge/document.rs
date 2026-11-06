//! Browser snapshots, provider stack, and snapshot extraction.
//!
//! The crate depends on `crawler-types` and `crawler-extract` only. The crawl
//! orchestrator selects a [`BrowserProvider`] instead of owning one, so the
//! provider, the fixture/HTTP stack, and snapshot I/O stay orthogonal to the
//! crawl loop.

#![warn(missing_docs)]

mod extract;
mod http;
mod provider;
mod snapshot_io;
mod stack;

pub use extract::{ChallengeBlocked, run_extract_from_snapshot};
pub use http::{DEFAULT_SNAPSHOT_DOCUMENT_BUDGET_BYTES, DEFAULT_SNAPSHOT_TRANSFER_BUDGET_BYTES, HttpBrowserProvider};
pub use provider::{BrowserProvider, FixtureBrowserProvider, validate_browser_boundary};
pub use snapshot_io::{
    load_fixture_provider_from_dir, load_fixture_provider_from_dirs, load_snapshot_json, parse_browser_dirs,
    path_looks_like_snapshot, save_snapshot_json,
};
pub use stack::{BrowserProviderStack, build_browser_stack};

use std::collections::BTreeMap;

use crawler_types::{BodyStorage, BrowserSnapshot, FetchArtifact, PageChallengeState, TransportProvenance};

/// Convert a browser snapshot into a fetch artifact for the existing extract pipeline.
pub fn snapshot_to_fetch_artifact(snapshot: &BrowserSnapshot) -> FetchArtifact {
    let body = BodyStorage::inline(snapshot.document_html.as_bytes().to_vec());
    let headers = BTreeMap::from([("content-type".into(), "text/html".into())]);
    FetchArtifact {
        request_url: snapshot.requested_url.clone(),
        final_url: snapshot.final_url.clone(),
        status: if snapshot.challenge_state == PageChallengeState::Normal { 200 } else { 503 },
        response_headers: headers,
        content_type: Some("text/html".into()),
        body,
        redirect_chain: Vec::new(),
        retrieved_at_epoch: snapshot.captured_at_epoch,
        transport_provenance: TransportProvenance::Test,
        cache_key: format!("browser:{}:{}", snapshot.profile_id, snapshot.final_url),
    }
}
