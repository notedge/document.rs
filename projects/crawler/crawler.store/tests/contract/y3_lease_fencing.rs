//! Y-3 租约与栅栏：陈旧 token 被拒，过期 claim 可被重新领取。

use std::{thread, time::Duration};

use crawler_store::{RunSpec, StoreError};

use crate::contract::support::{HOST, TEST_LEASE, assert_both, item};

/// Longer than `TEST_LEASE` so the claim has certainly expired.
const EXPIRY_WAIT: Duration = Duration::from_millis(600);

#[test]
fn y3_stale_fencing_is_rejected_and_expired_claims_are_reclaimed() {
    assert_both!("y3", TEST_LEASE, |store, backend| {
        store.begin_run(RunSpec { seed_url: "https://example.com/".into() }).expect("begin run");
        store.enqueue(item("/lease")).expect("enqueue");

        let stale = store.claim_next(HOST, "w1").expect("claim").expect("first claim");
        store.second_worker_claim("w2").expect("competing token");
        assert!(
            matches!(store.begin_page_tx("w2"), Err(StoreError::StoreFencingMismatch)),
            "stale fencing token must be rejected"
        );

        thread::sleep(EXPIRY_WAIT);
        let fresh = store.claim_next(HOST, "w2").expect("claim").expect("expired claim reclaimed");
        assert_eq!(fresh.item_id, stale.item_id);
        assert!(
            fresh.fencing_token > stale.fencing_token,
            "reclaim must advance fencing on {backend}: stale={} fresh={}",
            stale.fencing_token,
            fresh.fencing_token
        );
        store.begin_page_tx("w2").expect("fresh token begins tx");
        store.page_tx_put_fetch("fetch/lease", b"lease").expect("put fetch");
        store.commit_page_tx().expect("commit");
        assert_eq!(store.event_count().expect("events"), 1);
    });
}
