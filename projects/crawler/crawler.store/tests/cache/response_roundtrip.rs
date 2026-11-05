//! Extracted from `src/response_cache.rs`.

#![cfg(feature = "yydb")]

use crawler_types::{BodyStorage, FetchArtifact, TransportProvenance};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

use crawler_store::YydbResponseCache;

#[test]
fn round_trips_artifact() {
    let path = std::env::temp_dir().join(format!(
        "crawler-store-cache-{}.yydb",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
    ));
    let cache = YydbResponseCache::open(&path).expect("cache");
    let artifact = FetchArtifact {
        request_url: Url::parse("https://example.com/").unwrap(),
        final_url: Url::parse("https://example.com/").unwrap(),
        status: 200,
        response_headers: Default::default(),
        content_type: Some("text/html".into()),
        body: BodyStorage::inline(b"ok"),
        redirect_chain: Vec::new(),
        retrieved_at_epoch: 1,
        transport_provenance: TransportProvenance::Network,
        cache_key: "https://example.com/".into(),
    };
    cache.put(&artifact).expect("put");
    let hit = cache.get(&artifact.cache_key).expect("get").expect("hit");
    assert_eq!(hit.body_bytes(), Some(b"ok".as_slice()));
    let _ = std::fs::remove_file(path);
}
