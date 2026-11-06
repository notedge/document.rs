//! Y-7 重开：已提交页面与事件计数保留，已提交项不再被领取。

use crawler_store::{PagePhase, RunSpec};

use crate::contract::support::{HOST, LONG_LEASE, assert_both, item};

#[test]
fn y7_reopen_keeps_committed_page_and_skips_it() {
    assert_both!("y7", LONG_LEASE, |store| {
        store.begin_run(RunSpec { seed_url: "https://example.com/".into() }).expect("begin run");
        store.enqueue(item("/first")).expect("enqueue first");
        store.enqueue(item("/second")).expect("enqueue second");

        let first = store.claim_next(HOST, "w1").expect("claim").expect("first claim");
        store.begin_page_tx("w1").expect("begin tx");
        store.page_tx_put_fetch("fetch/first", b"first").expect("put fetch");
        store.page_tx_put_ir("ir/first", b"first-ir").expect("put ir");
        store.commit_page_tx().expect("commit");
        assert_eq!(store.event_count().expect("events"), 1);

        store.reopen().expect("reopen");
        assert_eq!(store.event_count().expect("events"), 1);
        assert_eq!(store.page_phase(&first.item_id).expect("phase"), PagePhase::Committed);

        let next = store.claim_next(HOST, "w1").expect("claim").expect("claim after reopen");
        assert_ne!(next.item_id, first.item_id, "committed item must stay terminal after reopen");
        store.begin_page_tx("w1").expect("begin tx");
        store.page_tx_put_fetch("fetch/second", b"second").expect("put fetch");
        store.page_tx_put_ir("ir/second", b"second-ir").expect("put ir");
        store.commit_page_tx().expect("commit");
        assert_eq!(store.event_count().expect("events"), 2);
    });
}
