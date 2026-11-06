//! Y-4 前缀扫描：分页后仍能领取每一项且不重复。

use std::collections::BTreeSet;

use crawler_store::RunSpec;

use crate::contract::support::{HOST, LONG_LEASE, assert_both, item};

const ITEMS: usize = 20;

#[test]
fn y4_paged_scan_claims_every_item_exactly_once() {
    assert_both!("y4", LONG_LEASE, |store| {
        store.begin_run(RunSpec::new("https://example.com/")).expect("begin run");
        for index in 0..ITEMS {
            store.enqueue(item(&format!("/page-{index}"))).expect("enqueue");
        }

        let mut seen = BTreeSet::new();
        for _ in 0..ITEMS {
            let claim = store.claim_next(HOST, "w1").expect("claim").expect("claim");
            assert!(seen.insert(claim.item_id.clone()), "claim must not repeat an item");
            store.begin_page_tx("w1").expect("begin tx");
            store.page_tx_put_fetch("fetch/page", b"x").expect("put fetch");
            store.commit_page_tx().expect("commit");
        }
        assert_eq!(seen.len(), ITEMS);
        assert!(store.claim_next(HOST, "w1").expect("claim").is_none(), "frontier must be drained");
        assert_eq!(store.event_count().expect("events"), ITEMS as u64);
    });
}
