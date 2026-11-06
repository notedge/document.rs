//! Y-1 页面事务：提交是单个批次，中止不留幽灵。

use crawler_store::{PagePhase, RunSpec, StoreError};

use crate::contract::support::{HOST, LONG_LEASE, assert_both, item};

#[test]
fn y1_page_commit_is_one_batch_and_abort_leaves_no_ghost() {
    assert_both!("y1", LONG_LEASE, |store| {
        store.begin_run(RunSpec::new("https://example.com/")).expect("begin run");
        store.enqueue(item("/a")).expect("enqueue a");
        store.enqueue(item("/b")).expect("enqueue b");

        let first = store.claim_next(HOST, "w1").expect("claim").expect("first claim");
        assert_eq!(store.page_phase("auto").expect("auto phase"), PagePhase::Claimed);
        let tx = store.begin_page_tx("w1").expect("begin tx");
        assert_eq!(tx.page_id, first.item_id);
        store.page_tx_put_fetch("fetch/a", b"alpha").expect("put fetch");
        store.page_tx_put_ir("ir/a", b"alpha-ir").expect("put ir");
        assert_eq!(store.page_phase("auto").expect("auto phase"), PagePhase::IrValidated);
        assert_eq!(store.event_count().expect("events"), 0);
        store.commit_page_tx().expect("commit");
        assert_eq!(store.page_phase("auto").expect("auto phase"), PagePhase::Committed);
        assert_eq!(store.event_count().expect("events"), 1);

        let second = store.claim_next(HOST, "w1").expect("claim").expect("second claim");
        assert_ne!(second.item_id, first.item_id, "committed item must not be re-claimed");
        store.begin_page_tx("w1").expect("begin tx");
        store.page_tx_put_fetch("fetch/b", b"beta").expect("put fetch");
        store.page_tx_put_ir("ir/b", b"beta-ir").expect("put ir");
        store.abort_page_tx().expect("abort");
        assert!(
            matches!(store.page_phase(&second.item_id), Err(StoreError::StoreNotFound(_))),
            "aborted page must not leave a visible record"
        );
        assert_eq!(store.event_count().expect("events"), 1, "abort must not append an event");
    });
}
