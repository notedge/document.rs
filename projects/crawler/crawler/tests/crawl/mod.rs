//! Extracted from `src/crawl.rs`.

use std::collections::BTreeMap;

use crawler::{
    CrawlOptions, Extractor, FixtureBrowserProvider, HtmlExtractor, resume_crawl_from_checkpoint, run_crawl,
    run_crawl_with_options, run_extract,
};
use crawler_fetch::{MemoryEntry, MemoryTransport};
use crawler_types::{
    BrowserFallbackPolicy, BrowserSnapshot, ChallengePolicy, CrawlBudgetStatus, CrawlEvent, CrawlRequest, CrawlScope,
    ExtractBudget, ExtractContext, ExtractStatus, PageChallengeState, diagnostics as pd,
};
use url::Url;

#[test]
fn crawls_multiple_seeds_in_one_request() {
    let mut transport = MemoryTransport::new();
    for (path, title) in [("/a", "A"), ("/b", "B")] {
        transport.insert(
            format!("https://example.com{path}"),
            MemoryEntry {
                status: 200,
                headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
                body: format!("<html><head><title>{title}</title></head><body>{title}</body></html>").into_bytes(),
            },
        );
    }
    let request = CrawlRequest::from_seeds(["https://example.com/a", "https://example.com/b"]).expect("seeds");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl(request, transport, &extractors).expect("crawl");
    assert_eq!(output.report.committed, 2);
}

#[test]
fn resume_rejects_browser_fallback_strategy_mismatch() {
    let seed = Url::parse("https://example.com/private").expect("url");
    let transport = MemoryTransport::new();
    let mut browser = FixtureBrowserProvider::new();
    browser.insert(BrowserSnapshot {
        requested_url: seed.clone(),
        final_url: Url::parse("https://example.com/login").expect("url"),
        document_html: "<html><body>login</body></html>".into(),
        captured_at_epoch: 1,
        browser_engine: "fixture".into(),
        profile_id: "test".into(),
        challenge_state: PageChallengeState::ChallengeRequired,
    });
    let request = CrawlRequest::from_seed(seed.as_str()).expect("seed");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let paused = run_crawl_with_options(
        request,
        transport,
        &extractors,
        CrawlOptions {
            browser: Some(&browser),
            browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
            challenge_policy: ChallengePolicy::PauseForOperator,
            ..Default::default()
        },
    )
    .expect("pause");
    let checkpoint = paused.checkpoint.expect("checkpoint");
    let error = resume_crawl_from_checkpoint(
        checkpoint,
        MemoryTransport::new(),
        &extractors,
        CrawlOptions { browser: Some(&browser), browser_fallback: BrowserFallbackPolicy::Never, ..Default::default() },
    )
    .expect_err("mismatch");
    assert!(error.to_string().contains("resume strategy mismatch"));
}

#[test]
fn crawls_linked_html_pages_offline() {
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: br#"<html><head><title>Home</title></head><body><a href="/next">next</a> hello</body></html>"#.to_vec(),
        },
    );
    transport.insert(
        "https://example.com/next",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: br#"<html><head><title>Next</title></head><body>next page</body></html>"#.to_vec(),
        },
    );

    let request = CrawlRequest::from_seed("https://example.com/").expect("seed");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl(request, transport, &extractors).expect("crawl");
    assert_eq!(output.report.committed, 2);
    assert_eq!(output.committed_pages.len(), 2);
}

#[test]
fn scope_gate_rejects_hosts_outside_the_allow_list() {
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: br#"<html><head><title>Home</title></head><body><a href="/in-scope">in</a></body></html>"#.to_vec(),
        },
    );
    transport.insert(
        "https://example.com/in-scope",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: br#"<html><head><title>In</title></head><body>in scope</body></html>"#.to_vec(),
        },
    );

    let mut request = CrawlRequest::from_seeds(["https://example.com/", "https://other.test/"]).expect("seeds");
    request.budget.max_depth = 1;
    request.scope = Some(CrawlScope { allowed_hosts: vec!["example.com".into()], allowed_domains: Vec::new() });
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl(request, transport, &extractors).expect("crawl");

    assert_eq!(output.report.committed, 2);
    assert!(output.report.events.events().iter().any(|event| matches!(
        event,
        CrawlEvent::Admitted { url, decision }
            if url.as_str() == "https://other.test/"
                && serde_json::to_string(decision).is_ok_and(|json| json.contains("crawler.reject.out-of-scope"))
    )));
    assert!(!output.committed_pages.iter().any(|page| page.contains("other.test")));
}

#[test]
fn scope_gate_rejects_the_seed_outside_the_allow_list() {
    let transport = MemoryTransport::new();
    let mut request = CrawlRequest::from_seed("https://example.com/").expect("seed");
    request.scope = Some(CrawlScope { allowed_hosts: Vec::new(), allowed_domains: vec!["other.test".into()] });
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl(request, transport, &extractors).expect("crawl");

    assert_eq!(output.report.committed, 0);
    assert_eq!(output.report.skipped, 1);
}

#[test]
fn budget_exhaustion_records_status_and_remaining_frontier() {
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: br#"<html><head><title>Home</title></head><body><a href="/a">a</a><a href="/b">b</a></body></html>"#.to_vec(),
        },
    );

    let mut request = CrawlRequest::from_seed("https://example.com/").expect("seed");
    request.budget.max_depth = 1;
    request.budget.max_requests = 1;
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl(request, transport, &extractors).expect("crawl");

    assert_eq!(output.report.committed, 1);
    assert_eq!(output.report.budget_status, CrawlBudgetStatus::Exhausted);
    assert_eq!(output.report.remaining_frontier, 2);
    assert!(output.report.diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code().as_str() == pd::CRAWL_BUDGET_EXHAUSTED
            && diagnostic.severity() == boost_diagnostic::DiagnosticSeverity::Warning
    }));
}

#[test]
fn crawl_skips_urls_denied_by_robots_txt() {
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/robots.txt",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/plain".into())]),
            body: b"User-agent: *\nDisallow: /secret\n".to_vec(),
        },
    );
    transport.insert(
        "https://example.com/",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: br#"<html><head><title>Home</title></head><body><a href="/secret">secret</a><a href="/open">open</a></body></html>"#.to_vec(),
        },
    );
    transport.insert(
        "https://example.com/open",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: br#"<html><head><title>Open</title></head><body>open page</body></html>"#.to_vec(),
        },
    );
    transport.insert(
        "https://example.com/secret",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: br#"<html><head><title>Secret</title></head><body>secret page</body></html>"#.to_vec(),
        },
    );

    let request = CrawlRequest::from_seed("https://example.com/").expect("seed");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl(request, transport, &extractors).expect("crawl");
    assert_eq!(output.report.committed, 2);
    assert_eq!(output.report.failed, 1);
    assert!(output.report.events.events().iter().any(|event| matches!(
        event,
        CrawlEvent::Failed { url, reason }
            if url.as_str() == "https://example.com/secret"
                && reason.contains("robots policy denied")
    )));
}

#[test]
fn browser_fallback_commits_when_http_fetch_missing() {
    let transport = MemoryTransport::new();
    let mut browser = FixtureBrowserProvider::new();
    browser.insert(BrowserSnapshot {
        requested_url: Url::parse("https://example.com/js-page").expect("url"),
        final_url: Url::parse("https://example.com/js-page").expect("url"),
        document_html: "<html><head><title>JS</title></head><body><p>rendered</p></body></html>".into(),
        captured_at_epoch: 1,
        browser_engine: "fixture".into(),
        profile_id: "test".into(),
        challenge_state: PageChallengeState::Normal,
    });

    let request = CrawlRequest::from_seed("https://example.com/js-page").expect("seed");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl_with_options(
        request,
        transport,
        &extractors,
        CrawlOptions {
            browser: Some(&browser),
            browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
            challenge_policy: ChallengePolicy::Stop,
            ..Default::default()
        },
    )
    .expect("crawl");
    assert_eq!(output.report.committed, 1);
    assert!(output.report.events.events().iter().any(|event| matches!(event, CrawlEvent::BrowserFallback { .. })));
}

#[test]
fn browser_fallback_skips_challenge_page_when_policy_allows() {
    let transport = MemoryTransport::new();
    let mut browser = FixtureBrowserProvider::new();
    browser.insert(BrowserSnapshot {
        requested_url: Url::parse("https://example.com/private").expect("url"),
        final_url: Url::parse("https://example.com/login").expect("url"),
        document_html: "<html><body>login</body></html>".into(),
        captured_at_epoch: 1,
        browser_engine: "fixture".into(),
        profile_id: "test".into(),
        challenge_state: PageChallengeState::LoginRequired,
    });

    let request = CrawlRequest::from_seed("https://example.com/private").expect("seed");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl_with_options(
        request,
        transport,
        &extractors,
        CrawlOptions {
            browser: Some(&browser),
            browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
            challenge_policy: ChallengePolicy::SkipPage,
            ..Default::default()
        },
    )
    .expect("crawl");
    assert_eq!(output.report.skipped, 1);
    assert_eq!(output.report.failed, 0);
    assert_eq!(output.report.committed, 0);
}

#[test]
fn browser_fallback_fails_challenge_page_when_policy_stops() {
    let transport = MemoryTransport::new();
    let mut browser = FixtureBrowserProvider::new();
    browser.insert(BrowserSnapshot {
        requested_url: Url::parse("https://example.com/private").expect("url"),
        final_url: Url::parse("https://example.com/login").expect("url"),
        document_html: "<html><body>login</body></html>".into(),
        captured_at_epoch: 1,
        browser_engine: "fixture".into(),
        profile_id: "test".into(),
        challenge_state: PageChallengeState::LoginRequired,
    });

    let request = CrawlRequest::from_seed("https://example.com/private").expect("seed");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl_with_options(
        request,
        transport,
        &extractors,
        CrawlOptions {
            browser: Some(&browser),
            browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
            challenge_policy: ChallengePolicy::Stop,
            ..Default::default()
        },
    )
    .expect("crawl");
    assert_eq!(output.report.skipped, 0);
    assert_eq!(output.report.failed, 1);
    assert_eq!(output.report.committed, 0);
}

#[test]
fn browser_fallback_pauses_and_resumes_after_operator_snapshot() {
    let seed = Url::parse("https://example.com/private").expect("url");
    let transport = MemoryTransport::new();
    let mut browser = FixtureBrowserProvider::new();
    browser.insert(BrowserSnapshot {
        requested_url: seed.clone(),
        final_url: Url::parse("https://example.com/login").expect("url"),
        document_html: "<html><body>login</body></html>".into(),
        captured_at_epoch: 1,
        browser_engine: "fixture".into(),
        profile_id: "test".into(),
        challenge_state: PageChallengeState::ChallengeRequired,
    });

    let request = CrawlRequest::from_seed(seed.as_str()).expect("seed");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let options = CrawlOptions {
        browser: Some(&browser),
        browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
        challenge_policy: ChallengePolicy::PauseForOperator,
        ..Default::default()
    };
    let paused = run_crawl_with_options(request, transport, &extractors, options).expect("pause");
    assert_eq!(paused.report.paused, 1);
    let checkpoint = paused.checkpoint.expect("checkpoint");
    let strategy_fp = checkpoint.strategy_fingerprint.clone();
    let attempt_id = checkpoint.attempt_identity;

    browser.insert(BrowserSnapshot {
        requested_url: seed.clone(),
        final_url: seed.clone(),
        document_html: "<html><head><title>OK</title></head><body><p>ready</p></body></html>".into(),
        captured_at_epoch: 2,
        browser_engine: "fixture".into(),
        profile_id: "test".into(),
        challenge_state: PageChallengeState::Normal,
    });

    let resumed = resume_crawl_from_checkpoint(
        checkpoint,
        MemoryTransport::new(),
        &extractors,
        CrawlOptions {
            browser: Some(&browser),
            browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
            challenge_policy: ChallengePolicy::Stop,
            ..Default::default()
        },
    )
    .expect("resume");
    assert_eq!(resumed.report.committed, 1);
    assert_eq!(resumed.report.paused, 1);
    assert_eq!(resumed.checkpoint, None);
    assert!(strategy_fp.is_some());
    assert_eq!(attempt_id, 0);
}

#[test]
fn disk_cache_reuses_response_without_transport() {
    let dir = std::env::temp_dir().join(format!("crawler-crawl-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/cache-page",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: br#"<html><head><title>Cached</title></head><body>cached</body></html>"#.to_vec(),
        },
    );
    let request = CrawlRequest::from_seed("https://example.com/cache-page").expect("seed");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let options = CrawlOptions { cache_dir: Some(dir.as_path()), ..Default::default() };
    let first = run_crawl_with_options(request.clone(), transport, &extractors, options).expect("first crawl");
    assert_eq!(first.report.committed, 1);

    let second = run_crawl_with_options(
        request,
        MemoryTransport::new(),
        &extractors,
        CrawlOptions { cache_dir: Some(dir.as_path()), ..Default::default() },
    )
    .expect("second crawl");
    assert_eq!(second.report.committed, 1);
    assert!(second.report.events.events().iter().any(|event| matches!(event, CrawlEvent::Fetched { cache_hit: true, .. })));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn crawl_discovers_sitemap_urls_after_robots_loaded() {
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/robots.txt",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/plain".into())]),
            body: b"User-agent: *\nSitemap: https://example.com/sitemap.xml\n".to_vec(),
        },
    );
    transport.insert(
        "https://example.com/",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: br#"<html><head><title>Home</title></head><body>home</body></html>"#.to_vec(),
        },
    );
    transport.insert(
        "https://example.com/sitemap.xml",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "application/xml".into())]),
            body: br#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://example.com/from-sitemap</loc></url>
</urlset>"#
                .to_vec(),
        },
    );
    transport.insert(
        "https://example.com/from-sitemap",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: br#"<html><head><title>Sitemap</title></head><body>from sitemap</body></html>"#.to_vec(),
        },
    );
    let request = CrawlRequest::from_seed("https://example.com/").expect("seed");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl(request, transport, &extractors).expect("crawl");
    assert_eq!(output.report.committed, 2);
    assert!(output.report.events.events().iter().any(|event| matches!(
        event,
        CrawlEvent::Discovered { url, .. }
            if url.as_str() == "https://example.com/from-sitemap"
    )));
}

#[test]
fn extract_mode_reuses_artifact_without_transport() {
    let artifact = crawler_fetch::build_artifact(
        url::Url::parse("https://example.com/page").expect("url"),
        url::Url::parse("https://example.com/page").expect("url"),
        200,
        BTreeMap::from([("content-type".into(), "text/html".into())]),
        crawler_types::BodyStorage::inline(b"<html><head><title>T</title></head><body>body</body></html>"),
        Vec::new(),
        crawler_types::TransportProvenance::Test,
        None,
    );
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let result = run_extract(&artifact, &extractors, ExtractBudget::default(), &ExtractContext::default());
    assert_eq!(result.status, ExtractStatus::Partial);
    assert!(result.document.is_some());
}
