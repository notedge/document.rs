//! Y-5 TTL 与重开：robots 缓存过期失效，响应缓存在 reopen 后仍命中。

use std::{thread, time::Duration};

use crawler_types::{BodyStorage, FetchArtifact, TransportProvenance};
use url::Url;

use crate::contract::support::{HOST, TEST_LEASE, assert_both};

const ROBOTS_TTL: Duration = Duration::from_millis(300);
const EXPIRY_WAIT: Duration = Duration::from_millis(400);

fn artifact() -> FetchArtifact {
    FetchArtifact {
        request_url: Url::parse("https://example.com/page").expect("url"),
        final_url: Url::parse("https://example.com/page").expect("url"),
        status: 200,
        response_headers: Default::default(),
        content_type: Some("text/html".into()),
        body: BodyStorage::inline(b"cached"),
        redirect_chain: Vec::new(),
        retrieved_at_epoch: 1,
        transport_provenance: TransportProvenance::Network,
        cache_key: "https://example.com/page".into(),
    }
}

#[test]
fn y5_robots_ttl_expires_and_response_cache_survives_reopen() {
    assert_both!("y5", TEST_LEASE, |store| {
        store.put_robots_cache(HOST, b"robots", ROBOTS_TTL).expect("put robots");
        assert_eq!(store.get_robots_cache(HOST).expect("get robots").as_deref(), Some(b"robots".as_slice()));
        thread::sleep(EXPIRY_WAIT);
        assert!(store.get_robots_cache(HOST).expect("get robots").is_none(), "expired robots must miss");

        let artifact = artifact();
        store.put_response_cache(&artifact).expect("put response");
        store.reopen().expect("reopen");
        let hit = store.get_response_cache(&artifact.cache_key).expect("get response").expect("cache hit after reopen");
        assert_eq!(hit.body_bytes(), Some(b"cached".as_slice()));
        assert_eq!(hit.transport_provenance, TransportProvenance::CacheHit);
    });
}
