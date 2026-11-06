//! G-PANDARK-5：大对象进 ObjectStore，按 content fingerprint 去重。

use crawler_store::{ObjectKind, PageAssetBinding, PagePhase, RunSpec};
use crawler_types::CrawlEvent;

use crate::contract::support::{HOST, LONG_LEASE, assert_both, item};

#[cfg(feature = "yydb")]
use crate::contract::support::temp_db_path;

#[cfg(feature = "yydb")]
#[test]
fn g7_legacy_yydb_paths_are_rejected_with_a_migration_message() {
    use crawler_store::{Store, StoreConfig, YydbStore};

    let legacy = temp_db_path("g7-legacy").with_extension("yydb");
    let error = match YydbStore::open(StoreConfig::yydb(&legacy)) {
        Ok(_) => panic!("legacy single-file layout must be rejected"),
        Err(error) => error,
    };
    let message = error.to_string();
    assert!(message.contains("read-only"), "legacy policy must be explicit: {message}");
    assert!(message.contains(".yydx"), "the error must name the replacement layout: {message}");
    assert!(!legacy.exists(), "opening must not create or silently rewrite the legacy file");
}

#[test]
fn g7_object_store_dedupes_bytes_by_fingerprint() {
    assert_both!("g7", LONG_LEASE, |store| {
        let payload = b"shared-asset-bytes";
        let first = store.put_object(ObjectKind::AssetBytes, Some("image/png"), payload).expect("put asset");
        let second =
            store.put_object(ObjectKind::Attachment, Some("application/octet-stream"), payload).expect("put duplicate");
        assert_eq!(first.fingerprint, second.fingerprint, "identical bytes must share one fingerprint");
        assert_eq!(second.kind, ObjectKind::AssetBytes, "the first metadata record stays authoritative");
        assert_eq!(second.media_type.as_deref(), Some("image/png"));
        assert_eq!(first.size, payload.len() as u64);

        let stored = store.object_ref(&first.fingerprint).expect("object ref").expect("object metadata exists");
        assert_eq!(stored, first);
        assert_eq!(store.get_object(&first.fingerprint).expect("object bytes"), Some(payload.to_vec()));
        assert!(store.get_object("missing-fingerprint").expect("missing bytes").is_none());
        assert!(store.object_ref("missing-fingerprint").expect("missing ref").is_none());

        store.reopen().expect("reopen");
        assert_eq!(store.get_object(&first.fingerprint).expect("object survives reopen"), Some(payload.to_vec()));
    });
}

#[test]
fn g7_large_objects_survive_reopen_on_yydx_layout() {
    assert_both!("g7-large", LONG_LEASE, |store| {
        let large: Vec<u8> = (0..(8 * 1024 + 3)).map(|index| (index % 251) as u8).collect();
        let first =
            store.put_object(ObjectKind::Attachment, Some("application/octet-stream"), &large).expect("large object write");
        let second = store.put_object(ObjectKind::Attachment, None, &large).expect("duplicate large write");
        assert_eq!(first.fingerprint, second.fingerprint, "large bytes deduplicate by fingerprint");
        assert_eq!(first.size, large.len() as u64, "payload exceeds the 4 KiB single-file inline limit");

        store.begin_run(RunSpec::new("https://example.com/")).expect("begin run");
        store.enqueue(item("/big")).expect("enqueue big");
        let claim = store.claim_next(HOST, "w1").expect("claim").expect("claimable item");
        store.begin_page_tx("w1").expect("begin tx");
        store.page_tx_put_fetch("fetch/big", &large).expect("put large fetch");
        store.page_tx_put_ir("ir/big", b"{\"pages\":1}").expect("put ir");
        store
            .page_tx_bind_asset(&PageAssetBinding {
                asset_id: "asset/big".into(),
                fingerprint: "logical".into(),
                media_type: Some("application/octet-stream".into()),
                bytes: large.clone(),
            })
            .expect("bind large asset");
        store.commit_page_tx().expect("commit large page");
        let refs = store.page_object_refs(&claim.item_id).expect("page object refs");
        assert_eq!(refs[0].size, large.len() as u64);
        assert_eq!(refs[2].size, large.len() as u64);
        assert_eq!(store.get_object(&refs[0].fingerprint).expect("fetch bytes"), Some(large.clone()));
        store.release_claim(&claim, crawler_store::ReleaseOutcome::Done).expect("release claim");

        store.reopen().expect("reopen");
        assert_eq!(store.get_object(&first.fingerprint).expect("object after reopen"), Some(large.clone()));
        assert_eq!(store.get_object(&refs[0].fingerprint).expect("page fetch after reopen"), Some(large.clone()));
        assert_eq!(store.get_object(&refs[2].fingerprint).expect("page asset after reopen"), Some(large));
        let report = store.doctor().expect("doctor");
        assert_eq!(report.orphan_object_count, 0, "staged objects stay referenced after reopen");
    });
}

#[test]
fn g7_page_objects_route_through_the_object_store() {
    assert_both!("g7-page", LONG_LEASE, |store| {
        store.begin_run(RunSpec::new("https://example.com/")).expect("begin run");
        store.enqueue(item("/a")).expect("enqueue a");
        store.enqueue(item("/b")).expect("enqueue b");

        let mut fetch_fingerprints = Vec::new();
        let mut asset_fingerprints = Vec::new();
        for slot in ["/a", "/b"] {
            let claim = store.claim_next(HOST, "w1").expect("claim").expect("claimable item");
            store.begin_page_tx("w1").expect("begin tx");
            store.page_tx_put_fetch("fetch/shared", b"shared-fetch-bytes").expect("put fetch");
            store.page_tx_put_ir(&format!("ir{slot}"), slot.as_bytes()).expect("put ir");
            store
                .page_tx_bind_asset(&PageAssetBinding {
                    asset_id: format!("asset{slot}"),
                    fingerprint: "logical-identity".into(),
                    media_type: Some("image/png".into()),
                    bytes: b"shared-asset-bytes".to_vec(),
                })
                .expect("bind asset");
            store
                .page_tx_put_event(&CrawlEvent::Committed { page_identity: format!("https://example.com{slot}") })
                .expect("stage event");
            store.commit_page_tx().expect("commit");

            let refs = store.page_object_refs(&claim.item_id).expect("page object refs");
            assert_eq!(refs.len(), 3, "one fetch, one IR and one asset object");
            assert_eq!(refs[0].kind, ObjectKind::ResponseBody);
            assert_eq!(refs[1].kind, ObjectKind::IrPackage);
            assert_eq!(refs[2].kind, ObjectKind::AssetBytes);
            assert_eq!(refs[2].media_type.as_deref(), Some("image/png"));
            assert_eq!(store.get_object(&refs[0].fingerprint).expect("fetch bytes"), Some(b"shared-fetch-bytes".to_vec()));
            assert_eq!(store.get_object(&refs[2].fingerprint).expect("asset bytes"), Some(b"shared-asset-bytes".to_vec()));
            assert_eq!(store.page_asset_refs(&claim.item_id).expect("asset refs"), vec![refs[2].fingerprint.clone()]);
            assert_eq!(store.page_phase(&claim.item_id).expect("page phase"), PagePhase::Committed);

            fetch_fingerprints.push(refs[0].fingerprint.clone());
            asset_fingerprints.push(refs[2].fingerprint.clone());
            store.release_claim(&claim, crawler_store::ReleaseOutcome::Done).expect("release claim");
        }

        assert_eq!(fetch_fingerprints[0], fetch_fingerprints[1], "identical fetch bytes must deduplicate across pages");
        assert_eq!(asset_fingerprints[0], asset_fingerprints[1], "identical asset bytes must deduplicate across pages");

        let asset = store.object_ref(&asset_fingerprints[0]).expect("asset ref").expect("asset metadata exists");
        assert_eq!(asset.size, b"shared-asset-bytes".len() as u64);
        let fetch = store.object_ref(&fetch_fingerprints[0]).expect("fetch ref").expect("fetch metadata exists");
        assert_eq!(fetch.kind, ObjectKind::ResponseBody);
    });
}
