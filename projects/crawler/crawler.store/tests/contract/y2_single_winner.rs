//! Y-2 领取：同一 frontier 项只有一个赢家，同一 worker 重领是幂等的。

use crawler_store::RunSpec;

use crate::contract::support::{HOST, TEST_LEASE, assert_both, item};

#[test]
fn y2_claim_has_single_winner_and_is_idempotent_per_worker() {
    assert_both!("y2", TEST_LEASE, |store| {
        store.begin_run(RunSpec::new("https://example.com/")).expect("begin run");
        store.enqueue(item("/only")).expect("enqueue");

        let first = store.claim_next(HOST, "w1").expect("claim").expect("first claim");
        assert!(store.claim_next(HOST, "w2").expect("second claim").is_none(), "second worker must not win");
        let again = store.claim_next(HOST, "w1").expect("replay").expect("same worker replays claim");
        assert_eq!(again.item_id, first.item_id);
        assert_eq!(again.fencing_token, first.fencing_token, "replay must not bump fencing");
    });
}
