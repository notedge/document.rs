//! Extracted from `src/disk_cache.rs`.

use std::{collections::BTreeMap, fs};

use crawler_fetch::{DiskResponseCache, LayeredResponseCache};
use crawler_types::{BodyStorage, FetchArtifact, TransportProvenance, now_epoch};
use url::Url;

fn sample_artifact(body: &[u8], cache_key: &str) -> FetchArtifact {
    FetchArtifact {
        request_url: Url::parse("https://example.com/").expect("url"),
        final_url: Url::parse("https://example.com/").expect("url"),
        status: 200,
        response_headers: BTreeMap::from([("etag".into(), "\"abc\"".into()), ("content-type".into(), "text/html".into())]),
        content_type: Some("text/html".into()),
        body: BodyStorage::inline(body),
        redirect_chain: Vec::new(),
        retrieved_at_epoch: now_epoch(),
        transport_provenance: TransportProvenance::Network,
        cache_key: cache_key.into(),
    }
}

#[test]
fn disk_cache_round_trips_inline_body() {
    let dir = std::env::temp_dir().join(format!("crawler-disk-cache-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let mut cache = DiskResponseCache::open(&dir).expect("open");
    let artifact = sample_artifact(b"hello", "https://example.com/|etag=\"abc\"");
    cache.put(artifact).expect("put");
    let loaded = cache.get("https://example.com/").expect("get").expect("hit");
    assert_eq!(loaded.body_bytes(), Some(b"hello".as_ref()));
    assert_eq!(loaded.transport_provenance, TransportProvenance::CacheHit);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn layered_cache_promotes_disk_hit_to_memory() {
    let dir = std::env::temp_dir().join(format!("crawler-layered-cache-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let disk = DiskResponseCache::open(&dir).expect("open");
    let mut layered = LayeredResponseCache::with_disk(disk);
    let artifact = sample_artifact(b"cached", "https://example.com/|etag=\"abc\"");
    layered.put(artifact).expect("put");
    let loaded = layered.get_by_request_url("https://example.com/").expect("get").expect("hit");
    assert_eq!(loaded.body_bytes(), Some(b"cached".as_ref()));
    let _ = fs::remove_dir_all(&dir);
}
