use crawler::save_snapshot_json;
use crawler_types::{BrowserSnapshot, PageChallengeState};
use url::Url;

#[test]
fn save_snapshot_json_publishes_atomically() {
    let dir = std::env::temp_dir().join(format!("crawler-snapshot-publish-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("page.snapshot.json");

    let url = Url::parse("https://example.test/page").expect("url");
    let snapshot = BrowserSnapshot {
        requested_url: url.clone(),
        final_url: url,
        document_html: "<html><body>hello</body></html>".into(),
        captured_at_epoch: 1,
        browser_engine: "fixture".into(),
        profile_id: "test".into(),
        challenge_state: PageChallengeState::Normal,
        boundary: Default::default(),
    };

    save_snapshot_json(&path, &snapshot).expect("save snapshot");
    let text = std::fs::read_to_string(&path).expect("read snapshot");
    assert!(text.contains("https://example.test/page"));

    let _ = std::fs::remove_dir_all(dir);
}
