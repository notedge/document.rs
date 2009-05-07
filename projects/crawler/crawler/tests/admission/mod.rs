//! Extracted from `src/admission.rs`.

use crawler::{admit_frontier_item, seed_frontier};
use crawler_types::{AdmissionDecision, CrawlRequest};

#[test]
fn rejects_items_beyond_depth() {
    let request = CrawlRequest::from_seed("https://example.com").expect("seed");
    let mut items = seed_frontier(&request).expect("frontier");
    items[0].depth = request.budget.max_depth + 1;
    assert!(matches!(admit_frontier_item(&items[0], &request), AdmissionDecision::Rejected { .. }));
}
