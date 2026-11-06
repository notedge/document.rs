//! Extracted from `src/client.rs`.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crawler_fetch::{
    FetchClient, MemoryEntry, MemoryTransport, RobotsPolicy, Transport, fetch_url, retry_after_delay, site_key_for_url,
};
use crawler_types::{BodyStorage, CrawlError, FetchArtifact, HttpStatus, PolitenessProfile, TransportProvenance};
use url::Url;

/// Test transport that plays back artifacts or errors in FIFO order.
#[derive(Clone)]
struct SeqTransport {
    queue: Arc<Mutex<Vec<std::result::Result<FetchArtifact, CrawlError>>>>,
}

impl SeqTransport {
    fn new(items: Vec<std::result::Result<FetchArtifact, CrawlError>>) -> Self {
        Self { queue: Arc::new(Mutex::new(items)) }
    }

    fn remaining(&self) -> usize {
        self.queue.lock().unwrap().len()
    }

    fn artifact(url: &str, status: HttpStatus, headers: BTreeMap<String, String>, body: &[u8]) -> FetchArtifact {
        let request_url = Url::parse(url).unwrap();
        crawler_fetch::build_artifact(
            request_url.clone(),
            request_url,
            status,
            headers,
            BodyStorage::inline(body),
            Vec::new(),
            TransportProvenance::Test,
            None,
        )
    }
}

impl Transport for SeqTransport {
    fn fetch(&self, _request: &crawler_fetch::FetchRequest) -> std::result::Result<FetchArtifact, CrawlError> {
        self.queue.lock().unwrap().remove(0)
    }
}

#[test]
fn cache_hit_marks_provenance() {
    let mut transport = MemoryTransport::new();
    transport.insert("https://example.com/", MemoryEntry { status: 200, headers: Default::default(), body: b"hello".to_vec() });
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(PolitenessProfile::Developer));
    let url = Url::parse("https://example.com/").expect("url");
    let first = fetch_url(&mut client, url.clone()).expect("first");
    assert_eq!(first.transport_provenance, TransportProvenance::Test);
    let second = fetch_url(&mut client, url).expect("second");
    assert_eq!(second.transport_provenance, TransportProvenance::CacheHit);
}

#[test]
fn robots_denies_admin_path() {
    let transport = MemoryTransport::new();
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(PolitenessProfile::Conservative));
    let url = Url::parse("https://example.com/admin/page").expect("url");
    let error = fetch_url(&mut client, url).expect_err("denied");
    assert!(matches!(error, CrawlError::Diagnostic(_)));
    assert_eq!(error.code(), Some("crawler.robots.denied"));
}

#[test]
fn loads_robots_txt_before_fetching_page() {
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/robots.txt",
        MemoryEntry { status: 200, headers: Default::default(), body: b"User-agent: *\nDisallow: /secret\n".to_vec() },
    );
    transport
        .insert("https://example.com/open", MemoryEntry { status: 200, headers: Default::default(), body: b"open".to_vec() });
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(PolitenessProfile::Developer));
    let allowed = Url::parse("https://example.com/open").expect("url");
    fetch_url(&mut client, allowed.clone()).expect("allowed page");
    let denied = Url::parse("https://example.com/secret/page").expect("url");
    let error = fetch_url(&mut client, denied).expect_err("denied by robots.txt");
    assert!(matches!(error, CrawlError::Diagnostic(_)));
    assert_eq!(error.code(), Some("crawler.robots.denied"));
    assert!(client.robots().has_site_rules(&allowed));
}

#[test]
fn missing_robots_txt_uses_fallback_paths() {
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/private/data",
        MemoryEntry { status: 200, headers: Default::default(), body: b"private".to_vec() },
    );
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(PolitenessProfile::Conservative));
    let url = Url::parse("https://example.com/private/data").expect("url");
    let error = fetch_url(&mut client, url).expect_err("fallback deny");
    assert!(matches!(error, CrawlError::Diagnostic(_)));
    assert_eq!(error.code(), Some("crawler.robots.denied"));
}

#[test]
fn applies_loaded_crawl_delay_between_same_site_requests() {
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/robots.txt",
        MemoryEntry { status: 200, headers: Default::default(), body: b"User-agent: *\nCrawl-delay: 1\n".to_vec() },
    );
    for path in ["/one", "/two"] {
        transport.insert(
            format!("https://example.com{path}"),
            MemoryEntry { status: 200, headers: Default::default(), body: path.as_bytes().to_vec() },
        );
    }
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(PolitenessProfile::Developer));
    fetch_url(&mut client, Url::parse("https://example.com/one").expect("url")).expect("first page");
    let started = Instant::now();
    fetch_url(&mut client, Url::parse("https://example.com/two").expect("url")).expect("second page");
    assert!(started.elapsed() >= Duration::from_millis(900));
}

#[test]
fn failed_transport_attempt_keeps_site_timing_state() {
    let mut policy = RobotsPolicy::from_profile(PolitenessProfile::Developer);
    let url = Url::parse("https://example.com/missing").expect("url");
    let site = site_key_for_url(&url);
    policy.load_site_rules(&site, "User-agent: *\nCrawl-delay: 1\n");
    let mut client = FetchClient::new(MemoryTransport::new(), policy);
    fetch_url(&mut client, url).expect_err("missing fixture");
    assert!(client.has_tracked_site(&site));
}

#[test]
fn retryable_status_is_returned_without_transport_retry_or_sleep() {
    let url = Url::parse("file:///seq").expect("url");
    let transport = SeqTransport::new(vec![Ok(SeqTransport::artifact(
        url.as_str(),
        429,
        BTreeMap::from([("retry-after".into(), "1".into())]),
        b"rate-limited".as_slice(),
    ))]);
    let mut client = FetchClient::new(transport.clone(), RobotsPolicy::from_profile(PolitenessProfile::Developer));
    let started = Instant::now();
    let artifact = fetch_url(&mut client, url).expect("retryable status stays an artifact");
    assert_eq!(artifact.status, 429);
    assert!(started.elapsed() < Duration::from_millis(500), "the transport must not sleep on retry");
    assert_eq!(transport.remaining(), 0, "only one attempt is made");
    assert_eq!(retry_after_delay(&artifact), Some(Duration::from_secs(1)));
}

#[test]
fn retry_after_requires_a_numeric_header() {
    let url = Url::parse("file:///seq").expect("url");
    let without = SeqTransport::artifact(url.as_str(), 503, BTreeMap::new(), b"busy");
    assert_eq!(retry_after_delay(&without), None);
    let with_date = SeqTransport::artifact(
        url.as_str(),
        503,
        BTreeMap::from([("retry-after".into(), "Wed, 21 Oct 2015 07:28:00 GMT".into())]),
        b"busy",
    );
    assert_eq!(retry_after_delay(&with_date), None, "HTTP-date Retry-After is not supported");
}
