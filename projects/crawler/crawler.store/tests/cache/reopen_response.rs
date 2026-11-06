//! 响应缓存：reopen 后仍命中。

#[cfg(feature = "yydb")]
use crawler_store::YydbStore;
use crawler_store::{MemoryStore, Store, StoreConfig};
use crawler_types::{BodyStorage, FetchArtifact, TransportProvenance};
use url::Url;

fn sample_artifact() -> FetchArtifact {
    FetchArtifact {
        request_url: Url::parse("https://example.com/page").expect("url"),
        final_url: Url::parse("https://example.com/page").expect("url"),
        status: 200,
        response_headers: Default::default(),
        content_type: Some("text/html".into()),
        body: BodyStorage::inline(b"cached-body"),
        redirect_chain: Vec::new(),
        retrieved_at_epoch: 1,
        transport_provenance: TransportProvenance::Network,
        cache_key: "https://example.com/page".into(),
    }
}

fn assert_reopen_response_cache<S: Store>(make_store: impl FnOnce() -> S, reopen: fn(&mut S) -> crawler_store::Result<()>) {
    let mut store = make_store();
    let artifact = sample_artifact();
    store.put_response_cache(&artifact).expect("put response cache");
    reopen(&mut store).expect("reopen store");
    let hit = store.get_response_cache(&artifact.cache_key).expect("get response cache").expect("cache hit after reopen");
    assert_eq!(hit.body_bytes(), Some(b"cached-body".as_slice()));
    assert_eq!(hit.transport_provenance, TransportProvenance::CacheHit);
}

#[test]
fn reopen_keeps_response_cache_memory() {
    assert_reopen_response_cache(|| MemoryStore::open(StoreConfig::memory()).expect("open"), |store| store.reopen());
}

#[cfg(feature = "yydb")]
mod yydb_helpers {
    use std::{
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    pub fn temp_db_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        std::env::temp_dir().join(format!("crawler-store-{label}-{nonce}.yydx"))
    }

    pub fn cleanup_db(path: &Path) {
        let _ = std::fs::remove_file(path);
        for sidecar in ["-wal", "-shm", "-lock"] {
            let _ = std::fs::remove_file(format!("{}{sidecar}", path.display()));
        }
        let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("crawler-store");
        let _ = std::fs::remove_dir(path.with_file_name(format!("{stem}-objects")));
    }
}

#[cfg(feature = "yydb")]
#[test]
fn reopen_keeps_response_cache_yydb() {
    let path = yydb_helpers::temp_db_path("cache-reopen");
    assert_reopen_response_cache(|| YydbStore::open(StoreConfig::yydb(&path)).expect("open"), |store| store.reopen());
    yydb_helpers::cleanup_db(&path);
}
