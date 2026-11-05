//! Extract from `src/html.rs`.

use std::collections::BTreeMap;

use crawler_extract::{Extractor, HtmlExtractor};
use crawler_types::{
    BodyStorage, ExtractBudget, ExtractContext, ExtractInput, ExtractStatus, FetchArtifact, LinkRelation, TransportProvenance,
};
use document_ir::{Block, DocumentGraph, SourceKind};
use url::Url;

fn artifact(page: &str, html: &str) -> FetchArtifact {
    FetchArtifact {
        request_url: Url::parse(page).expect("url"),
        final_url: Url::parse(page).expect("url"),
        status: 200,
        response_headers: BTreeMap::new(),
        content_type: Some("text/html".into()),
        body: BodyStorage::inline(html.as_bytes().to_vec()),
        redirect_chain: Vec::new(),
        retrieved_at_epoch: 1,
        transport_provenance: TransportProvenance::Test,
        cache_key: page.into(),
    }
}

fn loss_span_text<'a>(document: &DocumentGraph, code: &str, body: &'a str) -> &'a str {
    let index = document
        .coverage
        .loss
        .iter()
        .position(|loss| loss.code == code)
        .unwrap_or_else(|| panic!("missing loss `{code}`: {:?}", document.coverage.loss));
    let span = document.loss_span(index).expect("every diagram loss must carry an authored span");
    &body[span.start as usize..span.end as usize]
}

#[test]
fn html_import_drives_title_blocks_assets_and_provenance() {
    let artifact = artifact(
        "https://example.com/docs/page.html",
        r#"<html><head><title>Guide</title></head><body><h1>Title</h1><p>Body <img src="../img/diagram.png" alt="D"></p></body></html>"#,
    );
    let result = HtmlExtractor
        .extract(ExtractInput { artifact: &artifact, budget: ExtractBudget::default() }, &ExtractContext::default());

    let document = result.document.expect("document");
    assert_eq!(document.metadata.title.as_deref(), Some("Guide"));
    assert!(matches!(document.blocks[0].block, Block::Section { .. }));
    assert_eq!(document.assets.len(), 1);
    assert_eq!(document.assets[0].source.as_deref(), Some("https://example.com/img/diagram.png"));
    assert_eq!(document.relations.len(), 1);
    assert!(document.sources.iter().all(|source| matches!(source.kind, SourceKind::TextSpan { .. })), "{:?}", document.sources);
    assert_eq!(result.status, ExtractStatus::Partial);
}

#[test]
fn mermaid_source_blocks_stay_diagrams_with_provenance() {
    let artifact = artifact(
        "https://example.com/docs/flow.html",
        "<html><head><title>Flow</title></head><body>\n\
         <div class=\"mermaid\">flowchart TD\n  A[Fetch] --&gt; B{OK?}</div>\n\
         <pre><code class=\"language-mermaid\">flowchart LR\n  X --&gt; Y</code></pre>\n\
         </body></html>",
    );
    let result = HtmlExtractor
        .extract(ExtractInput { artifact: &artifact, budget: ExtractBudget::default() }, &ExtractContext::default());

    let document = result.document.expect("document");
    let diagrams: Vec<_> = document
        .blocks
        .iter()
        .filter(|node| matches!(&node.block, Block::Diagram { language, .. } if language == "mermaid"))
        .collect();
    assert_eq!(diagrams.len(), 2, "both mermaid containers must become diagram blocks: {:?}", document.blocks);
    for node in &diagrams {
        let Block::Diagram { source, .. } = &node.block
        else {
            unreachable!()
        };
        assert!(source.contains("flowchart"), "authored source must be preserved verbatim: {source:?}");
        assert!(source.contains("-->"), "character references must decode back to authored edges: {source:?}");
        assert!(
            document.sources_for(node.id).iter().any(|s| matches!(s.kind, SourceKind::TextSpan { .. })),
            "diagram blocks must carry byte-span provenance"
        );
    }
    // Source tier is lossless: no bitmap or structural loss markers.
    assert!(!document.coverage.loss.iter().any(|loss| loss.code == "crawler.extract.diagram_bitmap"));
}

#[test]
fn inline_svg_is_registered_as_graphic_resource_reference() {
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><rect width=\"10\" height=\"10\"/></svg>";
    let artifact = artifact(
        "https://example.com/docs/rendered.html",
        &format!("<html><head><title>Rendered</title></head><body><p>before</p>{svg}<p>after</p></body></html>"),
    );
    let result = HtmlExtractor
        .extract(ExtractInput { artifact: &artifact, budget: ExtractBudget::default() }, &ExtractContext::default());

    let document = result.document.expect("document");
    let svg_node = document
        .blocks
        .iter()
        .find(|node| matches!(&node.block, Block::RawHtml { html } if html.contains("<svg")))
        .expect("inline svg must survive as a RawHtml block");
    let asset = document
        .assets
        .iter()
        .find(|asset| asset.media_type.as_deref() == Some("image/svg+xml"))
        .expect("inline svg must be registered as a graphic resource asset");
    assert_eq!(asset.status, document_ir::SemanticStatus::Resolved);
    let payload = String::from_utf8(asset.bytes.clone().expect("svg markup kept as asset bytes")).expect("utf8 svg");
    assert!(payload.contains("<svg"), "asset payload must be the exact svg markup: {payload:?}");
    assert!(
        document.relations.iter().any(|relation| {
            relation.kind == document_ir::RelationKind::Embeds
                && relation.source == document_ir::RelationEndpoint::Node(svg_node.id)
                && relation.target == document_ir::RelationEndpoint::Asset(asset.id)
        }),
        "the svg block must embed the registered asset"
    );
}

#[test]
fn bitmap_only_diagram_is_kept_and_marked_unstructured() {
    let artifact = artifact(
        "https://example.com/docs/bitmap.html",
        r#"<html><head><title>Bitmap</title></head><body><p><img class="mermaid" src="renders/flow.png" alt="flowchart"></p></body></html>"#,
    );
    let result = HtmlExtractor
        .extract(ExtractInput { artifact: &artifact, budget: ExtractBudget::default() }, &ExtractContext::default());

    let document = result.document.expect("document");
    // The raster image stays a first-class asset with its resolved URL.
    let asset = document
        .assets
        .iter()
        .find(|asset| asset.source.as_deref() == Some("https://example.com/docs/renders/flow.png"))
        .expect("the bitmap must be kept as an image asset");
    assert_eq!(asset.media_type.as_deref(), Some("image/png"));
    // The IR explicitly marks it: 位图，无结构 — no OCR, no silent text fallback.
    let marker = document
        .coverage
        .loss
        .iter()
        .find(|loss| loss.code == "crawler.extract.diagram_bitmap")
        .expect("bitmap-only diagrams must carry the explicit loss marker");
    assert!(marker.message.contains("位图，无结构"), "{:?}", marker.message);
    assert!(marker.message.contains("flowchart"), "alt text must be recorded: {:?}", marker.message);
    let body = String::from_utf8(artifact.body_bytes().expect("inline body").to_vec()).expect("utf8 html");
    let covered = loss_span_text(&document, "crawler.extract.diagram_bitmap", &body);
    assert!(covered.contains("<img"), "the bitmap loss must point at the authored image block: {covered:?}");
}

#[test]
fn plain_images_do_not_trigger_diagram_bitmap_loss() {
    let artifact = artifact(
        "https://example.com/docs/photo.html",
        r#"<html><head><title>Photo</title></head><body><p><img src="photo.png" alt="a photo"></p></body></html>"#,
    );
    let result = HtmlExtractor
        .extract(ExtractInput { artifact: &artifact, budget: ExtractBudget::default() }, &ExtractContext::default());

    let document = result.document.expect("document");
    assert_eq!(document.assets.len(), 1);
    assert!(
        !document.coverage.loss.iter().any(|loss| loss.code == "crawler.extract.diagram_bitmap"),
        "unmarked images are ordinary assets, not diagrams"
    );
    assert!(
        !document.coverage.loss.iter().any(|loss| loss.code == "crawler.extract.diagram_source_missing"),
        "plain inline svg carries no diagram-source loss"
    );
}

#[test]
fn rendered_mermaid_svg_without_source_keeps_a_span_loss() {
    let html = r#"<html><head><title>Rendered</title></head><body><p>before</p><svg xmlns="http://www.w3.org/2000/svg" aria-roledescription="flowchart-v2" viewBox="0 0 10 10"><rect width="10" height="10"/></svg><p>after</p></body></html>"#;
    let artifact = artifact("https://example.com/docs/rendered.html", html);
    let result = HtmlExtractor
        .extract(ExtractInput { artifact: &artifact, budget: ExtractBudget::default() }, &ExtractContext::default());

    let document = result.document.expect("document");
    assert_eq!(result.status, ExtractStatus::Partial, "missing source is a partial extraction");
    assert!(
        document.assets.iter().any(|asset| asset.media_type.as_deref() == Some("image/svg+xml")),
        "the rendered vector stays a graphic asset"
    );
    let body = String::from_utf8(artifact.body_bytes().expect("inline body").to_vec()).expect("utf8 html");
    let covered = loss_span_text(&document, "crawler.extract.diagram_source_missing", &body);
    assert!(covered.contains("<svg"), "the missing-source loss must point at the rendered markup: {covered:?}");
}

#[test]
fn html_links_resolve_against_page_base_and_keep_crawl_relations() {
    let artifact = artifact(
        "https://example.com/docs/page.html",
        r##"<html><head><base href="/guide/"></head><body><a href="next.html">Next</a><a href="#top">Top</a><a href="https://other.example/x">Out</a><img src="pic.png"></body></html>"##,
    );
    let result = HtmlExtractor
        .extract(ExtractInput { artifact: &artifact, budget: ExtractBudget::default() }, &ExtractContext::default());

    let links = result.discovered_links;
    assert!(
        links
            .iter()
            .any(|link| link.target == "https://example.com/guide/next.html" && link.relation == LinkRelation::Navigation),
        "{links:?}"
    );
    assert!(
        links.iter().any(|link| link.target == "https://other.example/x" && link.relation == LinkRelation::External),
        "{links:?}"
    );
    assert!(
        links.iter().any(|link| link.target == "https://example.com/guide/pic.png" && link.relation == LinkRelation::Asset),
        "{links:?}"
    );
    assert!(links.iter().all(|link| !link.target.contains("#top")), "{links:?}");
}

#[test]
fn import_failure_preserves_notedown_format_code_in_loss() {
    // Use invalid UTF-8 bytes so notedown-formats HTML import hits the
    // `from_utf8` guard and returns `FormatError::parse("html", ...)`.
    let mut artifact = artifact("https://example.com/broken.html", "<html></html>");
    // Override with non-UTF-8 bytes — invalid UTF-8 triggers FormatError::parse.
    artifact.body = crawler_types::BodyStorage::inline(vec![0xFF, 0xFE, 0x80]);
    let result = HtmlExtractor
        .extract(ExtractInput { artifact: &artifact, budget: ExtractBudget::default() }, &ExtractContext::default());

    assert_eq!(result.status, ExtractStatus::Failed);
    // The primary loss must carry a `notedown.format.*` code from the
    // formats-layer Diagnostic, not a free-form Crawler-only string.
    let primary_code = result.losses.first().map(|l| l.code.as_str()).unwrap_or("");
    assert!(primary_code.starts_with("notedown.format."), "expected notedown.format.* code, got {primary_code:?}");
    // Crawler-side context loss must still be present.
    assert!(result.losses.iter().any(|l| l.code == "crawler.extract.html_import_failed"));
}
