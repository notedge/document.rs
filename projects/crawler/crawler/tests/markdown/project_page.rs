use std::fs;

use crawler::{HtmlExtractor, publish_extracted_page, run_extract};
use crawler_types::{BodyStorage, ExtractBudget, ExtractContext, ExtractStatus, FetchArtifact, TransportProvenance};
use document_formats::import::markdown::import_markdown_bytes;
use document_ir::{Block, Inline};
use url::Url;

fn fixture_artifact(html: &str, page: &str) -> FetchArtifact {
    FetchArtifact {
        request_url: Url::parse(page).expect("url"),
        final_url: Url::parse(page).expect("url"),
        status: 200,
        response_headers: Default::default(),
        content_type: Some("text/html".into()),
        body: BodyStorage::inline(html.as_bytes()),
        redirect_chain: Vec::new(),
        retrieved_at_epoch: 1,
        transport_provenance: TransportProvenance::Test,
        cache_key: page.to_string(),
    }
}

#[test]
fn html_page_publishes_markdown_project_with_reopen() {
    let html = "<html><head><title>Sample</title></head><body><p>Hello Crawler page</p></body></html>";
    let artifact = fixture_artifact(html, "https://example.com/article");
    let extractor = HtmlExtractor;
    let extractors: [&dyn crawler::Extractor; 1] = [&extractor];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());
    assert!(matches!(result.status, ExtractStatus::Partial | ExtractStatus::Complete));

    let output_dir = std::env::temp_dir().join(format!("crawler-page-project-{}", std::process::id()));
    let published = publish_extracted_page(&output_dir, "article.html", &result).expect("publish page project");

    assert!(published.index_path.exists());
    assert!(published.report_path.ends_with("panduck.report.json"));
    let index = fs::read_to_string(published.index_path).expect("read index.md");
    assert!(index.contains("Hello Crawler page"));

    let report = fs::read_to_string(published.report_path).expect("read report");
    assert!(report.contains("extract_page"));
    assert!(report.contains("https://example.com/article"));

    let reopened = import_markdown_bytes("index.md", &index).expect("reopen markdown");
    assert!(reopened.blocks.iter().any(|node| matches!(
        &node.block,
        Block::Paragraph { content }
            if content.iter().any(|inline| matches!(inline, Inline::Text { text } if text.contains("Hello Crawler page")))
    )));

    let _ = fs::remove_dir_all(output_dir);
}

/// Two consecutive publishes with identical input must produce identical
/// directory trees — same files, same content, same layout.
#[test]
fn repeated_publish_produces_deterministic_directory_tree() {
    let html = r#"<html><head><title>D</title></head><body><p>Hello again</p></body></html>"#;
    let artifact = fixture_artifact(html, "https://example.com/deterministic");
    let extractor = HtmlExtractor;
    let extractors: [&dyn crawler::Extractor; 1] = [&extractor];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());

    let base = std::env::temp_dir().join(format!("crawler-page-deterministic-{}", std::process::id()));
    let dir_a = base.join("a");
    let dir_b = base.join("b");

    publish_extracted_page(&dir_a, "d.html", &result).expect("first publish");
    publish_extracted_page(&dir_b, "d.html", &result).expect("second publish");

    let a_entries = sorted_entries(&dir_a);
    let b_entries = sorted_entries(&dir_b);
    assert_eq!(a_entries, b_entries, "two publishes should yield identical file trees");

    let _ = fs::remove_dir_all(&base);
}

/// A publish that fails mid-flow (missing document graph on an unsupported
/// extractor) must not mutate a previously successful output directory.
#[test]
fn failed_publish_does_not_pollute_existing_output() {
    let html = r#"<html><body><p>Good page</p></body></html>"#;
    let artifact = fixture_artifact(html, "https://example.com/clean");
    let extractor = HtmlExtractor;
    let extractors: [&dyn crawler::Extractor; 1] = [&extractor];
    let good_result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());

    // Bad result: ExtractStatus::Unsupported with None document — hits the
    // `extract.document.as_ref().ok_or_else(...)` guard in publish_extracted_page.
    let bad_result = crawler_types::ExtractResult {
        status: crawler_types::ExtractStatus::Unsupported,
        source_url: artifact.final_url.clone(),
        document: None,
        discovered_links: Vec::new(),
        losses: Vec::new(),
        extractor: "html".into(),
    };

    let output_dir = std::env::temp_dir().join(format!("crawler-page-failure-iso-{}", std::process::id()));
    publish_extracted_page(&output_dir, "good.html", &good_result).expect("initial publish");
    let before = sorted_entries(&output_dir);

    let err = publish_extracted_page(&output_dir, "bad.html", &bad_result);
    assert!(err.is_err(), "missing document graph must produce an error");

    let after = sorted_entries(&output_dir);
    assert_eq!(before, after, "failed publish must not mutate existing output");

    let _ = fs::remove_dir_all(&output_dir);
}

/// Republishing into the same directory must replace the whole layout instead
/// of merging: files left by the previous publish do not survive, and no
/// staging/backup/lock sibling is left behind.
#[test]
fn repeated_publish_to_same_directory_replaces_stale_layout() {
    let html = r#"<html><head><title>Re</title></head><body><p>Repeated</p></body></html>"#;
    let artifact = fixture_artifact(html, "https://example.com/repeated");
    let extractor = HtmlExtractor;
    let extractors: [&dyn crawler::Extractor; 1] = [&extractor];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());

    let output_dir = unique_temp_dir("crawler-repeated-publish");
    publish_extracted_page(&output_dir, "first.html", &result).expect("first publish");
    fs::write(output_dir.join("stale.md"), "stale").expect("write stale file");

    let published = publish_extracted_page(&output_dir, "second.html", &result).expect("second publish");
    assert!(!output_dir.join("stale.md").exists(), "republish must replace the layout, not merge it");
    let index = fs::read_to_string(published.index_path).expect("read index");
    assert!(index.contains("Repeated"));
    let report = fs::read_to_string(published.report_path).expect("read report");
    assert!(report.contains("second.html"));
    assert!(publish_siblings(&output_dir).is_empty(), "publish must not leave staging/backup/lock siblings");

    let _ = fs::remove_dir_all(&output_dir);
}

/// Publishes that target the same directory from two threads must serialize,
/// both succeed, and leave one complete layout with no residue.
#[test]
fn concurrent_publishes_to_same_directory_serialize() {
    let html = r#"<html><body><p>Concurrent</p></body></html>"#;
    let artifact = fixture_artifact(html, "https://example.com/concurrent");
    let extractor = HtmlExtractor;
    let extractors: [&dyn crawler::Extractor; 1] = [&extractor];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());

    let output_dir = unique_temp_dir("crawler-concurrent-publish");
    std::thread::scope(|scope| {
        for _ in 0..2 {
            let result = result.clone();
            let output_dir = output_dir.clone();
            scope.spawn(move || publish_extracted_page(&output_dir, "page.html", &result).expect("concurrent publish"));
        }
    });

    let index = fs::read_to_string(output_dir.join("index.md")).expect("read index");
    assert!(index.contains("Concurrent"));
    assert!(output_dir.join("panduck.report.json").exists());
    assert!(publish_siblings(&output_dir).is_empty(), "concurrent publishes must not leave residue");

    let _ = fs::remove_dir_all(&output_dir);
}

/// A lock held by a live publisher rejects the second publish with the stable
/// conflict code and leaves the existing output untouched.
#[test]
fn live_publish_lock_reports_conflict() {
    let html = r#"<html><body><p>Locked</p></body></html>"#;
    let artifact = fixture_artifact(html, "https://example.com/locked");
    let extractor = HtmlExtractor;
    let extractors: [&dyn crawler::Extractor; 1] = [&extractor];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());

    let output_dir = unique_temp_dir("crawler-live-lock");
    fs::create_dir_all(&output_dir).expect("create output dir");
    let lock_path =
        output_dir.parent().expect("parent").join(format!(".{}.lock", output_dir.file_name().expect("name").to_string_lossy()));
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_secs()).unwrap_or(0);
    fs::write(&lock_path, format!(r#"{{"pid":1,"started_at_epoch":{now}}}"#)).expect("write lock");
    fs::write(output_dir.join("index.md"), "old").expect("write old output");

    let error = publish_extracted_page(&output_dir, "locked.html", &result).expect_err("live lock must conflict");
    assert_eq!(error.code(), Some("crawler.publish.conflict"));
    assert_eq!(fs::read_to_string(output_dir.join("index.md")).expect("read old index"), "old");
    assert!(lock_path.exists(), "the holder's lock must not be stolen");

    fs::remove_file(&lock_path).expect("remove lock");
    let _ = fs::remove_dir_all(&output_dir);
}

/// A stale lock left by a dead publisher is reclaimed once, then the publish
/// proceeds and removes it.
#[test]
fn stale_publish_lock_is_reclaimed() {
    let html = r#"<html><body><p>Reclaimed</p></body></html>"#;
    let artifact = fixture_artifact(html, "https://example.com/reclaimed");
    let extractor = HtmlExtractor;
    let extractors: [&dyn crawler::Extractor; 1] = [&extractor];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());

    let output_dir = unique_temp_dir("crawler-stale-lock");
    fs::create_dir_all(&output_dir).expect("create output dir");
    let lock_path =
        output_dir.parent().expect("parent").join(format!(".{}.lock", output_dir.file_name().expect("name").to_string_lossy()));
    fs::write(&lock_path, r#"{"pid":999999,"started_at_epoch":0}"#).expect("write stale lock");

    publish_extracted_page(&output_dir, "reclaimed.html", &result).expect("stale lock must be reclaimed");
    assert!(!lock_path.exists(), "publish must release the reclaimed lock");
    assert!(fs::read_to_string(output_dir.join("index.md")).expect("read index").contains("Reclaimed"));

    let _ = fs::remove_dir_all(&output_dir);
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_nanos()).unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

/// Sibling staging/backup/lock artifacts that a publish should always clean up.
fn publish_siblings(output_dir: &std::path::Path) -> Vec<String> {
    let parent = output_dir.parent().expect("parent");
    let name = output_dir.file_name().expect("name").to_string_lossy();
    let prefix = format!(".{name}.");
    fs::read_dir(parent)
        .expect("read parent")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|entry| entry.starts_with(&prefix))
        .collect()
}

fn sorted_entries(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    walk(dir, dir, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        let rel = path.strip_prefix(root).expect("strip");
        if path.is_dir() {
            walk(root, &path, out);
        }
        else {
            let bytes = fs::read(&path).expect("read file");
            out.push((rel.to_string_lossy().to_string(), bytes));
        }
    }
}
