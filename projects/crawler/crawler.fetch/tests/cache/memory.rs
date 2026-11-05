//! Extracted from `src/cache.rs`.

use crawler_fetch::ResponseCache;
use crawler_types::{BodyStorage, FetchArtifact, TransportProvenance, now_epoch};
use url::Url;

#[test]
fn stores_and_retrieves_by_cache_key() {
    let mut cache = ResponseCache::new();
    let artifact = FetchArtifact {
        request_url: Url::parse("https://example.com/").expect("url"),
        final_url: Url::parse("https://example.com/").expect("url"),
        status: 200,
        response_headers: Default::default(),
        content_type: Some("text/html".into()),
        body: BodyStorage::inline(b"ok"),
        redirect_chain: Vec::new(),
        retrieved_at_epoch: now_epoch(),
        transport_provenance: TransportProvenance::Test,
        cache_key: "https://example.com/".into(),
    };
    cache.put(artifact);
    assert!(cache.get("https://example.com/").is_some());
}
