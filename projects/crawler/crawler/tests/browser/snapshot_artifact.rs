//! Extracted from `src/browser/mod.rs`.

use crawler::*;
use url::Url;

#[test]
fn snapshot_produces_html_artifact() {
    let snapshot = BrowserSnapshot {
        requested_url: Url::parse("https://example.com/").expect("url"),
        final_url: Url::parse("https://example.com/").expect("url"),
        document_html: "<html><body>hi</body></html>".into(),
        captured_at_epoch: 1,
        browser_engine: "fixture".into(),
        profile_id: "test".into(),
        challenge_state: PageChallengeState::Normal,
        boundary: Default::default(),
    };
    let artifact = snapshot_to_fetch_artifact(&snapshot);
    assert_eq!(artifact.content_type.as_deref(), Some("text/html"));
    assert_eq!(artifact.status, 200);
    assert!(artifact.body_bytes().is_some());
}
