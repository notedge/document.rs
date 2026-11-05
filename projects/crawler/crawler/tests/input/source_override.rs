//! Covers `load_input_with_source` page-identity override.

use crawler::{InputFormat, LoadedInput, load_input, load_input_with_source};

#[test]
fn source_url_override_replaces_artifact_identity_for_relative_resolution() {
    let dir = std::env::temp_dir().join(format!("crawler-source-override-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("page.html");
    std::fs::write(&path, "<html><body><img src=\"img/flow.png\"></body></html>").expect("write");

    let loaded = load_input_with_source(
        path.to_str().expect("utf8 path"),
        InputFormat::Html,
        Some("https://example.com/docs/page.html"),
    )
    .expect("load");
    let LoadedInput::Artifact(artifact) = loaded
    else {
        panic!("expected artifact")
    };
    assert_eq!(artifact.final_url.as_str(), "https://example.com/docs/page.html");
    assert_eq!(artifact.request_url.as_str(), "https://example.com/docs/page.html");
    assert!(artifact.cache_key.contains("page.html"), "cache key stays per local file: {}", artifact.cache_key);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_source_url_the_local_file_url_stays_the_identity() {
    let dir = std::env::temp_dir().join(format!("crawler-source-default-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("page.html");
    std::fs::write(&path, "<html><body>ok</body></html>").expect("write");

    let loaded = load_input(path.to_str().expect("utf8 path"), InputFormat::Html).expect("load");
    let LoadedInput::Artifact(artifact) = loaded
    else {
        panic!("expected artifact")
    };
    assert_eq!(artifact.final_url.scheme(), "file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalid_source_url_is_a_structured_error() {
    let dir = std::env::temp_dir().join(format!("crawler-source-invalid-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("page.html");
    std::fs::write(&path, "<html><body>ok</body></html>").expect("write");

    let error =
        load_input_with_source(path.to_str().expect("utf8 path"), InputFormat::Html, Some("not a url")).expect_err("reject");
    assert!(error.to_string().contains("not a url"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn source_url_override_never_touches_snapshots() {
    let dir = std::env::temp_dir().join(format!("crawler-source-snapshot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("page.snapshot.json");
    std::fs::write(
        &path,
        serde_json::to_string(&crawler_types::BrowserSnapshot {
            requested_url: url::Url::parse("https://example.com/page").expect("url"),
            final_url: url::Url::parse("https://example.com/page").expect("url"),
            document_html: "<html><body>ok</body></html>".into(),
            captured_at_epoch: 1,
            browser_engine: "fixture".into(),
            profile_id: "test".into(),
            challenge_state: crawler_types::PageChallengeState::Normal,
        })
        .expect("json"),
    )
    .expect("write");

    let loaded =
        load_input_with_source(path.to_str().expect("utf8 path"), InputFormat::Snapshot, Some("https://other.example/x"))
            .expect("load");
    let LoadedInput::Snapshot(snapshot) = loaded
    else {
        panic!("expected snapshot")
    };
    assert_eq!(snapshot.final_url.as_str(), "https://example.com/page", "snapshots keep their browser-session url");
    let _ = std::fs::remove_dir_all(&dir);
}
