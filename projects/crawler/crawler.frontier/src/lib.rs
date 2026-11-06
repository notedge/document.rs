//! Frontier queue and navigation link admission.
//!
//! The crate owns bounded in-memory scheduling and link expansion. It depends
//! on `crawler-types` only, so the crawl orchestrator can replace the queue
//! through [`FrontierSink`] without pulling transport or persistence into the
//! frontier layer.

#![warn(missing_docs)]

use std::collections::HashSet;

use crawler_types::{FrontierItem, LinkCandidate, LinkRelation};

/// Destination for admitted frontier items.
pub trait FrontierSink {
    /// Enqueue an item when its request URL is not yet known.
    ///
    /// Returns `false` for a duplicate or a rejected item.
    fn push(&mut self, item: FrontierItem) -> bool;
}

/// Bounded frontier queue with URL deduplication.
#[derive(Debug, Clone, Default)]
pub struct FrontierQueue {
    seen: HashSet<String>,
    pending: Vec<FrontierItem>,
}

impl FrontierQueue {
    /// Empty frontier queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the canonical request URL is already known.
    pub fn contains(&self, request_url: &str) -> bool {
        self.seen.contains(request_url)
    }

    /// Enqueue an item when not yet seen.
    pub fn push(&mut self, item: FrontierItem) -> bool {
        let key = item.request_url.to_string();
        if !self.seen.insert(key) {
            return false;
        }
        self.pending.push(item);
        true
    }

    /// Re-enqueue a frontier item after operator resume, replacing any pending copy.
    pub fn requeue(&mut self, item: FrontierItem) {
        let key = item.request_url.to_string();
        self.pending.retain(|pending| pending.request_url.to_string() != key);
        self.seen.remove(&key);
        self.push(item);
    }

    /// Pop the next frontier item by priority then discovery time.
    pub fn pop(&mut self) -> Option<FrontierItem> {
        if self.pending.is_empty() {
            return None;
        }
        let index = self
            .pending
            .iter()
            .enumerate()
            .min_by_key(|(_, item)| (item.priority, item.discovered_at_epoch))
            .map(|(index, _)| index)
            .expect("non-empty pending");
        Some(self.pending.remove(index))
    }

    /// Number of pending items.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Whether the queue has no pending items.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Pending frontier items in scheduling order.
    pub fn pending_items(&self) -> &[FrontierItem] {
        &self.pending
    }

    /// Replace queue contents from a checkpoint frontier list.
    pub fn restore(&mut self, pending: Vec<FrontierItem>) {
        self.seen.clear();
        self.pending.clear();
        for item in pending {
            self.seen.insert(item.request_url.to_string());
            self.pending.push(item);
        }
    }
}

impl FrontierSink for FrontierQueue {
    fn push(&mut self, item: FrontierItem) -> bool {
        FrontierQueue::push(self, item)
    }
}

/// Convert navigation link candidates into frontier items.
pub fn frontier_from_links<S: FrontierSink + ?Sized>(
    queue: &mut S,
    source: &FrontierItem,
    links: &[LinkCandidate],
    max_depth: u32,
) -> Vec<FrontierItem> {
    let mut admitted = Vec::new();
    for link in links {
        if link.relation != LinkRelation::Navigation {
            continue;
        }
        let Ok(url) = crawler_types::canonicalize_request_url(&link.target)
        else {
            continue;
        };
        if source.depth + 1 > max_depth {
            continue;
        }
        let item = FrontierItem {
            request_url: url.clone(),
            discovery: crawler_types::DiscoverySource::NavigationLink { from: source.request_url.clone() },
            depth: source.depth + 1,
            site_key: crawler_types::site_key(&url),
            priority: source.priority + 1,
            attempts: 0,
            discovered_at_epoch: crawler_types::now_epoch(),
        };
        if queue.push(item.clone()) {
            admitted.push(item);
        }
    }
    admitted
}
