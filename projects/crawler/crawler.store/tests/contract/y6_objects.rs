//! Y-6 CAS 对象：提交批次同时落元数据与对象引用，重复内容去重，孤儿可回收。

use crawler_store::RunSpec;

use crate::contract::support::{HOST, LONG_LEASE, assert_both, item};

fn commit_shared_pages<S: crawler_store::Store>(store: &mut S) {
    store.begin_run(RunSpec { seed_url: "https://example.com/".into() }).expect("begin run");
    store.enqueue(item("/a")).expect("enqueue a");
    store.enqueue(item("/b")).expect("enqueue b");
    for _ in 0..2 {
        store.claim_next(HOST, "w1").expect("claim").expect("claim");
        store.begin_page_tx("w1").expect("begin tx");
        store.page_tx_put_fetch("fetch/shared", b"shared-body").expect("put fetch");
        store.page_tx_put_ir("ir/shared", b"shared-ir").expect("put ir");
        store.commit_page_tx().expect("commit");
    }
}

#[test]
fn y6_committed_pages_leave_no_orphans() {
    assert_both!("y6", LONG_LEASE, |store| {
        commit_shared_pages(store);
        assert_eq!(store.doctor().expect("doctor").orphan_object_count, 0);
    });
}

#[cfg(feature = "yydb")]
#[test]
fn y6_yydb_deduplicates_cas_objects_and_reclaims_orphans() {
    use crawler_store::{Store, StoreConfig, YydbStore};

    use crate::contract::support::{cleanup_db, temp_db_path};

    let path = temp_db_path("y6-objects");
    let mut store = YydbStore::open(StoreConfig::yydb(&path).with_claim_lease(LONG_LEASE)).expect("open yydb store");
    commit_shared_pages(&mut store);

    assert_eq!(store.object_count().expect("object count"), 2, "identical bytes must share one CAS object per slot");
    store.put_orphan_object(b"orphan").expect("put orphan");
    assert_eq!(store.object_count().expect("object count"), 3);

    let report = store.doctor().expect("doctor");
    assert_eq!(report.orphan_object_count, 1);
    assert!(report.issues.iter().any(|issue| issue.code == "store.doctor.orphan_object"));

    assert_eq!(store.reclaim_orphans().expect("reclaim"), 1);
    let after = store.doctor().expect("doctor after reclaim");
    assert_eq!(after.orphan_object_count, 0);
    assert!(after.issues.iter().all(|issue| issue.code != "yydb.doctor.missing_blob"), "reclaim must keep live refs");
    assert_eq!(store.object_count().expect("object count"), 2);

    drop(store);
    cleanup_db(&path);
}
