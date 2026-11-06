//! Frontier queue smoke.

use crawler_frontier::FrontierQueue;
use crawler_types::{DiscoverySource, FrontierItem};
use url::Url;

fn item(url: &str, priority: i32) -> FrontierItem {
    FrontierItem {
        request_url: Url::parse(url).expect("url"),
        discovery: DiscoverySource::Seed,
        depth: 0,
        site_key: "example.com".into(),
        priority,
        attempts: 0,
        discovered_at_epoch: 0,
    }
}

#[test]
fn deduplicates_request_url() {
    let mut queue = FrontierQueue::new();
    assert!(queue.push(item("https://example.com/a", 0)));
    assert!(!queue.push(item("https://example.com/a", 0)));
}

#[test]
fn pops_by_priority() {
    let mut queue = FrontierQueue::new();
    queue.push(item("https://example.com/slow", 5));
    queue.push(item("https://example.com/fast", -1));
    assert_eq!(queue.pop().expect("pop").request_url.as_str(), "https://example.com/fast");
}

#[test]
fn restore_rebuilds_dedup_set() {
    let mut queue = FrontierQueue::new();
    queue.restore(vec![item("https://example.com/a", 0)]);
    assert!(!queue.push(item("https://example.com/a", 0)));
    assert_eq!(queue.len(), 1);
}
