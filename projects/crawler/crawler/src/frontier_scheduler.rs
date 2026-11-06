//! Host-partitioned frontier scheduling over the store claim/lease contract.
//!
//! The scheduler only decides which host may run next and how long to wait;
//! exclusivity, fencing and expiry recovery stay inside the store. This keeps
//! rate state in memory while the durable truth stays in native records.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crawler_store::{ClaimToken, ReleaseOutcome, Store, keyspace::item_id_from_url};
use crawler_types::{FrontierItem, PolitenessProfile};

/// How long a host sits out after the store reports no claimable item.
///
/// A host can still hold a live lease, so an empty answer is retried instead of
/// being treated as terminal.
pub const EMPTY_HOST_RETRY: Duration = Duration::from_secs(1);

/// Per-host scheduling limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostPolicy {
    /// Claims allowed to be in flight at the same time.
    pub max_concurrent: u32,
    /// Minimum spacing between claims to the same host.
    pub crawl_delay: Duration,
}

impl HostPolicy {
    /// Build one policy.
    pub fn new(max_concurrent: u32, crawl_delay: Duration) -> Self {
        Self { max_concurrent: max_concurrent.max(1), crawl_delay }
    }

    /// Defaults for a crawl politeness preset.
    pub fn for_profile(profile: PolitenessProfile) -> Self {
        match profile {
            PolitenessProfile::Conservative => Self::new(1, Duration::from_secs(1)),
            PolitenessProfile::Balanced => Self::new(2, Duration::from_millis(250)),
            PolitenessProfile::Developer => Self::new(4, Duration::ZERO),
        }
    }

    /// Exponential backoff for a retry after `attempts` failed attempts.
    pub fn backoff(&self, attempts: u32) -> Duration {
        let factor = 1u32 << attempts.min(6);
        self.crawl_delay.saturating_mul(factor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct HostState {
    in_flight: u32,
    next_available_ms: u64,
}

/// In-memory host scheduler driven by per-host policies.
#[derive(Debug, Clone)]
pub struct HostScheduler {
    default: HostPolicy,
    overrides: BTreeMap<String, HostPolicy>,
    state: BTreeMap<String, HostState>,
    empty_retry: Duration,
}

impl HostScheduler {
    /// Build a scheduler whose hosts share the profile defaults.
    pub fn new(profile: PolitenessProfile) -> Self {
        Self {
            default: HostPolicy::for_profile(profile),
            overrides: BTreeMap::new(),
            state: BTreeMap::new(),
            empty_retry: EMPTY_HOST_RETRY,
        }
    }

    /// Override the policy for one host partition.
    pub fn with_host_policy(mut self, host: impl Into<String>, policy: HostPolicy) -> Self {
        self.overrides.insert(host.into(), policy);
        self
    }

    /// Override the retry interval used when a host has no claimable item.
    pub fn with_empty_retry(mut self, retry: Duration) -> Self {
        self.empty_retry = retry;
        self
    }

    /// Policy applied to one host.
    pub fn policy(&self, host: &str) -> HostPolicy {
        self.overrides.get(host).copied().unwrap_or(self.default)
    }

    /// Register a host that has pending work.
    pub fn note_queued(&mut self, host: &str, now_ms: u64) {
        self.state.entry(host.to_owned()).or_insert(HostState { in_flight: 0, next_available_ms: now_ms });
    }

    /// Pick the next host allowed to claim at `now_ms`.
    pub fn select(&self, now_ms: u64) -> Option<String> {
        self.state
            .iter()
            .filter(|(host, state)| {
                let policy = self.policy(host);
                state.in_flight < policy.max_concurrent && state.next_available_ms <= now_ms
            })
            .min_by_key(|(host, state)| (state.next_available_ms, (*host).clone()))
            .map(|(host, _)| host.clone())
    }

    /// Record a successful claim.
    pub fn on_claim(&mut self, host: &str, now_ms: u64) {
        let delay = self.policy(host).crawl_delay.as_millis() as u64;
        let state = self.state.entry(host.to_owned()).or_default();
        state.in_flight = state.in_flight.saturating_add(1);
        state.next_available_ms = now_ms.saturating_add(delay);
    }

    /// Record a released claim so the host can run again after its delay.
    pub fn on_release(&mut self, host: &str, now_ms: u64) {
        let delay = self.policy(host).crawl_delay.as_millis() as u64;
        let state = self.state.entry(host.to_owned()).or_default();
        state.in_flight = state.in_flight.saturating_sub(1);
        state.next_available_ms = state.next_available_ms.max(now_ms.saturating_add(delay));
    }

    /// Record a failed attempt and schedule a backoff before the retry.
    pub fn on_failure(&mut self, host: &str, now_ms: u64, attempts: u32) {
        let backoff = self.policy(host).backoff(attempts).as_millis() as u64;
        let state = self.state.entry(host.to_owned()).or_default();
        state.in_flight = state.in_flight.saturating_sub(1);
        state.next_available_ms = now_ms.saturating_add(backoff);
    }

    /// Record that the store had no claimable item for this host.
    ///
    /// The host is retried after [`Self::empty_retry`] because the pending item
    /// may be held by an live lease that expires later.
    pub fn on_empty(&mut self, host: &str, now_ms: u64) {
        let retry = self.empty_retry.as_millis() as u64;
        let state = self.state.entry(host.to_owned()).or_default();
        state.next_available_ms = now_ms.saturating_add(retry);
    }

    /// Hosts currently tracked by the scheduler.
    pub fn hosts(&self) -> impl Iterator<Item = &String> {
        self.state.keys()
    }
}

/// Frontier facade that claims through the store and schedules by host.
///
/// The store is borrowed per call so the same crawl loop can still use it for
/// robots, response cache and page transactions between frontier operations.
pub struct StoreFrontier {
    worker_id: String,
    scheduler: HostScheduler,
    host_by_item: BTreeMap<String, String>,
    seen: BTreeSet<String>,
}

impl StoreFrontier {
    /// Build a frontier for one worker.
    pub fn new(worker_id: impl Into<String>, profile: PolitenessProfile) -> Self {
        Self {
            worker_id: worker_id.into(),
            scheduler: HostScheduler::new(profile),
            host_by_item: BTreeMap::new(),
            seen: BTreeSet::new(),
        }
    }

    /// Replace the host scheduler, mainly for per-host overrides and tests.
    pub fn with_scheduler(mut self, scheduler: HostScheduler) -> Self {
        self.scheduler = scheduler;
        self
    }

    /// Access the scheduler state.
    pub fn scheduler(&self) -> &HostScheduler {
        &self.scheduler
    }

    /// Whether this worker already admitted an item id.
    pub fn contains(&self, item_id: &str) -> bool {
        self.seen.contains(item_id)
    }

    /// Enqueue an item into the store run and register its host partition.
    ///
    /// Duplicate URLs are dropped locally so a link re-discovered on several
    /// pages does not produce extra claims.
    pub fn enqueue(&mut self, store: &mut dyn Store, item: FrontierItem) -> crawler_store::Result<bool> {
        let item_id = item_id_from_url(item.request_url.as_str());
        if !self.seen.insert(item_id.clone()) {
            return Ok(false);
        }
        self.host_by_item.insert(item_id, item.site_key.clone());
        self.scheduler.note_queued(&item.site_key, now_millis());
        store.enqueue(item)?;
        Ok(true)
    }

    /// Register links that were admitted atomically by a page commit.
    ///
    /// The store already holds the frontier records, so only the in-process
    /// schedule state is updated.
    pub fn note_admitted(&mut self, items: &[FrontierItem]) {
        for item in items {
            let item_id = item_id_from_url(item.request_url.as_str());
            if !self.seen.insert(item_id.clone()) {
                continue;
            }
            self.host_by_item.insert(item_id, item.site_key.clone());
            self.scheduler.note_queued(&item.site_key, now_millis());
        }
    }

    /// Register hosts that already have queued frontier records in the store.
    ///
    /// A resumed run rebuilds the scheduler from persisted records instead of
    /// re-enqueuing items.
    pub fn adopt_hosts(&mut self, hosts: &[String]) {
        let now = now_millis();
        for host in hosts {
            self.scheduler.note_queued(host, now);
        }
    }

    /// Claim the next item allowed by the host schedule.
    pub fn next(&mut self, store: &mut dyn Store) -> crawler_store::Result<Option<ClaimToken>> {
        let now = now_millis();
        while let Some(host) = self.scheduler.select(now) {
            match store.claim_next(&host, &self.worker_id)? {
                Some(token) => {
                    self.scheduler.on_claim(&host, now);
                    self.host_by_item.insert(token.item_id.clone(), host);
                    return Ok(Some(token));
                }
                None => self.scheduler.on_empty(&host, now),
            }
        }
        Ok(None)
    }

    /// Renew one active claim.
    pub fn renew(&mut self, store: &mut dyn Store, token: &ClaimToken) -> crawler_store::Result<u64> {
        store.renew_claim(token)
    }

    /// Finish an item and drop it from the frontier.
    pub fn release_done(&mut self, store: &mut dyn Store, token: &ClaimToken) -> crawler_store::Result<()> {
        store.release_claim(token, ReleaseOutcome::Done)?;
        if let Some(host) = self.host_by_item.get(&token.item_id) {
            self.scheduler.on_release(host, now_millis());
        }
        Ok(())
    }

    /// Return an unprocessed item to pending without advancing its attempts.
    pub fn release_pending(&mut self, store: &mut dyn Store, token: &ClaimToken) -> crawler_store::Result<()> {
        store.release_claim(token, ReleaseOutcome::Pending)?;
        if let Some(host) = self.host_by_item.get(&token.item_id) {
            self.scheduler.on_release(host, now_millis());
        }
        Ok(())
    }

    /// Requeue a failed item with its attempt count advanced and a host backoff.
    ///
    /// `attempts` is the count the item had on this attempt, before the store
    /// advances it.
    pub fn release_failed(&mut self, store: &mut dyn Store, token: &ClaimToken, attempts: u32) -> crawler_store::Result<()> {
        store.release_claim(token, ReleaseOutcome::Requeue)?;
        if let Some(host) = self.host_by_item.get(&token.item_id) {
            self.scheduler.on_failure(host, now_millis(), attempts);
        }
        Ok(())
    }
}

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
