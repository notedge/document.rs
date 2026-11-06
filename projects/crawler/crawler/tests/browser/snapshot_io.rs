//! Extracted from `src/browser/snapshot_io.rs`.

use crawler::BrowserProvider;
use url::Url;

use crawler_types::PageChallengeState;

use crawler::*;

#[test]
fn load_fixture_provider_reads_snapshot_directory() {
    let dir = std::env::temp_dir().join(format!("crawler-fixtures-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("page.snapshot.json");
    std::fs::write(
        &path,
        serde_json::to_string(&BrowserSnapshot {
            requested_url: Url::parse("https://example.com/page").expect("url"),
            final_url: Url::parse("https://example.com/page").expect("url"),
            document_html: "<html><body>ok</body></html>".into(),
            captured_at_epoch: 1,
            browser_engine: "fixture".into(),
            profile_id: "test".into(),
            challenge_state: PageChallengeState::Normal,
            boundary: Default::default(),
        })
        .expect("json"),
    )
    .expect("write");

    let provider = load_fixture_provider_from_dir(&dir).expect("load fixtures");
    let snapshot = provider.capture_snapshot(&Url::parse("https://example.com/page").expect("url")).expect("snapshot");
    assert_eq!(snapshot.profile_id, "test");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_fixture_provider_from_dirs_later_dir_overrides_url() {
    let base = std::env::temp_dir().join(format!("crawler-fixtures-multi-{}", std::process::id()));
    let first = base.join("first");
    let second = base.join("second");
    std::fs::create_dir_all(&first).expect("first dir");
    std::fs::create_dir_all(&second).expect("second dir");
    let url = Url::parse("https://example.com/page").expect("url");
    let first_snapshot = BrowserSnapshot {
        requested_url: url.clone(),
        final_url: url.clone(),
        document_html: "<html><body>first</body></html>".into(),
        captured_at_epoch: 1,
        browser_engine: "fixture".into(),
        profile_id: "first".into(),
        challenge_state: PageChallengeState::Normal,
        boundary: Default::default(),
    };
    let second_snapshot = BrowserSnapshot {
        requested_url: url.clone(),
        final_url: url.clone(),
        document_html: "<html><body>second</body></html>".into(),
        captured_at_epoch: 2,
        browser_engine: "fixture".into(),
        profile_id: "second".into(),
        challenge_state: PageChallengeState::Normal,
        boundary: Default::default(),
    };
    save_snapshot_json(first.join("page.snapshot.json"), &first_snapshot).expect("write first");
    save_snapshot_json(second.join("page.snapshot.json"), &second_snapshot).expect("write second");

    let provider = load_fixture_provider_from_dirs(&[first, second]).expect("load fixtures");
    let snapshot = provider.capture_snapshot(&url).expect("snapshot");
    assert_eq!(snapshot.profile_id, "second");
    let _ = std::fs::remove_dir_all(&base);
}
