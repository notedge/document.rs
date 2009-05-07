//! Extracted from `src/inspect.rs`.

use std::collections::BTreeMap;

use crawler_extract::{Extractor, HtmlExtractor, inspect_artifact};
use crawler_types::{BodyStorage, ExtractBudget, FetchArtifact, InspectStage, TransportProvenance};
use url::Url;

fn html_artifact(html: &str) -> FetchArtifact {
    FetchArtifact {
        request_url: Url::parse("https://example.com/").expect("url"),
        final_url: Url::parse("https://example.com/").expect("url"),
        status: 200,
        response_headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
        content_type: Some("text/html".into()),
        body: BodyStorage::inline(html.as_bytes().to_vec()),
        redirect_chain: Vec::new(),
        retrieved_at_epoch: 1,
        transport_provenance: TransportProvenance::Test,
        cache_key: "https://example.com/".into(),
    }
}

#[test]
fn inspect_links_stage_finds_hrefs() {
    let artifact = html_artifact(r#"<html><body><a href="/next">next</a></body></html>"#);
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let report = inspect_artifact(&artifact, &extractors, InspectStage::Links, ExtractBudget::default());
    assert_eq!(report.discovered_links.len(), 1);
    assert!(report.discovered_links[0].target.contains("/next"));
}
