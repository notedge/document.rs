//! Extracted from `src/assets.rs`.

use crawler::{fingerprint, hydrate_assets};
use crawler_fetch::{FetchClient, FetchRequest, MemoryEntry, MemoryTransport, RobotsPolicy, Transport, build_artifact};
use crawler_types::{BodyStorage, FetchArtifact, Result, TransportProvenance};
use document_ir::{Asset, AssetId, AssetKind, Block, DocumentGraph, DocumentId, Inline, NodeId, SemanticStatus};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
};

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

/// Transport that returns queued responses for asset URLs and 404 for robots
/// so the retry sequence is not consumed by the robots prefetch.
struct SequencedTransport {
    responses: Mutex<VecDeque<MemoryEntry>>,
    attempts: Arc<Mutex<usize>>,
}

impl Transport for SequencedTransport {
    fn fetch(&self, request: &FetchRequest) -> Result<FetchArtifact> {
        if request.url.path().ends_with("robots.txt") {
            return Ok(build_artifact(
                request.url.clone(),
                request.url.clone(),
                404,
                BTreeMap::new(),
                BodyStorage::Inline(Vec::new()),
                Vec::new(),
                TransportProvenance::Test,
                None,
            ));
        }
        *self.attempts.lock().expect("attempts") += 1;
        let entry = {
            let mut responses = self.responses.lock().expect("responses");
            if responses.len() > 1 {
                responses.pop_front().expect("queued response")
            }
            else {
                responses.front().expect("final response").clone()
            }
        };
        Ok(build_artifact(
            request.url.clone(),
            request.url.clone(),
            entry.status,
            entry.headers,
            BodyStorage::Inline(entry.body),
            Vec::new(),
            TransportProvenance::Test,
            None,
        ))
    }
}

/// R-7.16 asset fixture: a retryable status is retried before the asset is
/// resolved, so one transient failure does not lose the asset.
#[test]
fn retryable_asset_status_is_retried_before_resolving() {
    let source = "https://example.com/retried.png";
    let attempts = Arc::new(Mutex::new(0));
    let transport = SequencedTransport {
        attempts: attempts.clone(),
        responses: Mutex::new(VecDeque::from([
            MemoryEntry { status: 503, headers: BTreeMap::from([("retry-after".into(), "0".into())]), body: b"busy".to_vec() },
            MemoryEntry {
                status: 200,
                headers: BTreeMap::from([("content-type".into(), "image/png".into())]),
                body: b"retried-body".to_vec(),
            },
        ])),
    };
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(crawler_types::PolitenessProfile::Developer));
    let mut document = document_with_asset(source);

    assert_eq!(hydrate_assets(&mut document, &mut client, 8, 1024).expect("hydrate"), 1);
    assert_eq!(*attempts.lock().expect("attempts"), 2, "the retryable status must be fetched again");
    assert_eq!(document.assets[0].status, SemanticStatus::Resolved);
    assert_eq!(document.assets[0].bytes.as_deref(), Some(b"retried-body".as_slice()));
    assert_eq!(document.assets[0].content_identity.as_deref(), Some(fingerprint(b"retried-body").as_str()));
}

/// R-7.16 asset fixture: identical bytes resolve to one fingerprint and one
/// project asset, with both sources aliasing the same file.
#[test]
fn identical_asset_bytes_share_one_fingerprint_and_one_project_file() {
    let mut document = DocumentGraph::new(DocumentId(1));
    for (id, source) in [(1, "https://example.com/a.png"), (2, "https://example.com/b.png")] {
        document.push_asset(Asset {
            id: AssetId(id),
            kind: AssetKind::Image,
            content_identity: None,
            source: Some(source.to_string()),
            media_type: None,
            status: SemanticStatus::Unresolved,
            bytes: None,
        });
        document.push_block_with_id(
            NodeId(id),
            Block::Paragraph {
                content: vec![Inline::Image {
                    source: source.to_string(),
                    alt: vec![Inline::Text { text: format!("Image {id}") }],
                    title: None,
                    asset: Some(AssetId(id)),
                }],
            },
        );
    }
    let mut transport = MemoryTransport::new();
    for source in ["https://example.com/a.png", "https://example.com/b.png"] {
        transport.insert(
            source,
            MemoryEntry {
                status: 200,
                headers: BTreeMap::from([("content-type".into(), "image/png".into())]),
                body: b"shared-bytes".to_vec(),
            },
        );
    }
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(crawler_types::PolitenessProfile::Developer));

    assert_eq!(hydrate_assets(&mut document, &mut client, 8, 1024).expect("hydrate"), 2);
    let first = document.assets[0].content_identity.clone().expect("identity");
    assert_eq!(document.assets[1].content_identity.as_deref(), Some(first.as_str()));
    assert_eq!(first, fingerprint(b"shared-bytes"));

    let project = document_formats::export::markdown_project::export_markdown_project(&document).expect("export");
    assert_eq!(project.assets.len(), 1, "identical fingerprints must materialize one asset file");
    let file = &project.assets[0].relative_path;
    assert_eq!(project.index_markdown.matches(file.as_str()).count(), 2, "both image sources must alias the file");
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
