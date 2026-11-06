//! G-PANDARK-4 检查点视图：记录是权威，JSON 只导出。

use crawler_store::{PagePhase, RunSpec};
use crawler_types::CrawlEvent;

use crate::contract::support::{HOST, LONG_LEASE, assert_both, item};

fn commit_one(store: &mut impl crawler_store::Store, slot: &str) {
    let claim = store.claim_next(HOST, "w1").expect("claim").expect("claimable item");
    store.begin_page_tx("w1").expect("begin tx");
    store.page_tx_put_fetch(&format!("fetch/{slot}"), slot.as_bytes()).expect("put fetch");
    store.page_tx_put_ir(&format!("ir/{slot}"), slot.as_bytes()).expect("put ir");
    store
        .page_tx_put_event(&CrawlEvent::Committed { page_identity: format!("https://example.com{slot}") })
        .expect("stage event");
    store.commit_page_tx().expect("commit");
    assert_eq!(store.page_phase(&claim.item_id).expect("page phase"), PagePhase::Committed);
}

#[test]
fn g6_checkpoint_view_is_derived_and_never_authoritative() {
    assert_both!("g6", LONG_LEASE, |store| {
        store.begin_run(RunSpec { seed_url: "https://example.com/".into() }).expect("begin run");
        store.enqueue(item("/a")).expect("enqueue a");
        assert!(store.checkpoint_view().expect("view").is_none(), "no view exists before an export");
        assert!(store.checkpoint_view_json().expect("view json").is_none());

        commit_one(store, "/a");
        let sequence = store.save_checkpoint_view().expect("save view");
        assert_eq!(sequence, 1, "the exported sequence matches the committed event count");
        let view = store.checkpoint_view().expect("view").expect("view exists");
        let expected_run = crawler_store::keyspace::run_id_from_seed("https://example.com/");
        assert_eq!(view.run_id, expected_run);
        assert_eq!(view.seed_url, "https://example.com/");
        assert_eq!(view.last_commit_sequence, 1);
        assert_eq!(view.committed_pages, 1);
        assert_eq!(view.pending_frontier, 1);
        assert_eq!(view.open_claims.len(), 1);
        assert_eq!(view.open_claims[0].item_id, crawler_store::keyspace::item_id_from_url("https://example.com/a"));
        assert_eq!(view.open_claims[0].worker_id, "w1");
        assert!(view.open_claims[0].lease_until_ms > 0);

        let json = store.checkpoint_view_json().expect("view json").expect("view json exists");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("view json parses");
        assert_eq!(parsed["last_commit_sequence"], 1);
        assert_eq!(parsed["committed_pages"], 1);

        // Export only: more commits land in records, not in the exported view.
        store.enqueue(item("/b")).expect("enqueue b");
        commit_one(store, "/b");
        assert_eq!(store.doctor().expect("doctor").checkpoint_lag_records, 1, "records outrun the exported view");
        let stale = store.checkpoint_view().expect("view").expect("view still exists");
        assert_eq!(stale.last_commit_sequence, 1, "the exported view does not advance on commit");
        assert_eq!(stale.committed_pages, 1);
        assert_eq!(store.event_count().expect("events"), 2, "records stay authoritative");

        store.reopen().expect("reopen");
        let reopened = store.checkpoint_view().expect("view").expect("export survives reopen");
        assert_eq!(reopened.last_commit_sequence, 1);
        assert_eq!(reopened.committed_pages, 1);
        assert_eq!(
            store.page_phase(&crawler_store::keyspace::item_id_from_url("https://example.com/b")).expect("page b"),
            PagePhase::Committed
        );

        let sequence = store.save_checkpoint_view().expect("save view again");
        assert_eq!(sequence, 2);
        let caught_up = store.checkpoint_view().expect("view").expect("view exists");
        assert_eq!(caught_up.last_commit_sequence, 2);
        assert_eq!(caught_up.committed_pages, 2);
        assert_eq!(caught_up.pending_frontier, 2);
        assert_eq!(caught_up.open_claims.len(), 2);
        assert_eq!(store.doctor().expect("doctor").checkpoint_lag_records, 0, "exporting brings lag to zero");
    });
}
