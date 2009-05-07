//! Extracted from `src/browser/stack.rs`.

use crawler::{BrowserProvider, BrowserProviderStack, FixtureBrowserProvider, HttpBrowserProvider};
use crawler_types::{BrowserSnapshot, PageChallengeState};
use url::Url;

#[test]
fn fixture_provider_wins_before_http_worker() {
    let mut fixture = FixtureBrowserProvider::new();
    fixture.insert(BrowserSnapshot {
        requested_url: Url::parse("https://example.com/page").expect("url"),
        final_url: Url::parse("https://example.com/page").expect("url"),
        document_html: "<html><body>fixture</body></html>".into(),
        captured_at_epoch: 1,
        browser_engine: "fixture".into(),
        profile_id: "test".into(),
        challenge_state: PageChallengeState::Normal,
    });
    let http = HttpBrowserProvider::new(Url::parse("http://127.0.0.1:1/unreachable").expect("url"));
    let stack = BrowserProviderStack::new().with_fixture(fixture).with_http(http);
    let snapshot = stack.capture_snapshot(&Url::parse("https://example.com/page").expect("url")).expect("snapshot");
    assert_eq!(snapshot.browser_engine, "fixture");
}
