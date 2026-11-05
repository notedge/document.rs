//! Extracted from `src/assets.rs`.

use crawler::{fingerprint, hydrate_assets};
use crawler_fetch::{FetchClient, MemoryEntry, MemoryTransport, RobotsPolicy};
use document_ir::{Asset, AssetId, AssetKind, DocumentGraph, DocumentId, SemanticStatus};
use std::collections::BTreeMap;

fn document_with_asset(source: &str) -> DocumentGraph {
    let mut document = DocumentGraph::new(DocumentId(1));
    document.push_asset(Asset {
        id: AssetId(1),
        kind: AssetKind::Image,
        content_identity: None,
        source: Some(source.to_string()),
        media_type: Some("image/jpeg".to_string()),
        status: SemanticStatus::Unresolved,
        bytes: None,
    });
    document
}

#[test]
fn materializes_bytes_identity_and_response_media_type() {
    let source = "https://example.com/image.bin";
    let mut transport = MemoryTransport::new();
    transport.insert(
        source,
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "image/png".into())]),
            body: b"asset-body".to_vec(),
        },
    );
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(crawler_types::PolitenessProfile::Developer));
    let mut document = document_with_asset(source);

    let count = hydrate_assets(&mut document, &mut client, 8, 1024).expect("hydrate");

    assert_eq!(count, 1);
    let asset = &document.assets[0];
    let expected_identity = fingerprint(b"asset-body");
    assert_eq!(asset.bytes.as_deref(), Some(b"asset-body".as_slice()));
    assert_eq!(asset.media_type.as_deref(), Some("image/png"));
    assert_eq!(asset.status, SemanticStatus::Resolved);
    assert_eq!(asset.content_identity.as_deref(), Some(expected_identity.as_str()));
    assert!(document.coverage.complete);
}

#[test]
fn fetch_failure_preserves_unresolved_asset_and_records_loss() {
    let source = "https://example.com/missing.png";
    let mut client =
        FetchClient::new(MemoryTransport::new(), RobotsPolicy::from_profile(crawler_types::PolitenessProfile::Developer));
    let mut document = document_with_asset(source);

    assert_eq!(hydrate_assets(&mut document, &mut client, 8, 1024).expect("hydrate"), 0);
    assert_eq!(document.assets[0].status, SemanticStatus::Unresolved);
    assert_eq!(document.coverage.loss[0].code, "crawler.asset.fetch_failed");
    assert!(!document.coverage.complete);
}

#[test]
fn byte_budget_records_loss_without_materializing_asset() {
    let source = "https://example.com/large.png";
    let mut transport = MemoryTransport::new();
    transport.insert(source, MemoryEntry { status: 200, headers: BTreeMap::new(), body: b"12345".to_vec() });
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(crawler_types::PolitenessProfile::Developer));
    let mut document = document_with_asset(source);

    assert_eq!(hydrate_assets(&mut document, &mut client, 8, 4).expect("hydrate"), 0);
    assert_eq!(document.assets[0].status, SemanticStatus::Unresolved);
    assert_eq!(document.coverage.loss[0].code, "crawler.asset.budget_exhausted");
}

#[test]
fn non_success_response_records_loss_without_resolving_asset() {
    let source = "https://example.com/missing.png";
    let mut transport = MemoryTransport::new();
    transport.insert(source, MemoryEntry { status: 404, headers: BTreeMap::new(), body: b"not found".to_vec() });
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(crawler_types::PolitenessProfile::Developer));
    let mut document = document_with_asset(source);

    assert_eq!(hydrate_assets(&mut document, &mut client, 8, 1024).expect("hydrate"), 0);
    assert_eq!(document.assets[0].status, SemanticStatus::Unresolved);
    assert_eq!(document.coverage.loss[0].code, "crawler.asset.http_status");
}

#[test]
fn zero_asset_budget_records_loss_without_fetching() {
    let mut document = document_with_asset("https://example.com/image.png");
    let mut client =
        FetchClient::new(MemoryTransport::new(), RobotsPolicy::from_profile(crawler_types::PolitenessProfile::Developer));
    assert_eq!(hydrate_assets(&mut document, &mut client, 0, 1024).expect("hydrate"), 0);
    assert_eq!(document.assets[0].status, SemanticStatus::Unresolved);
    assert_eq!(document.coverage.loss.len(), 1);
    assert_eq!(document.coverage.loss[0].code, "crawler.asset.budget_exhausted");
    assert!(!document.coverage.complete);
}
