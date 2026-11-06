//! 架构约束：`frontier_from_links` 可替换队列。

use crawler_frontier::{FrontierQueue, FrontierSink, frontier_from_links};
use crawler_types::{DiscoverySource, FrontierItem, LinkCandidate, LinkRelation};
use url::Url;

#[derive(Debug, Default)]
struct RecordingSink {
    admitted: Vec<FrontierItem>,
}

impl FrontierSink for RecordingSink {
    fn push(&mut self, item: FrontierItem) -> bool {
        self.admitted.push(item);
        true
    }
}

#[derive(Debug, Default)]
struct RejectingSink;

impl FrontierSink for RejectingSink {
    fn push(&mut self, _item: FrontierItem) -> bool {
        false
    }
}

fn seed_item() -> FrontierItem {
    FrontierItem {
        request_url: Url::parse("https://example.com/").expect("url"),
        discovery: DiscoverySource::Seed,
        depth: 0,
        site_key: "example.com".into(),
        priority: 0,
        attempts: 0,
        discovered_at_epoch: 0,
    }
}

fn navigation_link(target: &str) -> LinkCandidate {
    LinkCandidate { target: target.into(), relation: LinkRelation::Navigation, source_hint: None }
}

#[test]
fn frontier_links_accept_replaceable_sink() {
    let mut sink = RecordingSink::default();
    let source = seed_item();
    let links = [navigation_link("https://example.com/next"), navigation_link("https://example.com/other")];
    let admitted = frontier_from_links(&mut sink, &source, &links, 2);
    assert_eq!(admitted.len(), 2);
    assert_eq!(sink.admitted.len(), 2);
    assert_eq!(sink.admitted[0].depth, 1);
}

#[test]
fn frontier_links_respect_sink_rejection() {
    let mut sink = RejectingSink;
    let source = seed_item();
    let links = [navigation_link("https://example.com/next")];
    assert!(frontier_from_links(&mut sink, &source, &links, 2).is_empty());
}

#[test]
fn frontier_queue_still_implements_sink() {
    let mut queue = FrontierQueue::new();
    let source = seed_item();
    let links = [navigation_link("https://example.com/next")];
    assert_eq!(frontier_from_links(&mut queue, &source, &links, 2).len(), 1);
    assert_eq!(queue.len(), 1);
}
