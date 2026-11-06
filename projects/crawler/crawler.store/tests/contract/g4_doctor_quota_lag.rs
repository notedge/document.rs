//! G-STORE-4：doctor 报告命名空间配额用量与 checkpoint 落后量。

use crawler_store::{PagePhase, RunSpec, Store, StoreConfig, YydbStore};
use crawler_types::{BodyStorage, FetchArtifact, TransportProvenance};
use url::Url;

use crate::contract::support::{HOST, LONG_LEASE, assert_both, cleanup_db, item, temp_db_path};

fn artifact(cache_key: &str) -> FetchArtifact {
    let url = Url::parse(cache_key).expect("url");
    FetchArtifact {
        request_url: url.clone(),
        final_url: url,
        status: 200,
        response_headers: Default::default(),
        content_type: Some("text/html".into()),
        body: BodyStorage::inline(b"quota-cached"),
        redirect_chain: Vec::new(),
        retrieved_at_epoch: 1,
        transport_provenance: TransportProvenance::Network,
        cache_key: cache_key.to_owned(),
    }
}

/// Quota upper bound: three writes into a two-record namespace evict the
/// least-recently-used entry and doctor reports the bounded usage.
#[test]
fn g4_doctor_reports_quota_bound_and_usage() {
    let path = temp_db_path("g4-quota");
    let mut store = YydbStore::open(StoreConfig::yydb(&path).with_claim_lease(LONG_LEASE)).expect("open yydb store");
    store.set_namespace_quota("cache/response/", 4096, 2).expect("set quota");

    for key in ["https://example.com/a", "https://example.com/b", "https://example.com/c"] {
        store.put_response_cache(&artifact(key)).expect("put response");
    }

    assert!(store.get_response_cache("https://example.com/a").expect("get a").is_none(), "oldest entry must be evicted");
    assert!(store.get_response_cache("https://example.com/c").expect("get c").is_some());

    let report = store.doctor().expect("doctor");
    let namespace = report
        .namespaces
        .iter()
        .find(|namespace| namespace.prefix == "cache/response/")
        .expect("cache namespace must be reported");
    assert_eq!(namespace.record_count, 2, "usage must stay inside the quota");
    assert_eq!(namespace.quota_max_bytes, Some(4096));
    assert!(namespace.bytes_used > 0);

    drop(store);
    cleanup_db(&path);
}

/// Checkpoint lag: commits outrun the last checkpoint, reopening still rebuilds
/// the run from native records, and exporting a checkpoint brings lag to zero.
#[test]
fn g4_checkpoint_lag_reports_and_recovery_keeps_committed_pages() {
    assert_both!("g4-lag", LONG_LEASE, |store| {
        store.begin_run(RunSpec::new("https://example.com/")).expect("begin run");
        store.enqueue(item("/a")).expect("enqueue a");
        store.enqueue(item("/b")).expect("enqueue b");
        let mut committed = Vec::new();
        for slot in ["a", "b"] {
            let claim = store.claim_next(HOST, "w1").expect("claim").expect("claim");
            store.begin_page_tx("w1").expect("begin tx");
            store.page_tx_put_fetch(&format!("fetch/{slot}"), slot.as_bytes()).expect("put fetch");
            store.page_tx_put_ir(&format!("ir/{slot}"), slot.as_bytes()).expect("put ir");
            store.commit_page_tx().expect("commit");
            committed.push(claim.item_id);
        }

        let lagging = store.doctor().expect("doctor");
        assert_eq!(lagging.checkpoint_lag_records, 2, "commits must outrun the last checkpoint");
        assert!(
            lagging.issues.iter().any(|issue| issue.code == "store.doctor.checkpoint_stale"),
            "stale checkpoint must be surfaced as a doctor issue"
        );

        store.reopen().expect("reopen");
        assert_eq!(store.event_count().expect("events"), 2, "recovery must not depend on a caught-up checkpoint");
        for page_id in &committed {
            assert_eq!(
                store.page_phase(page_id).expect("page phase"),
                PagePhase::Committed,
                "committed page must survive a lagging checkpoint"
            );
        }
        assert_eq!(store.doctor().expect("doctor after reopen").checkpoint_lag_records, 2);

        let sequence = store.save_checkpoint_view().expect("save checkpoint view");
        assert_eq!(sequence, 2);
        assert_eq!(store.doctor().expect("doctor after checkpoint").checkpoint_lag_records, 0);

        store.reopen().expect("reopen after checkpoint");
        assert_eq!(store.event_count().expect("events"), 2);
        assert_eq!(store.doctor().expect("doctor after second reopen").checkpoint_lag_records, 0);
    });
}
