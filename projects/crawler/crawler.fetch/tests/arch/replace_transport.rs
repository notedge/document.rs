//! 架构约束：FetchClient 可替换 Transport。

use std::collections::BTreeMap;

use crawler_fetch::{FetchClient, FetchRequest, RobotsPolicy, Transport, build_artifact};
use crawler_types::{BodyStorage, PolitenessProfile, TransportProvenance};
use url::Url;

#[derive(Debug, Clone, Default)]
struct FixtureTransport;

impl Transport for FixtureTransport {
    fn fetch(&self, request: &FetchRequest) -> crawler_types::Result<crawler_types::FetchArtifact> {
        Ok(build_artifact(
            request.url.clone(),
            request.url.clone(),
            200,
            BTreeMap::new(),
            BodyStorage::inline(b"fixture-body"),
            Vec::new(),
            TransportProvenance::Test,
            None,
        ))
    }
}

#[test]
fn fetch_client_accepts_replaceable_transport() {
    let mut client = FetchClient::new(FixtureTransport, RobotsPolicy::from_profile(PolitenessProfile::Developer));
    let url = Url::parse("https://example.com/replace").expect("url");
    let artifact = client.fetch(&FetchRequest { url, headers: BTreeMap::new() }).expect("fetch");
    assert_eq!(artifact.body_bytes(), Some(b"fixture-body".as_slice()));
    assert_eq!(artifact.transport_provenance, TransportProvenance::Test);
}
