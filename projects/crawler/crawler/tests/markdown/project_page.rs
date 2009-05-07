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
