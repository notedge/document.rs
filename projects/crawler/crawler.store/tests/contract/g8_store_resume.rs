//! G-PANDARK-8：store-backed resume 取代 JSON checkpoint。

use std::{thread, time::Duration};

use crawler_store::{ReleaseOutcome, RunSpec, Store};
use crawler_types::{CrawlEvent, CrawlRequest};

use crate::contract::support::{HOST, LONG_LEASE, TEST_LEASE, assert_both, item};

#[test]
fn g8_resume_rebuilds_state_from_records() {
    assert_both!("g8-resume", LONG_LEASE, |store| {
        let request = CrawlRequest::from_seed("https://example.com/").expect("request");
        let handle = store
            .begin_run(RunSpec {
                seed_url: "https://example.com/".into(),
                request: Some(request),
                strategy_fingerprint: Some("browser_fallback=Never".into()),
            })
            .expect("begin run");
        store.enqueue(item("/a")).expect("enqueue a");
        store.enqueue(item("/b")).expect("enqueue b");
        let claim = store.claim_next(HOST, "w1").expect("claim").expect("first claim");
        store.begin_page_tx("w1").expect("begin tx");
        store.page_tx_put_event(&CrawlEvent::Committed { page_identity: "https://example.com/a".into() }).expect("event");
        store.commit_page_tx().expect("commit");
        store.release_claim(&claim, ReleaseOutcome::Done).expect("release");
        assert_eq!(store.pending_frontier_count().expect("frontier"), 1);

        store.reopen().expect("reopen");
        let resume = store.resume(&handle.run_id).expect("resume");
        assert_eq!(resume.run_id, handle.run_id);
        assert_eq!(resume.seed_url, "https://example.com/");
        assert_eq!(resume.request.expect("stored request").seeds[0].url.as_str(), "https://example.com/");
        assert_eq!(resume.strategy_fingerprint.as_deref(), Some("browser_fallback=Never"));
        assert_eq!(resume.committed_pages, 1);
        assert_eq!(resume.pending_frontier, 1);
        assert_eq!(resume.last_commit_sequence, store.event_count().expect("events"));
        assert!(resume.open_claims.is_empty());
        assert_eq!(store.active_run_id().expect("active run").as_deref(), Some(handle.run_id.as_str()));

        let next = store.claim_next(HOST, "w2").expect("claim").expect("frontier survives resume");
        assert_eq!(next.item_id, crawler_store::keyspace::item_id_from_url("https://example.com/b"));
    });
}

#[test]
fn g8_resume_reclaims_expired_claims() {
    assert_both!("g8-resume-expired", TEST_LEASE, |store| {
        let request = CrawlRequest::from_seed("https://example.com/").expect("request");
        let handle = store
            .begin_run(RunSpec { seed_url: "https://example.com/".into(), request: Some(request), strategy_fingerprint: None })
            .expect("begin run");
        store.enqueue(item("/a")).expect("enqueue a");
        let claim = store.claim_next(HOST, "w1").expect("claim").expect("first claim");
        thread::sleep(TEST_LEASE + Duration::from_millis(150));
        store.reopen().expect("reopen");

        let resume = store.resume(&handle.run_id).expect("resume");
        assert!(resume.open_claims.is_empty(), "expired claims must be reclaimed");
        let reclaimed = store.claim_next(HOST, "w2").expect("claim").expect("expired item is claimable");
        assert_eq!(reclaimed.item_id, claim.item_id);
        assert_ne!(reclaimed.fencing_token, claim.fencing_token, "reclaim must issue a fresh fencing token");
    });
}
