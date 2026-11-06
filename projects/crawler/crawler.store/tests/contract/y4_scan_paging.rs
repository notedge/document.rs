//! Y-4 前缀扫描：分页后仍能领取每一项且不重复。

use std::collections::BTreeSet;

use crawler_store::RunSpec;

use crate::contract::support::{HOST, LONG_LEASE, assert_both, item};

const ITEMS: usize = 20;

#[cfg(feature = "yydb")]
#[test]
fn y4_prefix_scan_pages_ten_thousand_records() {
    use yydb::{Batch, Connection};

    const KEYS: usize = 10_000;
    const PAGE: usize = 128;
    let conn = Connection::open_in_memory().expect("open in memory");
    let mut batch = Batch::new();
    for index in 0..KEYS {
        batch.put(format!("run/scale/frontier/{index:05}"), format!("{index}"));
    }
    batch.commit(&conn).expect("commit batch");

    let mut seen = BTreeSet::new();
    let mut after: Option<String> = None;
    loop {
        let page = conn.scan_prefix("run/scale/frontier/", after.as_deref(), PAGE).expect("scan page");
        assert!(page.len() <= PAGE, "page must respect the limit");
        let Some(last_key) = page.last().map(|(key, _)| key.clone())
        else {
            break;
        };
        for (key, _) in page {
            assert!(seen.insert(key), "paged scan must not repeat a record");
        }
        after = Some(last_key);
        if seen.len() == KEYS {
            break;
        }
    }
    assert_eq!(seen.len(), KEYS, "paged scan must recover every record");
}

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
