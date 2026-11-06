//! Store 冒烟：开库 + 响应缓存读写。复杂合同 / doctor / 并发用例在下游产品仓。

use crawler_store::{MemoryStore, Store, StoreConfig};
use crawler_types::{BodyStorage, FetchArtifact, TransportProvenance};
use url::Url;

fn sample() -> FetchArtifact {
    FetchArtifact {
        request_url: Url::parse("https://example.com/").expect("url"),
        final_url: Url::parse("https://example.com/").expect("url"),
        status: 200,
        response_headers: Default::default(),
        content_type: Some("text/html".into()),
        body: BodyStorage::inline(b"ok"),
        redirect_chain: Vec::new(),
        retrieved_at_epoch: 1,
        transport_provenance: TransportProvenance::Network,
        cache_key: "https://example.com/".into(),
    }
}

#[test]
fn memory_store_caches_a_response() {
    let mut store = MemoryStore::open(StoreConfig::memory()).expect("open");
    let artifact = sample();
    store.put_response_cache(&artifact).expect("put");
    let hit = store.get_response_cache(&artifact.cache_key).expect("get").expect("hit");
    assert_eq!(hit.body_bytes(), Some(b"ok".as_slice()));
}

#[cfg(feature = "yydb")]
#[test]
fn yydb_store_opens_and_caches_a_response() {
    use crawler_store::YydbStore;
    use std::time::{SystemTime, UNIX_EPOCH};

    let path = std::env::temp_dir().join(format!(
        "crawler-store-smoke-{}.yydx",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
    ));
    let mut store = YydbStore::open(StoreConfig::yydb(&path)).expect("open");
    let artifact = sample();
    store.put_response_cache(&artifact).expect("put");
    let hit = store.get_response_cache(&artifact.cache_key).expect("get").expect("hit");
    assert_eq!(hit.body_bytes(), Some(b"ok".as_slice()));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-lock", path.display()));
    let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("crawler-store-smoke");
    let _ = std::fs::remove_dir(path.with_file_name(format!("{stem}-objects")));
}
