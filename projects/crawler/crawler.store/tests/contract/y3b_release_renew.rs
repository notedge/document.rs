//! Y-3 release/renew extension: explicit lease end and heartbeat keep fencing.

use crawler_store::{ReleaseOutcome, RunSpec, StoreError};

use crate::contract::support::{HOST, LONG_LEASE, assert_both, item};

#[test]
fn y3_release_and_renew_keep_fencing_monotonic() {
    assert_both!("y3-release", LONG_LEASE, |store, backend| {
        store.begin_run(RunSpec { seed_url: "https://example.com/".into() }).expect("begin run");
        store.enqueue(item("/release")).expect("enqueue");

        let token = store.claim_next(HOST, "w1").expect("claim").expect("first claim");
        let renewed = store.renew_claim(&token).expect("renew");
        assert!(renewed > 0, "renew must return an expiry on {backend}");

        let stale = crawler_store::ClaimToken { fencing_token: 0, ..token.clone() };
        assert!(matches!(store.release_claim(&stale, ReleaseOutcome::Done), Err(StoreError::StoreFencingMismatch)));

        store.release_claim(&token, ReleaseOutcome::Requeue).expect("requeue");
        let retried = store.claim_next(HOST, "w2").expect("claim").expect("requeued item is claimable");
        assert_eq!(retried.item_id, token.item_id);
        assert!(retried.fencing_token > token.fencing_token, "requeue must advance fencing on {backend}");

        store.release_claim(&retried, ReleaseOutcome::Done).expect("done");
        assert!(store.claim_next(HOST, "w3").expect("claim").is_none(), "done item leaves the frontier on {backend}");
    });
}
