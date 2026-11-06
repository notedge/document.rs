//! G-PANDARK-2 fixtures: host-partitioned claims, stale fencing and reclaim.

use std::{thread, time::Duration};

use crawler::{HostPolicy, HostScheduler, StoreFrontier};
#[cfg(feature = "yydb")]
use crawler_store::YydbStore;
use crawler_store::{
    ClaimToken, MemoryStore, ReleaseOutcome, RunSpec, Store, StoreConfig, StoreError, keyspace::item_id_from_url,
};
use crawler_types::{FrontierItem, PolitenessProfile};
use url::Url;

const LEASE: Duration = Duration::from_millis(300);

fn item(url: &str, site: &str) -> FrontierItem {
    FrontierItem::seed(Url::parse(url).expect("url"), site.to_owned())
}

fn begin(store: &mut dyn Store, seed: &str) {
    store.begin_run(RunSpec { seed_url: seed.into() }).expect("begin run");
}

fn fast_scheduler() -> HostScheduler {
    HostScheduler::new(PolitenessProfile::Developer).with_empty_retry(Duration::from_millis(50))
}

#[test]
fn host_scheduler_partitions_claims_and_honours_concurrency() {
    let mut store = MemoryStore::open(StoreConfig::memory().with_claim_lease(LEASE)).expect("open");
    begin(&mut store, "https://a.test/");

    let scheduler = fast_scheduler()
        .with_host_policy("a.test", HostPolicy::new(1, Duration::ZERO))
        .with_host_policy("b.test", HostPolicy::new(1, Duration::ZERO));
    let mut frontier = StoreFrontier::new("w1", PolitenessProfile::Developer).with_scheduler(scheduler);
    frontier.enqueue(&mut store, item("https://a.test/one", "a.test")).expect("frontier enqueue a");
    frontier.enqueue(&mut store, item("https://b.test/two", "b.test")).expect("frontier enqueue b");

    let first = frontier.next(&mut store).expect("claim").expect("first host");
    let second = frontier.next(&mut store).expect("claim").expect("second host");
    assert_eq!(first.item_id, item_id_from_url("https://a.test/one"), "lowest host partition goes first");
    assert_eq!(second.item_id, item_id_from_url("https://b.test/two"), "the other host runs without waiting");
    assert!(frontier.next(&mut store).expect("claim").is_none(), "both hosts are at their concurrency cap");

    frontier.release_done(&mut store, &first).expect("done a");
    frontier.release_done(&mut store, &second).expect("done b");
    assert!(frontier.next(&mut store).expect("claim").is_none(), "done items leave the frontier");
}

#[test]
fn expired_claim_is_reclaimed_after_restart_and_stale_release_is_rejected() {
    let mut store = MemoryStore::open(StoreConfig::memory().with_claim_lease(LEASE)).expect("open");
    begin(&mut store, "https://a.test/");
    store.enqueue(item("https://a.test/one", "a.test")).expect("enqueue");

    let mut first = StoreFrontier::new("w1", PolitenessProfile::Developer).with_scheduler(fast_scheduler());
    first.enqueue(&mut store, item("https://a.test/one", "a.test")).expect("frontier enqueue");
    let stale = first.next(&mut store).expect("claim").expect("first claim");
    drop(first);

    store.reopen().expect("restart");

    let mut resumed = StoreFrontier::new("w2", PolitenessProfile::Developer).with_scheduler(fast_scheduler());
    resumed.enqueue(&mut store, item("https://a.test/one", "a.test")).expect("resume enqueue");
    assert!(resumed.next(&mut store).expect("claim").is_none(), "the live lease blocks a second worker");

    thread::sleep(LEASE + Duration::from_millis(250));
    let fresh = resumed.next(&mut store).expect("claim").expect("expired claim is reclaimed");
    assert_eq!(fresh.item_id, stale.item_id);
    assert!(fresh.fencing_token > stale.fencing_token, "reclaim advances fencing");
    resumed.release_done(&mut store, &fresh).expect("fresh worker finishes");
    drop(resumed);

    assert!(
        matches!(store.release_claim(&stale, ReleaseOutcome::Done), Err(StoreError::StoreFencingMismatch)),
        "the stale worker cannot release after reclaim"
    );
}

#[cfg(feature = "yydb")]
#[test]
fn yydb_expired_claim_is_reclaimed_after_reopen() {
    let path = temp_db_path("scheduler");
    let mut store = YydbStore::open(StoreConfig::yydb(&path).with_claim_lease(LEASE)).expect("open");
    begin(&mut store, "https://a.test/");
    let mut first = StoreFrontier::new("w1", PolitenessProfile::Developer).with_scheduler(fast_scheduler());
    first.enqueue(&mut store, item("https://a.test/one", "a.test")).expect("frontier enqueue");
    let stale = first.next(&mut store).expect("claim").expect("first claim");
    drop(first);

    store.reopen().expect("restart");
    let mut resumed = StoreFrontier::new("w2", PolitenessProfile::Developer).with_scheduler(fast_scheduler());
    resumed.enqueue(&mut store, item("https://a.test/one", "a.test")).expect("resume enqueue");

    thread::sleep(LEASE + Duration::from_millis(250));
    let fresh: ClaimToken = resumed.next(&mut store).expect("claim").expect("expired claim is reclaimed");
    assert_eq!(fresh.item_id, stale.item_id);
    assert!(fresh.fencing_token > stale.fencing_token, "reclaim advances fencing");
    resumed.release_done(&mut store, &fresh).expect("finish");
    drop(resumed);
    drop(store);
    cleanup_db(&path);
}

#[cfg(feature = "yydb")]
fn temp_db_path(label: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    std::env::temp_dir().join(format!("crawler-scheduler-{label}-{}-{nonce}.yydb", std::process::id()))
}

#[cfg(feature = "yydb")]
fn cleanup_db(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
}
