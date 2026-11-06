//! G-PANDARK-3 集成：crawl 循环经 store frontier 认领并以页面事务提交。

use std::{cell::Cell, collections::BTreeMap, rc::Rc};

use crawler::{Extractor, HtmlExtractor, run_crawl_with_store};
use crawler_fetch::{FetchRequest, MemoryEntry, MemoryTransport, Transport};
use crawler_store::{MemoryStore, PagePhase, Store, StoreConfig, keyspace::item_id_from_url};
use crawler_types::{CrawlRequest, FetchArtifact, PolitenessProfile, Result};

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

fn transport() -> CountingTransport {
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
            body: b"<html><body><p>Page A body.</p><a href=\"/b\">next</a></body></html>".to_vec(),
        },
    );
    inner.insert(
        "https://example.com/b",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: b"<html><body><p>Page B body.</p></body></html>".to_vec(),
        },
    );
    CountingTransport { inner, fetches: Rc::new(Cell::new(0)) }
}

#[test]
fn store_crawl_commits_each_page_through_the_store_frontier() {
    let mut store = MemoryStore::open(StoreConfig::memory()).expect("open store");
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let mut request = CrawlRequest::from_seed("https://example.com/a").expect("seed");
    request.politeness = PolitenessProfile::Developer;

    let output = run_crawl_with_store(request, transport(), &extractors, Default::default(), &mut store).expect("crawl");
    assert_eq!(output.report.committed, 2, "seed and discovered link both commit");
    assert_eq!(store.pending_frontier_count().expect("frontier"), 0, "done items leave the store frontier");
    assert_eq!(store.page_phase(&item_id_from_url("https://example.com/a")).expect("page a"), PagePhase::Committed);
    assert_eq!(store.page_phase(&item_id_from_url("https://example.com/b")).expect("page b"), PagePhase::Committed);
    assert!(store.event_count().expect("events") >= 2, "the page commits persist their report events");
}

#[test]
fn store_crawl_never_half_commits_an_unparsable_page() {
    let mut store = MemoryStore::open(StoreConfig::memory()).expect("open store");
    let mut inner = transport();
    inner.inner.insert(
        "https://example.com/a",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "application/octet-stream".into())]),
            body: b"not a document".to_vec(),
        },
    );
    let html = HtmlExtractor;
    let extractors: [&dyn Extractor; 1] = [&html];
    let request = CrawlRequest::from_seed("https://example.com/a").expect("seed");

    let output = run_crawl_with_store(request, inner, &extractors, Default::default(), &mut store).expect("crawl");
    assert_eq!(output.report.committed, 0);
    assert_eq!(output.report.failed, 1);
    assert_eq!(store.pending_frontier_count().expect("frontier"), 0, "the failed item is released");
    assert!(
        !matches!(store.page_phase(&item_id_from_url("https://example.com/a")), Ok(PagePhase::Committed)),
        "an unparsable page must not leave a committed record"
    );
    assert_eq!(store.event_count().expect("events"), 0, "an uncommitted page writes no events");
}
