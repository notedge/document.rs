//! Extracted from `src/crawl.rs`.

use std::collections::BTreeMap;

use crawler::{
    CancelSignal, CrawlOptions, Extractor, FixtureBrowserProvider, HtmlExtractor, resume_crawl_from_store, run_crawl,
    run_crawl_with_options, run_crawl_with_store, run_extract,
};
use crawler_fetch::{MemoryEntry, MemoryTransport};
use crawler_store::{MemoryStore, Store, StoreConfig};
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
    let mut store = MemoryStore::open(StoreConfig::memory()).expect("store");
    let paused = run_crawl_with_store(
        request,
        transport,
        &extractors,
        CrawlOptions {
            browser: Some(&browser),
            browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
            challenge_policy: ChallengePolicy::PauseForOperator,
            ..Default::default()
        },
        &mut store,
    )
    .expect("pause");
    let run_id = paused.run_id.expect("run id");
    store.reopen().expect("reopen");
    let resume = store.resume(&run_id).expect("resume");
    let error = resume_crawl_from_store(
        resume,
        MemoryTransport::new(),
        &extractors,
        CrawlOptions { browser: Some(&browser), browser_fallback: BrowserFallbackPolicy::Never, ..Default::default() },
        &mut store,
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
    assert_eq!(output.report.failed, 0);
    assert_eq!(output.report.skipped, 1);
    assert!(output.report.events.events().iter().any(|event| matches!(
        event,
        CrawlEvent::Skipped { url, reason }
            if url.as_str() == "https://example.com/secret" && reason == "robots-denied"
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
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/robots.txt",
        MemoryEntry { status: 200, headers: BTreeMap::from([("content-type".into(), "text/plain".into())]), body: Vec::new() },
    );
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
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/robots.txt",
        MemoryEntry { status: 200, headers: BTreeMap::from([("content-type".into(), "text/plain".into())]), body: Vec::new() },
    );
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
    let mut store = MemoryStore::open(StoreConfig::memory()).expect("store");
    let paused = run_crawl_with_store(request, transport, &extractors, options, &mut store).expect("pause");
    assert_eq!(paused.report.paused, 1);
    let run_id = paused.run_id.expect("run id");
    store.reopen().expect("reopen");
    let resume = store.resume(&run_id).expect("resume");
    assert!(resume.strategy_fingerprint.is_some());
    assert_eq!(resume.committed_pages, 0);

    browser.insert(BrowserSnapshot {
        requested_url: seed.clone(),
        final_url: seed.clone(),
        document_html: "<html><head><title>OK</title></head><body><p>ready</p></body></html>".into(),
        captured_at_epoch: 2,
        browser_engine: "fixture".into(),
        profile_id: "test".into(),
        challenge_state: PageChallengeState::Normal,
    });

    let resumed = resume_crawl_from_store(
        resume,
        MemoryTransport::new(),
        &extractors,
        CrawlOptions {
            browser: Some(&browser),
            browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
            challenge_policy: ChallengePolicy::Stop,
            ..Default::default()
        },
        &mut store,
    )
    .expect("resume");
    assert_eq!(resumed.report.committed, 1);
    assert_eq!(resumed.report.paused, 0);
    assert_eq!(resumed.run_id.as_deref(), Some(run_id.as_str()));
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

/// Cancellation flag shared with the run loop.
struct Flag(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl CancelSignal for Flag {
    fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Transport that trips the cancellation flag after serving a page request.
struct CancelOnPageFetch {
    inner: MemoryTransport,
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl crawler_fetch::Transport for CancelOnPageFetch {
    fn fetch(&self, request: &crawler_fetch::FetchRequest) -> crawler_types::Result<crawler_types::FetchArtifact> {
        let artifact = self.inner.fetch(request);
        if !request.url.path().eq_ignore_ascii_case("/robots.txt") {
            self.flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        artifact
    }
}

fn page_transport(paths: &[(&str, &str)]) -> MemoryTransport {
    let mut transport = MemoryTransport::new();
    for (path, title) in paths {
        transport.insert(
            format!("https://example.com{path}"),
            MemoryEntry {
                status: 200,
                headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
                body: format!("<html><head><title>{title}</title></head><body>{title}</body></html>").into_bytes(),
            },
        );
    }
    transport
}

#[test]
fn cancelled_run_stops_before_admission() {
    let flag = Flag(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)));
    let request = CrawlRequest::from_seeds(["https://example.com/a", "https://example.com/b"]).expect("seeds");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl_with_options(
        request,
        page_transport(&[("/a", "A"), ("/b", "B")]),
        &extractors,
        CrawlOptions { cancel: Some(&flag), ..Default::default() },
    )
    .expect("crawl");
    assert_eq!(output.report.budget_status, CrawlBudgetStatus::Cancelled);
    assert_eq!(output.report.remaining_frontier, 2);
    assert_eq!(output.report.committed, 0);
    assert!(
        output.report.diagnostics.diagnostics.iter().any(|diagnostic| diagnostic.code().as_str() == pd::RUN_CANCELLED),
        "cancelled runs carry a structured diagnostic"
    );
}

#[test]
fn cancellation_after_a_page_keeps_the_remainder_pending() {
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let transport = CancelOnPageFetch { inner: page_transport(&[("/a", "A"), ("/b", "B")]), flag: flag.clone() };
    let request = CrawlRequest::from_seeds(["https://example.com/a", "https://example.com/b"]).expect("seeds");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let mut store = MemoryStore::open(StoreConfig::memory()).expect("store");
    let output = run_crawl_with_store(
        request,
        transport,
        &extractors,
        CrawlOptions { cancel: Some(&Flag(flag)), ..Default::default() },
        &mut store,
    )
    .expect("crawl");
    assert_eq!(output.report.committed, 1);
    assert_eq!(output.report.budget_status, CrawlBudgetStatus::Cancelled);
    assert_eq!(output.report.remaining_frontier, 1);
    assert_eq!(store.pending_frontier_count().expect("pending count"), 1, "the unstarted item stays in the frontier");
}

#[test]
fn robots_denied_pages_are_skipped_not_failed() {
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/robots.txt",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/plain".into())]),
            body: b"User-agent: *\nDisallow: /blocked\n".to_vec(),
        },
    );
    let request = CrawlRequest::from_seed("https://example.com/blocked").expect("seed");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let output = run_crawl(request, transport, &extractors).expect("crawl");
    assert_eq!(output.report.skipped, 1);
    assert_eq!(output.report.failed, 0);
    assert!(output.report.events.events().iter().any(|event| matches!(
        event,
        CrawlEvent::Skipped { reason, .. } if reason == "robots-denied"
    )));
}
