//! Response and robots caches persisted through the store survive a restart.

use std::{cell::Cell, collections::BTreeMap, rc::Rc};

use crawler::{Extractor, HtmlExtractor, run_crawl_with_store};
use crawler_fetch::{FetchRequest, MemoryEntry, MemoryTransport, Transport};
#[cfg(feature = "yydb")]
use crawler_store::YydbStore;
use crawler_store::{MemoryStore, Store, StoreConfig};
use crawler_types::{CrawlEvent, CrawlRequest, FetchArtifact, Result};

#[derive(Clone)]
struct CountingTransport {
    inner: MemoryTransport,
    fetches: Rc<Cell<usize>>,
}

impl Transport for CountingTransport {
    fn fetch(&self, request: &FetchRequest) -> Result<FetchArtifact> {
        self.fetches.set(self.fetches.get() + 1);
        self.inner.fetch(request)
    }
}

fn transport(fetches: &Rc<Cell<usize>>) -> CountingTransport {
    let mut inner = MemoryTransport::new();
    inner.insert(
        "https://example.com/robots.txt",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/plain".into())]),
            body: b"User-agent: *\nAllow: /".to_vec(),
        },
    );
    inner.insert(
        "https://example.com/a",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: b"<html><head><title>A</title></head><body>Hello from the page body.</body></html>".to_vec(),
        },
    );
    inner.insert(
        "https://example.com/b",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: b"<html><head><title>B</title></head><body>Second page body survives the restart.</body></html>".to_vec(),
        },
    );
    CountingTransport { inner, fetches: fetches.clone() }
}

/// Pre-populate the store response cache for a second seed so a restarted run
/// can be served without touching the transport.
fn cache_second_seed(store: &mut dyn Store) {
    let mut inner = MemoryTransport::new();
    inner.insert(
        "https://example.com/b",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: b"<html><head><title>B</title></head><body>Second page body survives the restart.</body></html>".to_vec(),
        },
    );
    let request = FetchRequest { url: "https://example.com/b".parse().expect("url"), headers: BTreeMap::new() };
    let artifact = inner.fetch(&request).expect("fixture artifact");
    store.put_response_cache(&artifact).expect("cache second seed");
}

#[cfg(feature = "yydb")]
fn temp_db_path(label: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    std::env::temp_dir().join(format!("crawler-{label}-{}-{nanos}.yydx", std::process::id()))
}

#[cfg(feature = "yydb")]
fn cleanup_db(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm", "-lock"] {
        let mut target = path.to_path_buf().into_os_string();
        target.push(suffix);
        let _ = std::fs::remove_file(std::path::PathBuf::from(target));
    }
    let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("crawler");
    let _ = std::fs::remove_dir(path.with_file_name(format!("{stem}-objects")));
}

#[cfg(feature = "yydb")]
#[test]
fn yydb_store_backed_caches_survive_a_restart_without_refetching() {
    let path = temp_db_path("cache-restart");
    let mut store = YydbStore::open(StoreConfig::yydb(&path)).expect("open yydb store");
    let fetches = Rc::new(Cell::new(0));
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let request = CrawlRequest::from_seed("https://example.com/a").expect("seed");

    let first =
        run_crawl_with_store(request, transport(&fetches), &extractors, Default::default(), &mut store).expect("first crawl");
    assert_eq!(first.report.committed, 1);
    assert_eq!(fetches.get(), 2, "robots and page must be fetched once");

    store.reopen().expect("restart");
    cache_second_seed(&mut store);
    let second_request = CrawlRequest::from_seed("https://example.com/b").expect("second seed");
    let second = run_crawl_with_store(second_request, transport(&fetches), &extractors, Default::default(), &mut store)
        .expect("second crawl");
    assert_eq!(second.report.committed, 1);
    assert_eq!(fetches.get(), 2, "restart must reuse stored robots and response");

    drop(store);
    cleanup_db(&path);
}

#[test]
fn store_backed_caches_survive_a_restart_without_refetching() {
    let mut store = MemoryStore::open(StoreConfig::memory()).expect("open store");
    let fetches = Rc::new(Cell::new(0));
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let request = CrawlRequest::from_seed("https://example.com/a").expect("seed");

    let first =
        run_crawl_with_store(request, transport(&fetches), &extractors, Default::default(), &mut store).expect("first crawl");
    assert_eq!(first.report.committed, 1);
    assert_eq!(fetches.get(), 2, "robots and page must be fetched once");

    store.reopen().expect("restart");
    cache_second_seed(&mut store);
    let second_request = CrawlRequest::from_seed("https://example.com/b").expect("second seed");
    let second = run_crawl_with_store(second_request, transport(&fetches), &extractors, Default::default(), &mut store)
        .expect("second crawl");
    assert_eq!(second.report.committed, 1);
    assert_eq!(fetches.get(), 2, "restart must reuse stored robots and response");
    assert!(
        second.report.events.events().iter().any(|event| matches!(event, CrawlEvent::Fetched { cache_hit: true, .. })),
        "second run must report a cache hit"
    );
}
