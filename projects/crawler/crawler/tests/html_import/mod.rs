//! Acceptance gate 1 fixtures for HTML import and link discovery.

use crawler::{Extractor, HtmlExtractor, run_extract};
use crawler_types::{BodyStorage, ExtractBudget, ExtractContext, ExtractStatus, FetchArtifact, TransportProvenance};
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
fn gate_title_case_and_entities() {
    let html = "<html><HEAD><TITLE>Case &amp; Title</TITLE></HEAD><body></body></html>";
    let artifact = fixture_artifact(html, "https://example.com/");
    let extractor = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&extractor];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());
    assert_eq!(result.status, ExtractStatus::Partial);
    let document = result.document.expect("document");
    assert_eq!(document.metadata.title.as_deref(), Some("Case & Title"));
}

#[test]
fn gate_single_quote_href_and_script_exclusion() {
    let html = r#"<html><body><script>alert(1)</script><a href='/next'>x</a></body></html>"#;
    let artifact = fixture_artifact(html, "https://example.com/docs/");
    let extractor = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&extractor];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());
    let document = result.document.expect("document");
    let body = document.blocks.first().and_then(|node| match &node.block {
        document_ir::Block::Paragraph { content } => content.first(),
        _ => None,
    });
    assert!(matches!(
        body,
        Some(document_ir::Inline::Link { display, .. })
            if matches!(display.as_slice(), [document_ir::Inline::Text { text }] if text == "x")
    ));
    assert_eq!(result.discovered_links.len(), 1);
    assert_eq!(result.discovered_links[0].target, "https://example.com/next");
}

#[test]
fn gate_base_href_rebases_links() {
    let html = r#"<html><head><base href="https://cdn.example.com/sub/"></head>
<body><a href="item.html">x</a></body></html>"#;
    let artifact = fixture_artifact(html, "https://example.com/docs/page.html");
    let extractor = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&extractor];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());
    assert_eq!(result.discovered_links[0].target, "https://cdn.example.com/sub/item.html");
}

#[test]
fn gate_form_action_and_srcset() {
    let html = r#"<html><body>
<form action="/search"></form>
<img src="/a.png" srcset="/b.png 1x, https://cdn.example/x.webp 2x">
</body></html>"#;
    let artifact = fixture_artifact(html, "https://example.com/");
    let extractor = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&extractor];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());
    let targets = result.discovered_links.iter().map(|link| link.target.as_str()).collect::<Vec<_>>();
    assert!(targets.contains(&"https://example.com/search"));
    assert!(targets.contains(&"https://example.com/a.png"));
    assert!(targets.contains(&"https://example.com/b.png"));
    assert!(targets.contains(&"https://cdn.example/x.webp"));
    assert!(
        result
            .discovered_links
            .iter()
            .filter(|link| link.target.ends_with(".png") || link.target.ends_with(".webp"))
            .all(|link| link.relation == crawler_types::LinkRelation::Asset)
    );
    let document = result.document.expect("document");
    assert_eq!(document.assets.len(), 1, "one authored image occurrence maps to one asset");
    assert_eq!(document.assets[0].source.as_deref(), Some("https://example.com/a.png"));
    assert_eq!(document.relations.len(), 1);
    assert!(document.blocks.iter().any(|node| match &node.block {
        document_ir::Block::Paragraph { content } =>
            content.iter().any(|inline| matches!(inline, document_ir::Inline::Image { asset: Some(_), .. })),
        _ => false,
    }));
}

#[test]
fn gate_comment_does_not_trap_link_discovery() {
    let html = "<html><body><!-- <a href=\"/trap\"> --><a href=\"/ok\">ok</a></body></html>";
    let artifact = fixture_artifact(html, "https://example.com/");
    let extractor = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&extractor];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());
    assert_eq!(result.discovered_links.len(), 1);
    assert_eq!(result.discovered_links[0].target, "https://example.com/ok");
}
