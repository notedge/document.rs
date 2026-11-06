//! G-PANDARK-3 页面事务：资产、链接与事件同页提交，过期租约不能提交。

use std::{thread, time::Duration};

use crawler_store::{PageAssetBinding, PagePhase, RunSpec, StoreError};
use crawler_types::CrawlEvent;

use crate::contract::support::{HOST, LONG_LEASE, TEST_LEASE, assert_both, item};

fn binding() -> PageAssetBinding {
    PageAssetBinding {
        asset_id: "7".into(),
        fingerprint: "fp-7".into(),
        media_type: Some("image/png".into()),
        bytes: vec![1, 2, 3],
    }
}

#[test]
fn g5_page_tx_assets_links_and_events_are_atomic() {
    assert_both!("g5", LONG_LEASE, |store| {
        store.begin_run(RunSpec { seed_url: "https://example.com/".into() }).expect("begin run");
        store.enqueue(item("/a")).expect("enqueue a");
        let claim = store.claim_next(HOST, "w1").expect("claim").expect("first claim");

        store.begin_page_tx("w1").expect("begin tx");
        store.page_tx_put_fetch("fetch/a", b"alpha").expect("put fetch");
        store.page_tx_put_ir("ir/a", b"alpha-ir").expect("put ir");
        store.page_tx_bind_asset(&binding()).expect("bind asset");
        assert_eq!(store.page_phase("auto").expect("phase"), PagePhase::AssetsBound);
        store.page_tx_admit_links(vec![item("/b")]).expect("admit link");
        store.page_tx_put_event(&CrawlEvent::Committed { page_identity: "https://example.com/a".into() }).expect("stage event");
        assert_eq!(store.pending_frontier_count().expect("frontier"), 1, "admitted link stays invisible until commit");
        store.abort_page_tx().expect("abort");
        assert!(
            matches!(store.page_phase(&claim.item_id), Err(StoreError::StoreNotFound(_))),
            "aborted page must not leave a visible record"
        );
        assert_eq!(store.pending_frontier_count().expect("frontier"), 1, "abort must not admit links");
        assert_eq!(store.event_count().expect("events"), 0, "abort must not append events");

        store.begin_page_tx("w1").expect("begin tx again");
        store.page_tx_put_fetch("fetch/a", b"alpha").expect("put fetch");
        store.page_tx_put_ir("ir/a", b"alpha-ir").expect("put ir");
        store.page_tx_bind_asset(&binding()).expect("bind asset");
        store.page_tx_admit_links(vec![item("/b")]).expect("admit link");
        store.page_tx_put_event(&CrawlEvent::Committed { page_identity: "https://example.com/a".into() }).expect("stage event");
        store.commit_page_tx().expect("commit");
        assert_eq!(store.page_phase(&claim.item_id).expect("page"), PagePhase::Committed);
        assert_eq!(store.page_asset_refs(&claim.item_id).expect("assets"), vec!["fp-7".to_string()]);
        assert_eq!(store.pending_frontier_count().expect("frontier"), 2, "the committed page admits its link");
        assert_eq!(store.event_count().expect("events"), 1);

        let admitted = store.claim_next(HOST, "w1").expect("claim").expect("admitted link is claimable");
        assert_eq!(admitted.item_id, crawler_store::keyspace::item_id_from_url("https://example.com/b"));
    });
}

#[test]
fn g5_expired_lease_cannot_commit_page_tx() {
    assert_both!("g5-expired", TEST_LEASE, |store| {
        store.begin_run(RunSpec { seed_url: "https://example.com/".into() }).expect("begin run");
        store.enqueue(item("/a")).expect("enqueue a");
        let claim = store.claim_next(HOST, "w1").expect("claim").expect("first claim");

        store.begin_page_tx("w1").expect("begin tx");
        store.page_tx_put_fetch("fetch/a", b"alpha").expect("put fetch");
        store.page_tx_put_ir("ir/a", b"alpha-ir").expect("put ir");
        thread::sleep(TEST_LEASE + Duration::from_millis(250));
        assert!(matches!(store.commit_page_tx(), Err(StoreError::StoreLeaseExpired)), "an expired lease must not commit");
        assert!(
            !matches!(store.page_phase(&claim.item_id), Ok(PagePhase::Committed)),
            "a rejected commit must not expose a committed page"
        );
        assert_eq!(store.pending_frontier_count().expect("frontier"), 1);
    });
}
