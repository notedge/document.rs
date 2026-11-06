//! 架构约束：浏览器能力可替换 provider。

use crawler_browser::{BrowserProvider, run_extract_from_snapshot};
use crawler_extract::HtmlExtractor;
use crawler_types::{BrowserSnapshot, ChallengePolicy, ExtractBudget, ExtractContext, PageChallengeState};
use url::Url;

#[derive(Debug, Clone)]
struct StaticProvider {
    html: String,
}

impl BrowserProvider for StaticProvider {
    fn capture_snapshot(&self, url: &Url) -> crawler_types::Result<BrowserSnapshot> {
        Ok(BrowserSnapshot {
            requested_url: url.clone(),
            final_url: url.clone(),
            document_html: self.html.clone(),
            captured_at_epoch: 1,
            browser_engine: "static".into(),
            profile_id: "replace".into(),
            challenge_state: PageChallengeState::Normal,
            boundary: Default::default(),
        })
    }
}

#[test]
fn browser_provider_is_replaceable() {
    let provider = StaticProvider { html: "<html><body><p>replaceable</p></body></html>".into() };
    let snapshot = provider.capture_snapshot(&Url::parse("https://example.com/page").expect("url")).expect("snapshot");
    assert_eq!(snapshot.browser_engine, "static");
}

#[test]
fn snapshot_extract_runs_through_replaceable_provider() {
    let provider = StaticProvider { html: "<html><body><p>replaceable</p></body></html>".into() };
    let url = Url::parse("https://example.com/page").expect("url");
    let snapshot = provider.capture_snapshot(&url).expect("snapshot");
    let html = HtmlExtractor;
    let extractors: [&dyn crawler_extract::Extractor; 1] = [&html];
    let result = run_extract_from_snapshot(
        &snapshot,
        ChallengePolicy::Stop,
        &extractors,
        ExtractBudget::default(),
        &ExtractContext::default(),
    )
    .expect("extract");
    assert!(result.document.is_some());
}
