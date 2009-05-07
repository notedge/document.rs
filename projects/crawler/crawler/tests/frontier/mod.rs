//! Extracted from `src/frontier.rs`.

use crawler::{FrontierQueue, frontier_from_links, seed_frontier};
use crawler_types::{CrawlRequest, LinkCandidate, LinkRelation};

#[test]
fn deduplicates_frontier_items() {
    let mut queue = FrontierQueue::new();
    let request = CrawlRequest::from_seed("https://example.com/a").expect("seed");
    let items = seed_frontier(&request).expect("items");
    assert!(queue.push(items[0].clone()));
    assert!(!queue.push(items[0].clone()));
}

#[test]
fn expands_navigation_links() {
    let mut queue = FrontierQueue::new();
    let request = CrawlRequest::from_seed("https://example.com/").expect("seed");
    let mut items = seed_frontier(&request).expect("items");
    let source = items.remove(0);
    queue.push(source.clone());
    let links = vec![LinkCandidate {
        target: "https://example.com/next".into(),
        relation: LinkRelation::Navigation,
        source_hint: None,
    }];
    let admitted = frontier_from_links(&mut queue, &source, &links, 2);
    assert_eq!(admitted.len(), 1);
    assert_eq!(queue.len(), 2);
}
