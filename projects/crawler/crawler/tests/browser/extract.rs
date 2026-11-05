//! Extracted from `src/browser/extract.rs`.

use crawler::{Extractor, HtmlExtractor, run_extract_from_snapshot};
use crawler_types::{
    BrowserSnapshot, ChallengeOutcome, ChallengePolicy, ExtractBudget, ExtractContext, ExtractStatus, PageChallengeState,
};
use url::Url;

fn snapshot(state: PageChallengeState) -> BrowserSnapshot {
    BrowserSnapshot {
        requested_url: Url::parse("https://example.com/").expect("url"),
        final_url: Url::parse("https://example.com/").expect("url"),
        document_html: "<html><head><title>T</title></head><body>ok</body></html>".into(),
        captured_at_epoch: 1,
        browser_engine: "fixture".into(),
        profile_id: "test".into(),
        challenge_state: state,
    }
}

#[test]
fn challenge_blocks_extraction_when_policy_stops() {
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let blocked = run_extract_from_snapshot(
        &snapshot(PageChallengeState::LoginRequired),
        ChallengePolicy::Stop,
        &extractors,
        ExtractBudget::default(),
        &ExtractContext::default(),
    )
    .unwrap_err();
    assert_eq!(blocked.outcome, ChallengeOutcome::Stop);
}

#[test]
fn normal_snapshot_extracts() {
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let result = run_extract_from_snapshot(
        &snapshot(PageChallengeState::Normal),
        ChallengePolicy::Stop,
        &extractors,
        ExtractBudget::default(),
        &ExtractContext::default(),
    )
    .expect("extract");
    assert_eq!(result.status, ExtractStatus::Partial);
}
