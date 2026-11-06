//! Y-2 领取：同一 frontier 项只有一个赢家，同一 worker 重领是幂等的。

use crawler_store::RunSpec;

use crate::contract::support::{HOST, TEST_LEASE, assert_both, item};

#[cfg(feature = "yydb")]
#[test]
fn y2_compare_exchange_has_a_single_winner() {
    use yydb::Connection;

    let conn = Connection::open_in_memory().expect("open in memory");
    let key = "run/cas/only";
    assert!(conn.compare_exchange(key, None, Some(b"first")).expect("first create"));
    assert!(
        !conn.compare_exchange(key, None, Some(b"second")).expect("second create"),
        "only one creator may win"
    );
    assert!(
        !conn.compare_exchange(key, Some(b"stale"), Some(b"third")).expect("stale value"),
        "stale value must not overwrite"
    );
    assert!(conn.compare_exchange(key, Some(b"first"), Some(b"winner")).expect("matched swap"));
    assert_eq!(conn.get(key).expect("get").as_deref(), Some(b"winner".as_slice()));
}

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
