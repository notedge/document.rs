//! Browser boundary fixtures: session expiry, frames, shadow roots and downloads.

use crawler_browser::{
    BrowserProvider, ChallengeBlocked, FixtureBrowserProvider, run_extract_from_snapshot, snapshot_to_fetch_artifact,
};
use crawler_extract::{Extractor, HtmlExtractor};
use crawler_types::{
    BrowserBoundary, BrowserDownload, BrowserFrame, BrowserShadowRoot, BrowserSnapshot, ChallengeOutcome, ChallengePolicy,
    ExtractBudget, ExtractContext, ExtractResult, PageChallengeState,
};
use url::Url;

fn page(html: &str) -> BrowserSnapshot {
    let url = Url::parse("https://example.test/page.html").expect("url");
    BrowserSnapshot {
        requested_url: url.clone(),
        final_url: url,
        document_html: html.into(),
        captured_at_epoch: 1_700_000_000,
        browser_engine: "fixture-chromium".into(),
        profile_id: "fixture".into(),
        challenge_state: PageChallengeState::Normal,
        boundary: BrowserBoundary::default(),
    }
}

fn extract(snapshot: &BrowserSnapshot, policy: ChallengePolicy) -> Result<ExtractResult, ChallengeBlocked> {
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    run_extract_from_snapshot(snapshot, policy, &extractors, ExtractBudget::default(), &ExtractContext::default())
}

#[test]
fn expired_session_is_a_distinct_blocked_state() {
    let mut login = page("<html><body><p>login</p></body></html>");
    login.challenge_state = PageChallengeState::LoginRequired;
    let mut challenge = page("<html><body><p>challenge</p></body></html>");
    challenge.challenge_state = PageChallengeState::ChallengeRequired;
    let mut expired = page("<html><body><p>login</p></body></html>");
    expired.challenge_state = PageChallengeState::SessionExpired;

    assert_eq!(serde_json::to_string(&PageChallengeState::SessionExpired).expect("json"), "\"session_expired\"");
    assert_ne!(
        serde_json::to_string(&PageChallengeState::SessionExpired).expect("json"),
        serde_json::to_string(&PageChallengeState::LoginRequired).expect("json")
    );

    for policy in [
        ChallengePolicy::Stop,
        ChallengePolicy::PauseForOperator,
        ChallengePolicy::SkipPage,
        ChallengePolicy::FallbackHttp,
        ChallengePolicy::UseExistingSession,
    ] {
        let blocked = extract(&expired, policy).expect_err("expired session must not extract");
        assert_eq!(blocked.state, PageChallengeState::SessionExpired, "policy {policy:?}");
        assert_ne!(blocked.outcome, ChallengeOutcome::Proceed, "expired session must not reuse credentials");
        let other = extract(&login, policy).expect_err("login page must not extract");
        assert_eq!(other.state, PageChallengeState::LoginRequired, "policy {policy:?}");
        let challenge_page = extract(&challenge, policy).expect_err("challenge page must not extract");
        assert_eq!(challenge_page.state, PageChallengeState::ChallengeRequired, "policy {policy:?}");
    }

    let blocked = extract(&expired, ChallengePolicy::PauseForOperator).expect_err("expired session must pause");
    assert_eq!(blocked.outcome, ChallengeOutcome::PauseForOperator);
}

#[test]
fn iframe_boundary_keeps_cross_origin_dom_out() {
    let mut snapshot = page("<html><body><p>top-level</p></body></html>");
    let same_origin = Url::parse("https://example.test/frame.html").expect("url");
    let cross_origin = Url::parse("https://ads.example.test/frame.html").expect("url");
    snapshot.boundary.frames.push(BrowserFrame {
        url: same_origin,
        same_origin: true,
        document_html: Some("<html><body><p>same-origin-frame</p></body></html>".into()),
    });
    snapshot.boundary.frames.push(BrowserFrame { url: cross_origin, same_origin: false, document_html: None });

    let result = extract(&snapshot, ChallengePolicy::Stop).expect("extract");
    assert_eq!(result.status, crawler_types::ExtractStatus::Partial);
    assert!(
        result.losses.iter().any(|loss| loss.code == "crawler.browser.frame_boundary"),
        "same-origin frame must be reported as a boundary loss"
    );
    let document = serde_json::to_string(&result.document).expect("document json");
    assert!(!document.contains("same-origin-frame"), "frame DOM must not be silently merged");
    assert!(
        !result.discovered_links.iter().any(|link| link.target.as_str().contains("ads.example.test")),
        "cross-origin iframe metadata must not become a navigation candidate"
    );

    let mut invalid = snapshot.clone();
    invalid.boundary.frames[1].document_html = Some("<p>forbidden</p>".into());
    assert!(crawler_browser::validate_browser_boundary(&invalid).is_err(), "cross-origin DOM must fail validation");
    let blocked = extract(&invalid, ChallengePolicy::Stop).expect_err("invalid boundary must not extract");
    assert_eq!(blocked.state, PageChallengeState::UnsupportedBrowserFlow);
    assert_eq!(blocked.outcome, ChallengeOutcome::Stop);

    let mut provider = FixtureBrowserProvider::new();
    provider.insert(invalid);
    assert!(
        provider.capture_snapshot(&Url::parse("https://example.test/page.html").expect("url")).is_err(),
        "provider must reject cross-origin DOM"
    );
}

#[test]
fn shadow_root_boundary_keeps_closed_root_out() {
    let mut snapshot = page("<html><body><p>top-level</p></body></html>");
    snapshot.boundary.shadow_roots.push(BrowserShadowRoot {
        host: "article#main".into(),
        open: true,
        document_html: Some("<p>open-shadow</p>".into()),
    });
    snapshot.boundary.shadow_roots.push(BrowserShadowRoot { host: "payment-widget".into(), open: false, document_html: None });

    let result = extract(&snapshot, ChallengePolicy::Stop).expect("extract");
    assert_eq!(result.losses.iter().filter(|loss| loss.code == "crawler.browser.shadow_root_boundary").count(), 2);
    let document = serde_json::to_string(&result.document).expect("document json");
    assert!(!document.contains("open-shadow"), "shadow DOM must not be silently merged");
    assert!(!document.contains("payment-widget"), "closed root metadata must not enter the document");

    let mut invalid = snapshot.clone();
    invalid.boundary.shadow_roots[1].document_html = Some("<p>forbidden</p>".into());
    assert!(crawler_browser::validate_browser_boundary(&invalid).is_err(), "closed root DOM must fail validation");
    let blocked = extract(&invalid, ChallengePolicy::Stop).expect_err("invalid boundary must not extract");
    assert_eq!(blocked.state, PageChallengeState::UnsupportedBrowserFlow);
}

#[test]
fn downloads_stay_provenance_only() {
    let mut snapshot = page("<html><body><p>top-level</p></body></html>");
    snapshot.boundary.downloads.push(BrowserDownload {
        url: Url::parse("https://example.test/export.zip").expect("url"),
        filename: Some("export.zip".into()),
        declared_bytes: Some(4096),
    });

    let result = extract(&snapshot, ChallengePolicy::Stop).expect("extract");
    assert!(result.losses.iter().any(|loss| loss.code == "crawler.browser.download_not_materialized"));
    assert!(
        !result.discovered_links.iter().any(|link| link.target.as_str().contains("export.zip")),
        "download must not become a navigation candidate"
    );
    let document = serde_json::to_string(&result.document).expect("document json");
    assert!(!document.contains("export.zip"));

    let artifact = snapshot_to_fetch_artifact(&snapshot);
    let body = artifact.body_bytes().expect("inline body");
    assert!(!String::from_utf8_lossy(body).contains("export.zip"));

    let mut provider = FixtureBrowserProvider::new();
    provider.insert(snapshot.clone());
    let captured = provider.capture_snapshot(&Url::parse("https://example.test/page.html").expect("url")).expect("snapshot");
    assert_eq!(captured.boundary.downloads.len(), 1, "download provenance must survive provider capture");
}

#[test]
fn boundary_metadata_round_trips_through_snapshot_json() {
    let mut snapshot = page("<html><body><p>top-level</p></body></html>");
    snapshot.boundary.frames.push(BrowserFrame {
        url: Url::parse("https://example.test/frame.html").expect("url"),
        same_origin: true,
        document_html: Some("<p>frame</p>".into()),
    });
    snapshot.boundary.shadow_roots.push(BrowserShadowRoot { host: "article#main".into(), open: true, document_html: None });
    snapshot.boundary.downloads.push(BrowserDownload {
        url: Url::parse("https://example.test/export.zip").expect("url"),
        filename: None,
        declared_bytes: None,
    });

    let json = serde_json::to_string(&snapshot).expect("snapshot json");
    let restored: BrowserSnapshot = serde_json::from_str(&json).expect("snapshot restore");
    assert_eq!(restored.boundary, snapshot.boundary);
    let legacy: BrowserSnapshot =
        serde_json::from_str(&json.replace(",\"boundary\":", ",\"ignored\":")).expect("legacy snapshot");
    assert_eq!(legacy.boundary, BrowserBoundary::default());
}
