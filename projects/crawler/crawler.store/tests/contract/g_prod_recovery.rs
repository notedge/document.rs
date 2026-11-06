//! G-PROD 恢复套件：crash / reopen / 重复 claim / lease 过期 / quota / orphan。
//!
//! 每个用例都在 MemoryStore 与 YydbStore 上跑同一套断言，除了 backend 原生
//! 能力差异（Memory 没有独立 CAS 根）用 `$backend` 显式分支。

use std::{thread, time::Duration};

use crawler_store::{PagePhase, ReleaseOutcome, RunSpec, Store};
use crawler_types::{BodyStorage, CrawlEvent, CrawlRequest, FetchArtifact, TransportProvenance};
use url::Url;

use crate::contract::support::{HOST, LONG_LEASE, TEST_LEASE, assert_both, item};

fn page_id(path: &str) -> String {
    crawler_store::keyspace::item_id_from_url(&format!("https://example.com{path}"))
}

fn artifact(key: &str, epoch: u64, body: &[u8]) -> FetchArtifact {
    let url = Url::parse("https://example.com/").expect("url");
    FetchArtifact {
        request_url: url.clone(),
        final_url: url,
        status: 200,
        response_headers: Default::default(),
        content_type: Some("text/html".into()),
        body: BodyStorage::Inline(body.to_vec()),
        redirect_chain: Vec::new(),
        retrieved_at_epoch: epoch,
        transport_provenance: TransportProvenance::Test,
        cache_key: key.into(),
    }
}

fn response_namespace(store: &dyn Store) -> Option<crawler_store::StoreNamespaceStats> {
    store.doctor().expect("doctor").namespaces.into_iter().find(|entry| entry.prefix == "cache/response/")
}

#[test]
fn g_prod_crash_reopen_keeps_committed_and_frontier() {
    assert_both!("prod-reopen", LONG_LEASE, |store| {
        let request = CrawlRequest::from_seed("https://example.com/").expect("request");
        let handle = store
            .begin_run(RunSpec {
                seed_url: "https://example.com/".into(),
                request: Some(request),
                strategy_fingerprint: Some("browser_fallback=Never".into()),
            })
            .expect("begin");
        store.enqueue(item("/a")).expect("enqueue a");
        store.enqueue(item("/b")).expect("enqueue b");
        let claim = store.claim_next(HOST, "w1").expect("claim").expect("first");
        store.begin_page_tx("w1").expect("tx");
        store.page_tx_put_event(&CrawlEvent::Committed { page_identity: "https://example.com/a".into() }).expect("event");
        store.commit_page_tx().expect("commit");
        store.release_claim(&claim, ReleaseOutcome::Done).expect("release");

        store.reopen().expect("reopen");
        let resume = store.resume(&handle.run_id).expect("resume");
        assert_eq!(resume.committed_pages, 1, "committed pages survive reopen");
        assert_eq!(resume.pending_frontier, 1, "frontier survives reopen");
        assert_eq!(resume.last_commit_sequence, store.event_count().expect("events"));
        assert!(resume.pending_hosts.contains(&HOST.into()), "host partitions rebuild");

        let next = store.claim_next(HOST, "w2").expect("claim").expect("frontier item is claimable");
        assert_eq!(next.item_id, page_id("/b"));
    });
}

#[test]
fn g_prod_duplicate_claim_has_one_winner() {
    assert_both!("prod-claim", TEST_LEASE, |store| {
        store.begin_run(RunSpec::new("https://example.com/")).expect("begin");
        store.enqueue(item("/only")).expect("enqueue");
        let first = store.claim_next(HOST, "w1").expect("claim").expect("winner");
        assert!(store.claim_next(HOST, "w2").expect("claim").is_none(), "second worker must not win");
        let replay = store.claim_next(HOST, "w1").expect("replay").expect("same worker replay");
        assert_eq!(replay.fencing_token, first.fencing_token, "replay must not bump fencing");
        store.release_claim(&first, ReleaseOutcome::Done).expect("release");
        assert!(store.claim_next(HOST, "w2").expect("claim").is_none(), "done item leaves the frontier");
    });
}

#[test]
fn g_prod_lease_expiry_reclaims_with_a_new_fence() {
    assert_both!("prod-lease", TEST_LEASE, |store| {
        let request = CrawlRequest::from_seed("https://example.com/").expect("request");
        let handle = store
            .begin_run(RunSpec { seed_url: "https://example.com/".into(), request: Some(request), strategy_fingerprint: None })
            .expect("begin");
        store.enqueue(item("/a")).expect("enqueue");
        let stale = store.claim_next(HOST, "w1").expect("claim").expect("first");
        thread::sleep(TEST_LEASE + Duration::from_millis(150));
        store.reopen().expect("reopen");

        let resume = store.resume(&handle.run_id).expect("resume");
        assert!(resume.open_claims.is_empty(), "expired claim is reclaimed");
        assert_eq!(resume.pending_frontier, 1);

        let fresh = store.claim_next(HOST, "w2").expect("claim").expect("reclaimed item");
        assert_eq!(fresh.item_id, stale.item_id);
        assert_ne!(fresh.fencing_token, stale.fencing_token, "reclaim issues a new fence");
        assert!(store.release_claim(&stale, ReleaseOutcome::Done).is_err(), "stale fence is rejected");

        store.begin_page_tx("w2").expect("tx");
        store.page_tx_put_fetch("fetch/after-lease", b"after-lease").expect("fetch");
        store.commit_page_tx().expect("commit");
        assert_eq!(store.page_phase(&page_id("/a")).expect("phase"), PagePhase::Committed);
    });
}

#[test]
fn g_prod_quota_bounds_the_response_cache() {
    assert_both!("prod-quota", LONG_LEASE, |store, backend| {
        store.set_namespace_quota("cache/response/", u64::MAX, u64::MAX).expect("open quota");
        store.put_response_cache(&artifact("cache/one", 1, b"one")).expect("put one");
        store.put_response_cache(&artifact("cache/two", 2, b"two")).expect("put two");
        store.put_response_cache(&artifact("cache/three", 3, b"three")).expect("put three");
        assert_eq!(response_namespace(&*store).expect("namespace").record_count, 3);

        store.set_namespace_quota("cache/response/", 1_000_000, 2).expect("quota");
        store.put_response_cache(&artifact("cache/four", 4, b"four")).expect("put four");
        let stats = response_namespace(&*store).expect("namespace");
        assert!(stats.record_count <= 2, "quota caps records on {backend}");
        assert!(stats.bytes_used <= 1_000_000, "quota caps bytes on {backend}");
        assert_eq!(stats.quota_max_bytes, Some(1_000_000));
        assert!(store.get_response_cache("cache/one").expect("get").is_none(), "oldest entry is evicted on {backend}");
        assert!(store.get_response_cache("cache/four").expect("get").is_some(), "newest entry stays on {backend}");

        store.reopen().expect("reopen");
        let stats = response_namespace(&*store).expect("namespace after reopen");
        assert!(stats.record_count <= 2, "quota survives reopen");
        assert_eq!(stats.quota_max_bytes, Some(1_000_000));
    });
}

#[test]
fn g_prod_orphan_reclaim_matches_doctor() {
    assert_both!("prod-orphan", LONG_LEASE, |store, backend| {
        store.begin_run(RunSpec::new("https://example.com/")).expect("begin");
        store.enqueue(item("/live")).expect("enqueue");
        let claim = store.claim_next(HOST, "w1").expect("claim").expect("claim");
        store.begin_page_tx("w1").expect("tx");
        store.page_tx_put_fetch("fetch/live", b"live-body").expect("fetch");
        store.commit_page_tx().expect("commit");
        store.release_claim(&claim, ReleaseOutcome::Done).expect("release");

        store.put_orphan_object(b"orphan-a").expect("orphan a");
        store.put_orphan_object(b"orphan-b").expect("orphan b");
        let orphans_before = store.doctor().expect("doctor").orphan_object_count;
        if backend == "yydb" {
            assert_eq!(orphans_before, 2, "yydb CAS root keeps unreferenced blobs");
        }
        let reclaimed = store.reclaim_orphans().expect("reclaim");
        assert_eq!(reclaimed, orphans_before, "reclaim reports what doctor counted on {backend}");
        assert_eq!(store.doctor().expect("doctor").orphan_object_count, 0);

        let refs = store.page_object_refs(&page_id("/live")).expect("refs");
        assert!(!refs.is_empty());
        assert!(store.get_object(&refs[0].fingerprint).expect("object").is_some(), "live blob survives reclaim");
    });
}

#[test]
fn g_prod_crash_mid_page_tx_is_discarded() {
    assert_both!("prod-crash", TEST_LEASE, |store| {
        let request = CrawlRequest::from_seed("https://example.com/").expect("request");
        let handle = store
            .begin_run(RunSpec { seed_url: "https://example.com/".into(), request: Some(request), strategy_fingerprint: None })
            .expect("begin");
        store.enqueue(item("/a")).expect("enqueue");
        let claim = store.claim_next(HOST, "w1").expect("claim").expect("claim");
        store.begin_page_tx("w1").expect("tx");
        store.page_tx_put_fetch("fetch/never-committed", b"never").expect("fetch");

        store.reopen().expect("reopen");
        assert_ne!(store.page_phase(&page_id("/a")).ok(), Some(PagePhase::Committed), "uncommitted page never lands");
        assert_eq!(store.pending_frontier_count().expect("frontier"), 1);

        thread::sleep(TEST_LEASE + Duration::from_millis(150));
        let resume = store.resume(&handle.run_id).expect("resume");
        assert!(resume.open_claims.is_empty(), "crashed claim expires and is reclaimed");
        assert!(resume.committed_pages == 0);
        let fresh = store.claim_next(HOST, "w2").expect("claim").expect("reclaim");
        assert_eq!(fresh.item_id, claim.item_id);
        store.begin_page_tx("w2").expect("tx");
        store.page_tx_put_fetch("fetch/retry", b"retry").expect("fetch");
        store.commit_page_tx().expect("commit");
        assert_eq!(store.page_phase(&page_id("/a")).expect("phase"), PagePhase::Committed);
        assert!(!store.page_object_refs(&page_id("/a")).expect("refs").is_empty(), "only committed objects attach");
    });
}

#[test]
fn g_prod_memory_and_yydb_report_the_same_contract_surface() {
    assert_both!("prod-surface", LONG_LEASE, |store, backend| {
        store.set_namespace_quota("cache/response/", u64::MAX, u64::MAX).expect("quota");
        let report = store.doctor().expect("doctor");
        assert!(
            report.namespaces.iter().any(|entry| entry.prefix == "cache/response/"),
            "cache namespace must be reported on {backend}"
        );
        assert!(report.issues.is_empty(), "doctor must stay quiet on {backend}");

        store.reopen().expect("reopen");
        let report = store.doctor().expect("doctor after reopen");
        assert!(
            report.namespaces.iter().any(|entry| entry.prefix == "cache/response/"),
            "cache namespace must survive reopen on {backend}"
        );
    });
}
