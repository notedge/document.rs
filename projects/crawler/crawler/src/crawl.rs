use std::collections::{BTreeMap, HashSet};

use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_browser::{BrowserProvider, snapshot_to_fetch_artifact};
use crawler_fetch::{FetchClient, FetchRequest, RobotsPolicy, Transport, site_key_for_url};
use crawler_frontier::{FrontierQueue, FrontierSink, frontier_from_links};
use crawler_store::{ClaimToken, PageAssetBinding, RunSpec, Store, keyspace::item_id_from_url};
use crawler_types::{
    AdmissionDecision, BrowserFallbackPolicy, BrowserFallbackTrigger, ChallengeOutcome, ChallengePolicy, CrawlBudgetStatus,
    CrawlCheckpoint, CrawlError, CrawlEvent, CrawlReport, CrawlRequest, ExtractBudget, ExtractContext, ExtractResult,
    ExtractStatus, FetchArtifact, FrontierItem, PageChallengeState, Result, apply_challenge_policy, diagnostics as pd,
};
use document_ir::{Block, DocumentGraph, Inline};

use crate::{
    admission::{admit_frontier_item, seed_frontier},
    extract::{Extractor, run_extract},
    frontier_scheduler::StoreFrontier,
    initial_report, sitemap,
    transaction::commit_page,
};
use crawler_types::PageTransaction;

/// Robots rules persisted through the store expire after this window.
pub const ROBOTS_STORE_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

/// Minimum plain-text characters an HTTP extraction must yield before the crawl
/// accepts the fetched page as real content.
///
/// Below this value the page is treated as a JavaScript shell or an empty
/// skeleton and, when browser fallback is enabled, the render path is tried.
pub const LOW_YIELD_MIN_TEXT_CHARS: u64 = 64;

/// Minimum number of content-bearing `notedown-ir` blocks an HTTP extraction
/// must yield before the crawl accepts the fetched page as real content.
pub const LOW_YIELD_MIN_CONTENT_BLOCKS: u32 = 1;

/// Inline `<script>` payload size from which a page may be considered
/// script-dominant rather than content-dominant.
pub const LOW_YIELD_SCRIPT_MIN_BYTES: u64 = 512;

/// Inline `<script>` payload must be at least this multiple of the extracted
/// text characters for the script-dominance signal to fire.
pub const LOW_YIELD_SCRIPT_TEXT_RATIO: u64 = 4;

/// Observed content yield of one extraction, used only to route a page toward
/// the render fallback. It never changes the `notedown-ir` produced by extract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PageYield {
    /// Total plain-text characters across content-bearing blocks.
    pub text_chars: u64,
    /// Number of content-bearing `notedown-ir` blocks.
    pub content_blocks: u32,
    /// Total bytes inside inline `<script>` elements.
    pub inline_script_bytes: u64,
}

impl PageYield {
    /// Measure the routing yield of an HTTP artifact and its extraction.
    pub fn measure(artifact: &FetchArtifact, extract: &ExtractResult) -> Self {
        let inline_script_bytes = artifact.body_bytes().map(inline_script_bytes).unwrap_or(0);
        let (text_chars, content_blocks) = extract.document.as_ref().map(measure_document).unwrap_or((0, 0));
        Self { text_chars, content_blocks, inline_script_bytes }
    }

    /// Whether this yield is too thin to accept as a real page.
    pub fn is_low(&self) -> bool {
        self.content_blocks < LOW_YIELD_MIN_CONTENT_BLOCKS
            || self.text_chars < LOW_YIELD_MIN_TEXT_CHARS
            || (self.inline_script_bytes >= LOW_YIELD_SCRIPT_MIN_BYTES
                && self.inline_script_bytes >= self.text_chars.saturating_mul(LOW_YIELD_SCRIPT_TEXT_RATIO))
    }
}

/// Optional browser and fallback settings for a crawl run.
#[derive(Clone, Copy)]
pub struct CrawlOptions<'a> {
    /// Browser provider used when fallback policy allows it.
    pub browser: Option<&'a dyn BrowserProvider>,
    /// When to invoke the browser provider.
    pub browser_fallback: BrowserFallbackPolicy,
    /// Challenge handling for browser snapshots.
    pub challenge_policy: ChallengePolicy,
    /// Optional persistent fetch cache directory.
    pub cache_dir: Option<&'a std::path::Path>,
    /// Strategy fingerprint recorded into checkpoints.
    pub strategy_fingerprint: Option<&'a str>,
    /// Attempt identity incremented on resume.
    pub attempt_identity: u32,
}

impl<'a> Default for CrawlOptions<'a> {
    fn default() -> Self {
        Self {
            browser: None,
            browser_fallback: BrowserFallbackPolicy::Never,
            challenge_policy: ChallengePolicy::Stop,
            cache_dir: None,
            strategy_fingerprint: None,
            attempt_identity: 0,
        }
    }
}

/// Mutable crawl session state for checkpointing.
#[derive(Debug, Clone)]
pub struct CrawlState {
    /// User request.
    pub request: CrawlRequest,
    /// Frontier queue.
    pub queue: FrontierQueue,
    /// Requests consumed so far.
    pub requests_used: u32,
    /// Accumulated report.
    pub report: CrawlReport,
    /// Last frontier item paused for operator action.
    pub last_paused_item: Option<FrontierItem>,
}

/// Output of a crawl run.
#[derive(Debug, Clone)]
pub struct CrawlOutput {
    /// Final report.
    pub report: CrawlReport,
    /// Committed page identities.
    pub committed_pages: Vec<String>,
    /// Artifacts available for offline re-extract.
    pub artifacts: Vec<FetchArtifact>,
    /// Checkpoint when the run paused for operator action.
    pub checkpoint: Option<CrawlCheckpoint>,
}

/// Run a bounded crawl using the provided transport.
pub fn run_crawl<T: Transport>(request: CrawlRequest, transport: T, extractors: &[&dyn Extractor]) -> Result<CrawlOutput> {
    run_crawl_with_options(request, transport, extractors, CrawlOptions::default())
}

/// Run a bounded crawl with optional browser fallback.
pub fn run_crawl_with_options<T: Transport>(
    request: CrawlRequest,
    transport: T,
    extractors: &[&dyn Extractor],
    options: CrawlOptions<'_>,
) -> Result<CrawlOutput> {
    let state = seeded_state(&request)?;
    run_crawl_loop(state, transport, extractors, options, None, None)
}

/// Run a bounded crawl whose frontier, page transactions and caches live in the
/// given store.
pub fn run_crawl_with_store<T: Transport>(
    request: CrawlRequest,
    transport: T,
    extractors: &[&dyn Extractor],
    options: CrawlOptions<'_>,
    store: &mut dyn Store,
) -> Result<CrawlOutput> {
    let mut state = seeded_state(&request)?;
    let mut frontier = begin_store_run(store, &request)?;
    enqueue_state_queue(store, &mut frontier, &mut state.queue)?;
    run_crawl_loop(state, transport, extractors, options, Some(store), Some(frontier))
}

fn seeded_state(request: &CrawlRequest) -> Result<CrawlState> {
    let mut state = CrawlState {
        request: request.clone(),
        queue: FrontierQueue::new(),
        requests_used: 0,
        report: initial_report(request)?,
        last_paused_item: None,
    };
    for item in seed_frontier(request)? {
        state.report.events.push(CrawlEvent::Discovered { url: item.request_url.clone(), depth: item.depth });
        state.queue.push(item);
    }
    Ok(state)
}

/// Resume a crawl from a saved checkpoint.
pub fn resume_crawl_from_checkpoint<T: Transport>(
    checkpoint: CrawlCheckpoint,
    transport: T,
    extractors: &[&dyn Extractor],
    options: CrawlOptions<'_>,
) -> Result<CrawlOutput> {
    resume_crawl_with_store(checkpoint, transport, extractors, options, None)
}

/// Resume a crawl from checkpoint while reusing the store-backed caches.
pub fn resume_crawl_from_checkpoint_with_store<T: Transport>(
    checkpoint: CrawlCheckpoint,
    transport: T,
    extractors: &[&dyn Extractor],
    options: CrawlOptions<'_>,
    store: &mut dyn Store,
) -> Result<CrawlOutput> {
    resume_crawl_with_store(checkpoint, transport, extractors, options, Some(store))
}

fn resume_crawl_with_store<T: Transport>(
    checkpoint: CrawlCheckpoint,
    transport: T,
    extractors: &[&dyn Extractor],
    options: CrawlOptions<'_>,
    store: Option<&mut dyn Store>,
) -> Result<CrawlOutput> {
    validate_resume_strategy(&checkpoint, options)?;
    let politeness = checkpoint.request.politeness;
    let mut state = CrawlState {
        request: checkpoint.request,
        queue: FrontierQueue::new(),
        requests_used: checkpoint.requests_used,
        report: checkpoint.report,
        last_paused_item: checkpoint.paused_item.clone(),
    };
    state.queue.restore(checkpoint.pending_frontier);
    if let Some(item) = checkpoint.paused_item {
        state.queue.requeue(item);
    }
    let mut store = store;
    let mut frontier = None;
    if let Some(store) = store.as_deref_mut() {
        let mut seeded = StoreFrontier::new(STORE_WORKER_ID, politeness);
        enqueue_state_queue(store, &mut seeded, &mut state.queue)?;
        frontier = Some(seeded);
    }
    let resume_options = CrawlOptions { attempt_identity: checkpoint.attempt_identity.saturating_add(1), ..options };
    run_crawl_loop(state, transport, extractors, resume_options, store, frontier)
}

/// Worker identity recorded on store claims for a local crawl run.
const STORE_WORKER_ID: &str = "crawl-local";

fn begin_store_run(store: &mut dyn Store, request: &CrawlRequest) -> Result<StoreFrontier> {
    let seed_url = request.seeds.first().map(|seed| seed.url.to_string()).unwrap_or_default();
    store.begin_run(RunSpec { seed_url }).map_err(store_error)?;
    Ok(StoreFrontier::new(STORE_WORKER_ID, request.politeness))
}

fn enqueue_state_queue(store: &mut dyn Store, frontier: &mut StoreFrontier, queue: &mut FrontierQueue) -> Result<()> {
    while let Some(item) = queue.pop() {
        frontier.enqueue(store, item).map_err(store_error)?;
    }
    Ok(())
}

fn store_error(error: impl std::fmt::Display) -> CrawlError {
    let diag = boost_diagnostic::Diagnostic::new(
        pd::code(pd::STORE_UNAVAILABLE),
        DiagnosticSeverity::Error,
        pd::origin_crawl(),
        Message::new("crawler.diagnostic.storeUnavailable").with_fallback(error.to_string()),
    );
    CrawlError::from(diag)
}

fn next_work(
    state: &mut CrawlState,
    store: &mut Option<&mut dyn Store>,
    frontier: &mut Option<StoreFrontier>,
) -> Result<Option<(FrontierItem, Option<ClaimToken>)>> {
    if let (Some(store), Some(frontier)) = (store.as_deref_mut(), frontier.as_mut()) {
        let Some(token) = frontier.next(store).map_err(store_error)?
        else {
            return Ok(None);
        };
        let item = store
            .frontier_item(&token.item_id)
            .map_err(store_error)?
            .ok_or_else(|| store_error(format!("claimed frontier item `{}` disappeared", token.item_id)))?;
        return Ok(Some((item, Some(token))));
    }
    Ok(state.queue.pop().map(|item| (item, None)))
}

fn finish_work(
    store: &mut Option<&mut dyn Store>,
    frontier: &mut Option<StoreFrontier>,
    token: Option<&ClaimToken>,
    paused: bool,
) {
    if let (Some(store), Some(frontier), Some(token)) = (store.as_deref_mut(), frontier.as_mut(), token) {
        let result = if paused { frontier.release_pending(store, token) } else { frontier.release_done(store, token) };
        let _ = result;
    }
}

fn run_crawl_loop<T: Transport>(
    mut state: CrawlState,
    transport: T,
    extractors: &[&dyn Extractor],
    options: CrawlOptions<'_>,
    mut store: Option<&mut dyn Store>,
    mut frontier: Option<StoreFrontier>,
) -> Result<CrawlOutput> {
    let request = state.request.clone();
    state.report.budget_status = CrawlBudgetStatus::Completed;
    state.report.remaining_frontier = 0;
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(request.politeness));
    if store.is_none() {
        if let Some(cache_dir) = options.cache_dir {
            client = client.with_disk_cache_dir(cache_dir)?;
        }
    }
    let mut committed_pages = Vec::new();
    let mut artifacts = Vec::new();
    let extract_budget = ExtractBudget::default();
    let mut sitemap_discovered = HashSet::new();

    loop {
        let Some((item, claim)) = next_work(&mut state, &mut store, &mut frontier)?
        else {
            break;
        };
        if state.requests_used >= request.budget.max_requests {
            if claim.is_none() {
                state.queue.requeue(item);
            }
            let remaining = remaining_frontier(&state, &store);
            record_budget_exhausted(&mut state, remaining);
            finish_work(&mut store, &mut frontier, claim.as_ref(), true);
            break;
        }

        let paused = process_one_item(
            &mut state,
            item,
            &mut client,
            extractors,
            extract_budget,
            &mut committed_pages,
            &mut artifacts,
            options,
            &mut sitemap_discovered,
            &mut store,
            &mut frontier,
            claim.as_ref(),
            &request,
        )?;
        finish_work(&mut store, &mut frontier, claim.as_ref(), paused);
        if paused {
            break;
        }
    }

    if state.last_paused_item.is_some() {
        state.report.budget_status = CrawlBudgetStatus::Cancelled;
        state.report.remaining_frontier = remaining_frontier(&state, &store);
    }
    let checkpoint = if state.last_paused_item.is_some() { Some(build_checkpoint(&state, options)) } else { None };

    Ok(CrawlOutput { report: state.report, committed_pages, artifacts, checkpoint })
}

fn remaining_frontier(state: &CrawlState, store: &Option<&mut dyn Store>) -> u32 {
    match store.as_deref() {
        Some(store) => store.pending_frontier_count().unwrap_or(0) as u32,
        None => state.queue.len() as u32,
    }
}

/// Process one claimed or queued item and report whether it paused the run.
#[allow(clippy::too_many_arguments)]
fn process_one_item<T: Transport>(
    state: &mut CrawlState,
    item: FrontierItem,
    client: &mut FetchClient<T>,
    extractors: &[&dyn Extractor],
    extract_budget: ExtractBudget,
    committed_pages: &mut Vec<String>,
    artifacts: &mut Vec<FetchArtifact>,
    options: CrawlOptions<'_>,
    sitemap_discovered: &mut HashSet<String>,
    store: &mut Option<&mut dyn Store>,
    frontier: &mut Option<StoreFrontier>,
    claim: Option<&ClaimToken>,
    request: &CrawlRequest,
) -> Result<bool> {
    let decision = admit_frontier_item(&item, request);
    state.report.events.push(CrawlEvent::Admitted { url: item.request_url.clone(), decision: decision.clone() });
    if !matches!(decision, AdmissionDecision::Admitted) {
        state.report.skipped += 1;
        return Ok(false);
    }

    state.requests_used += 1;
    let fetch_request = FetchRequest { url: item.request_url.clone(), headers: BTreeMap::new() };
    if let Some(store) = store.as_deref() {
        if matches!(item.request_url.scheme(), "http" | "https") {
            let site = site_key_for_url(&item.request_url);
            if !client.robots().has_site_rules(&item.request_url) {
                if let Ok(Some(body)) = store.get_robots_cache(&site) {
                    client.load_site_robots(&site, &String::from_utf8_lossy(&body));
                }
            }
        }
        if let Ok(Some(cached)) = store.get_response_cache(&item.request_url.to_string()) {
            client.cache_mut().put(cached);
        }
    }
    let fetched = client.fetch(&fetch_request);
    if let Some(store) = store.as_deref_mut() {
        for (site, body) in client.take_robots_updates() {
            let _ = store.put_robots_cache(&site, body.as_bytes(), ROBOTS_STORE_TTL);
        }
    }
    let artifact = match fetched {
        Ok(artifact) => {
            // Classify HTTP status before feeding into extract pipeline.
            let status_class = crawler_fetch::classify_http_status(artifact.status);
            match status_class {
                crawler_fetch::HttpStatusClass::PermanentFailure => {
                    record_skipped(state, &item, &format!("permanent-http-failure:{}", artifact.status));
                    return Ok(false);
                }
                crawler_fetch::HttpStatusClass::ClientError => {
                    // 401/403 — don't extract, don't retry.
                    record_skipped(state, &item, &format!("client-http-error:{}", artifact.status));
                    return Ok(false);
                }
                crawler_fetch::HttpStatusClass::Proceed | crawler_fetch::HttpStatusClass::Retryable => artifact,
            }
        }
        Err(error) => {
            record_crawl_diagnostic(state, &error);
            let fetch_error = error.to_string();
            match try_browser_fallback(&item, options) {
                BrowserFallbackAttempt::Captured { snapshot, browser_engine } => {
                    state.report.events.push(CrawlEvent::BrowserFallback {
                        url: item.request_url.clone(),
                        trigger: BrowserFallbackTrigger::FetchFailure,
                        fetch_error: Some(fetch_error),
                        browser_engine,
                        observed_text_chars: None,
                        observed_content_blocks: None,
                    });
                    if let Some(artifact) = accept_browser_snapshot(state, &item, snapshot, options) {
                        discover_sitemap(state, &item, client, sitemap_discovered, store, frontier)?;
                        process_fetched_item(
                            state,
                            &item,
                            artifact,
                            extractors,
                            extract_budget,
                            committed_pages,
                            artifacts,
                            request.budget.max_depth,
                            options,
                            false,
                            false,
                            store,
                            frontier,
                            claim,
                        );
                    }
                    return Ok(item_paused(state, &item));
                }
                BrowserFallbackAttempt::Failed(error) => {
                    record_browser_fallback_failure(state, &item, BrowserFallbackTrigger::FetchFailure, error);
                    return Ok(false);
                }
                BrowserFallbackAttempt::Disabled => {
                    state.report.failed += 1;
                    state.report.events.push(CrawlEvent::Failed { url: item.request_url.clone(), reason: fetch_error });
                    return Ok(false);
                }
            }
        }
    };

    if let Some(store) = store.as_deref_mut() {
        let _ = store.put_response_cache(&artifact);
    }

    discover_sitemap(state, &item, client, sitemap_discovered, store, frontier)?;
    process_fetched_item(
        state,
        &item,
        artifact,
        extractors,
        extract_budget,
        committed_pages,
        artifacts,
        request.budget.max_depth,
        options,
        true,
        true,
        store,
        frontier,
        claim,
    );
    Ok(item_paused(state, &item))
}

fn item_paused(state: &CrawlState, item: &FrontierItem) -> bool {
    state.last_paused_item.as_ref().is_some_and(|paused| paused.request_url == item.request_url)
}

fn discover_sitemap<T: Transport>(
    state: &mut CrawlState,
    item: &FrontierItem,
    client: &mut FetchClient<T>,
    sitemap_discovered: &mut HashSet<String>,
    store: &mut Option<&mut dyn Store>,
    frontier: &mut Option<StoreFrontier>,
) -> Result<()> {
    let host_key = site_key_for_url(&item.request_url);
    if !sitemap_discovered.insert(host_key) || client.robots().sitemaps_for_url(&item.request_url).is_empty() {
        return Ok(());
    }
    sitemap::discover(state, item, client, sitemap_discovered);
    if let (Some(store), Some(frontier)) = (store.as_deref_mut(), frontier.as_mut()) {
        enqueue_state_queue(store, frontier, &mut state.queue)?;
    }
    Ok(())
}

enum BrowserFallbackAttempt {
    Disabled,
    Captured { snapshot: crawler_types::BrowserSnapshot, browser_engine: String },
    Failed(CrawlError),
}

fn try_browser_fallback(item: &FrontierItem, options: CrawlOptions<'_>) -> BrowserFallbackAttempt {
    if options.browser_fallback != BrowserFallbackPolicy::OnFetchFailure {
        return BrowserFallbackAttempt::Disabled;
    }
    let Some(browser) = options.browser
    else {
        return BrowserFallbackAttempt::Disabled;
    };
    match browser.capture_snapshot(&item.request_url) {
        Ok(snapshot) => {
            let browser_engine = snapshot.browser_engine.clone();
            BrowserFallbackAttempt::Captured { snapshot, browser_engine }
        }
        Err(error) => BrowserFallbackAttempt::Failed(error),
    }
}

enum BrowserPageAction {
    Proceed(FetchArtifact),
    Skip(String),
    Pause(PageChallengeState),
    Fail(String),
}

/// Resolve a captured snapshot against the challenge policy and record the
/// resulting skip / pause / failure event when it cannot proceed.
fn accept_browser_snapshot(
    state: &mut CrawlState,
    item: &FrontierItem,
    snapshot: crawler_types::BrowserSnapshot,
    options: CrawlOptions<'_>,
) -> Option<FetchArtifact> {
    match resolve_browser_snapshot(snapshot, options.challenge_policy) {
        BrowserPageAction::Proceed(artifact) => Some(artifact),
        BrowserPageAction::Skip(reason) => {
            record_skipped(state, item, &reason);
            None
        }
        BrowserPageAction::Pause(challenge_state) => {
            record_paused(state, item, challenge_state);
            None
        }
        BrowserPageAction::Fail(reason) => {
            state.report.failed += 1;
            state.report.events.push(CrawlEvent::Failed { url: item.request_url.clone(), reason });
            None
        }
    }
}

/// Record a failed render fallback without swallowing the provider diagnostic.
fn record_browser_fallback_failure(
    state: &mut CrawlState,
    item: &FrontierItem,
    trigger: BrowserFallbackTrigger,
    error: CrawlError,
) {
    let diagnostic_code = error.code().unwrap_or(pd::BROWSER_FALLBACK_FAILED).to_string();
    let provider_code = error.text_arg("workerCode").filter(|code| !code.is_empty()).map(str::to_string);
    let reason = error.to_string();
    let CrawlError::Diagnostic(d) = &error;
    state.report.push_diagnostic(d.clone());
    state.report.failed += 1;
    state.report.events.push(CrawlEvent::BrowserFallbackFailed {
        url: item.request_url.clone(),
        trigger,
        diagnostic_code,
        provider_code,
        reason: reason.clone(),
    });
    state.report.events.push(CrawlEvent::Failed { url: item.request_url.clone(), reason });
}

fn resolve_browser_snapshot(snapshot: crawler_types::BrowserSnapshot, policy: ChallengePolicy) -> BrowserPageAction {
    let challenge_state = snapshot.challenge_state;
    let outcome = apply_challenge_policy(challenge_state, policy);
    match outcome {
        ChallengeOutcome::Proceed => BrowserPageAction::Proceed(snapshot_to_fetch_artifact(&snapshot)),
        ChallengeOutcome::Skip => BrowserPageAction::Skip(challenge_reason("skipped", challenge_state)),
        ChallengeOutcome::PauseForOperator => BrowserPageAction::Pause(challenge_state),
        ChallengeOutcome::Stop => BrowserPageAction::Fail(challenge_reason("blocked", challenge_state)),
        ChallengeOutcome::FallbackHttp => BrowserPageAction::Fail("challenge-fallback-http-unavailable".into()),
    }
}

fn challenge_reason(prefix: &str, state: PageChallengeState) -> String {
    format!("{prefix}:{}", serde_json::to_string(&state).unwrap_or_else(|_| "unknown".into()))
}

#[allow(clippy::too_many_arguments)]
fn process_fetched_item(
    state: &mut CrawlState,
    item: &FrontierItem,
    mut artifact: FetchArtifact,
    extractors: &[&dyn Extractor],
    extract_budget: ExtractBudget,
    committed_pages: &mut Vec<String>,
    artifacts: &mut Vec<FetchArtifact>,
    max_depth: u32,
    options: CrawlOptions<'_>,
    record_fetched: bool,
    allow_low_yield_fallback: bool,
    store: &mut Option<&mut dyn Store>,
    frontier: &mut Option<StoreFrontier>,
    claim: Option<&ClaimToken>,
) {
    let event_start = state.report.events.events().len();
    let cache_hit = artifact.transport_provenance == crawler_types::TransportProvenance::CacheHit;
    if record_fetched {
        state.report.events.push(CrawlEvent::Fetched { url: artifact.final_url.clone(), status: artifact.status, cache_hit });
    }
    artifacts.push(artifact.clone());

    let mut extract = extract_artifact(&artifact, item, extractors, extract_budget);
    state.report.events.push(CrawlEvent::Extracted {
        url: extract.source_url.clone(),
        status: extract.status,
        extractor: extract.extractor.clone(),
    });

    if matches!(extract.status, ExtractStatus::Failed | ExtractStatus::Unsupported) {
        state.report.failed += 1;
        return;
    }

    if allow_low_yield_fallback && is_html_artifact(&artifact) {
        let page_yield = PageYield::measure(&artifact, &extract);
        if page_yield.is_low() {
            match try_browser_fallback(item, options) {
                BrowserFallbackAttempt::Disabled => {}
                BrowserFallbackAttempt::Failed(error) => {
                    record_browser_fallback_failure(state, item, BrowserFallbackTrigger::LowYield, error);
                    return;
                }
                BrowserFallbackAttempt::Captured { snapshot, browser_engine } => {
                    state.report.events.push(CrawlEvent::BrowserFallback {
                        url: item.request_url.clone(),
                        trigger: BrowserFallbackTrigger::LowYield,
                        fetch_error: None,
                        browser_engine,
                        observed_text_chars: Some(page_yield.text_chars),
                        observed_content_blocks: Some(page_yield.content_blocks),
                    });
                    let Some(rendered) = accept_browser_snapshot(state, item, snapshot, options)
                    else {
                        return;
                    };
                    artifact = rendered;
                    if let Some(last) = artifacts.last_mut() {
                        *last = artifact.clone();
                    }
                    extract = extract_artifact(&artifact, item, extractors, extract_budget);
                    state.report.events.push(CrawlEvent::Extracted {
                        url: extract.source_url.clone(),
                        status: extract.status,
                        extractor: extract.extractor.clone(),
                    });
                    if matches!(extract.status, ExtractStatus::Failed | ExtractStatus::Unsupported) {
                        state.report.failed += 1;
                        return;
                    }
                }
            }
        }
    }

    state.report.extracted += 1;
    let discovered = extract.discovered_links.clone();
    let document = extract.document.clone();
    let page_identity = item.request_url.to_string();
    let transaction = PageTransaction { page_identity: page_identity.clone(), extract, discovered_links: discovered.clone() };
    match commit_page(transaction) {
        Ok(committed_identity) => {
            let committed_event = CrawlEvent::Committed { page_identity: committed_identity.clone() };
            let page_events: Vec<CrawlEvent> = state.report.events.events()[event_start..].to_vec();
            // Admitted links are staged in the page transaction when a store is
            // present, so the page record and its frontier links cannot diverge.
            let links = if store.is_some() {
                let mut sink = StagedLinks::new(frontier.as_ref());
                frontier_from_links(&mut sink, item, &discovered, max_depth)
            }
            else {
                frontier_from_links(&mut state.queue, item, &discovered, max_depth)
            };
            let discovered_events: Vec<CrawlEvent> =
                links.iter().map(|next| CrawlEvent::Discovered { url: next.request_url.clone(), depth: next.depth }).collect();
            if let Some(store) = store.as_deref_mut() {
                let worker_id = claim.map(|claim| claim.worker_id.as_str()).unwrap_or(STORE_WORKER_ID);
                let written = write_page_transaction(
                    store,
                    worker_id,
                    &page_identity,
                    &artifact,
                    document.as_ref(),
                    &page_events,
                    &committed_event,
                    &discovered_events,
                    &links,
                );
                if let Err(error) = written {
                    let _ = store.abort_page_tx();
                    let CrawlError::Diagnostic(d) = &error;
                    state.report.push_diagnostic(d.clone());
                    state.report.failed += 1;
                    state.report.events.push(CrawlEvent::Failed { url: item.request_url.clone(), reason: error.to_string() });
                    return;
                }
                if let Some(frontier) = frontier.as_mut() {
                    frontier.note_admitted(&links);
                }
            }
            state.report.committed += 1;
            state.report.events.push(committed_event);
            committed_pages.push(committed_identity);
            if state.last_paused_item.as_ref().is_some_and(|paused| paused.request_url == item.request_url) {
                state.last_paused_item = None;
            }
            for next in discovered_events {
                state.report.events.push(next);
            }
        }
        Err(error) => {
            let CrawlError::Diagnostic(d) = &error;
            state.report.push_diagnostic(d.clone());
            state.report.failed += 1;
            state.report.events.push(CrawlEvent::Failed { url: item.request_url.clone(), reason: error.to_string() });
            return;
        }
    }
}

/// Deduplicate discovered links against the store frontier before the page
/// commit stages them.
struct StagedLinks<'a> {
    frontier: Option<&'a StoreFrontier>,
    seen: HashSet<String>,
}

impl<'a> StagedLinks<'a> {
    fn new(frontier: Option<&'a StoreFrontier>) -> Self {
        Self { frontier, seen: HashSet::new() }
    }
}

impl FrontierSink for StagedLinks<'_> {
    fn push(&mut self, item: FrontierItem) -> bool {
        let key = item.request_url.to_string();
        if self.frontier.is_some_and(|frontier| frontier.contains(&item_id_from_url(&key))) {
            return false;
        }
        self.seen.insert(key)
    }
}

#[allow(clippy::too_many_arguments)]
fn write_page_transaction(
    store: &mut dyn Store,
    worker_id: &str,
    page_identity: &str,
    artifact: &FetchArtifact,
    document: Option<&DocumentGraph>,
    page_events: &[CrawlEvent],
    committed_event: &CrawlEvent,
    discovered_events: &[CrawlEvent],
    links: &[FrontierItem],
) -> Result<()> {
    store.begin_page_tx(worker_id).map_err(store_error)?;
    store.page_tx_put_fetch(page_identity, artifact.body_bytes().unwrap_or_default()).map_err(store_error)?;
    if let Some(document) = document {
        let bytes = serde_json::to_vec(document).map_err(|error| store_error(format!("IR serialization failed: {error}")))?;
        store.page_tx_put_ir(page_identity, &bytes).map_err(store_error)?;
        for asset in &document.assets {
            let Some(bytes) = asset.bytes.as_deref()
            else {
                continue;
            };
            if bytes.is_empty() {
                continue;
            }
            let fingerprint = asset.content_identity.clone().unwrap_or_else(|| crate::assets::fingerprint(bytes));
            let binding = PageAssetBinding {
                asset_id: asset.id.0.to_string(),
                fingerprint,
                media_type: asset.media_type.clone(),
                bytes: bytes.to_vec(),
            };
            store.page_tx_bind_asset(&binding).map_err(store_error)?;
        }
    }
    for event in page_events {
        store.page_tx_put_event(event).map_err(store_error)?;
    }
    store.page_tx_put_event(committed_event).map_err(store_error)?;
    for event in discovered_events {
        store.page_tx_put_event(event).map_err(store_error)?;
    }
    store.page_tx_admit_links(links.to_vec()).map_err(store_error)?;
    store.commit_page_tx().map_err(store_error)
}

fn extract_artifact(
    artifact: &FetchArtifact,
    item: &FrontierItem,
    extractors: &[&dyn Extractor],
    extract_budget: ExtractBudget,
) -> ExtractResult {
    run_extract(artifact, extractors, extract_budget, &ExtractContext { page_identity: Some(item.request_url.to_string()) })
}

/// Only HTML artifacts can be replaced by a rendered DOM. A short PDF, JSON or
/// plain-text response is not a JavaScript shell.
fn is_html_artifact(artifact: &FetchArtifact) -> bool {
    if let Some(content_type) = artifact.content_type.as_deref() {
        return content_type.to_ascii_lowercase().contains("html");
    }
    let Some(body) = artifact.body_bytes()
    else {
        return false;
    };
    let head = &body[..body.len().min(64)];
    let head = String::from_utf8_lossy(head);
    let head = head.trim_start().trim_start_matches('\u{feff}').to_ascii_lowercase();
    ["<!doctype html", "<html", "<body", "<div", "<p", "<article", "<main"].iter().any(|prefix| head.starts_with(prefix))
}

fn measure_document(graph: &DocumentGraph) -> (u64, u32) {
    let mut text_chars = 0u64;
    let mut content_blocks = 0u32;
    for node in &graph.blocks {
        if let Some(chars) = block_text_chars(&node.block) {
            content_blocks += 1;
            text_chars += chars;
        }
    }
    (text_chars, content_blocks)
}

fn block_text_chars(block: &Block) -> Option<u64> {
    let chars = match block {
        Block::Section { title, .. } => inline_text_chars(title),
        Block::Paragraph { content } => inline_text_chars(content),
        Block::List { items, .. } => items.iter().map(|item| inline_text_chars(&item.content)).sum(),
        Block::Table { caption, rows } => {
            caption.as_deref().map(|caption| caption.chars().count() as u64).unwrap_or(0)
                + rows.iter().flat_map(|row| &row.cells).map(|cell| inline_text_chars(&cell.content)).sum::<u64>()
        }
        Block::Quote { content, .. } => inline_text_chars(content),
        Block::Code { content, .. } => content.chars().count() as u64,
        Block::Math { formula } => formula.source.chars().count() as u64,
        Block::Diagram { source, .. } | Block::Chart { source, .. } => source.chars().count() as u64,
        Block::Opaque { .. } | Block::RawHtml { .. } => return None,
    };
    Some(chars)
}

fn inline_text_chars(inlines: &[Inline]) -> u64 {
    inlines.iter().map(|inline| inline.plain_text().chars().count() as u64).sum()
}

/// Sum the bytes inside inline `<script>` elements.
fn inline_script_bytes(body: &[u8]) -> u64 {
    const OPEN: &[u8] = b"<script";
    const CLOSE: &[u8] = b"</script";
    let mut total = 0u64;
    let mut cursor = 0usize;
    while let Some(start) = find_ascii_case_insensitive(body, OPEN, cursor) {
        let Some(open_end) = body[start..].iter().position(|byte| *byte == b'>')
        else {
            break;
        };
        let body_start = start + open_end + 1;
        let Some(end) = find_ascii_case_insensitive(body, CLOSE, body_start)
        else {
            break;
        };
        total += end.saturating_sub(body_start) as u64;
        cursor = end + CLOSE.len();
    }
    total
}

fn find_ascii_case_insensitive(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() || needle.len() > haystack.len() {
        return None;
    }
    let last = haystack.len() - needle.len();
    (from..=last).find(|start| {
        haystack
            .get(*start..)
            .and_then(|rest| rest.get(..needle.len()))
            .is_some_and(|window| window.eq_ignore_ascii_case(needle))
    })
}

fn record_skipped(state: &mut CrawlState, item: &FrontierItem, reason: &str) {
    state.report.skipped += 1;
    state.report.events.push(CrawlEvent::Skipped { url: item.request_url.clone(), reason: reason.into() });
}

/// Mark the run budget-exhausted and keep the pending frontier visible.
fn record_budget_exhausted(state: &mut CrawlState, remaining: u32) {
    state.report.budget_status = CrawlBudgetStatus::Exhausted;
    state.report.remaining_frontier = remaining;
    let diagnostic = boost_diagnostic::Diagnostic::new(
        pd::code(pd::CRAWL_BUDGET_EXHAUSTED),
        DiagnosticSeverity::Warning,
        pd::origin_crawl(),
        Message::new("crawler.diagnostic.crawlBudgetExhausted")
            .with_arg("remainingFrontier", MessageArg::U64(remaining as u64))
            .with_fallback(format!("crawl request budget exhausted with {remaining} frontier item(s) remaining")),
    );
    state.report.push_diagnostic(diagnostic);
}

/// Push a run-level diagnostic into the report envelope when a page fails
/// with a structured [`CrawlError::Diagnostic`].
fn record_crawl_diagnostic(state: &mut CrawlState, error: &CrawlError) {
    let CrawlError::Diagnostic(d) = error;
    state.report.push_diagnostic(d.clone());
}

fn record_paused(state: &mut CrawlState, item: &FrontierItem, challenge_state: PageChallengeState) {
    state.requests_used = state.requests_used.saturating_sub(1);
    state.report.paused += 1;
    state.last_paused_item = Some(item.clone());
    state.report.events.push(CrawlEvent::PausedForOperator { url: item.request_url.clone(), challenge_state });
}

fn build_checkpoint(state: &CrawlState, options: CrawlOptions<'_>) -> CrawlCheckpoint {
    let (paused_url, paused_challenge) = last_pause_from_report(&state.report);
    CrawlCheckpoint::new(
        state.request.clone(),
        state.queue.pending_items().to_vec(),
        state.requests_used,
        state.report.clone(),
        paused_url,
        paused_challenge,
        state.last_paused_item.clone(),
        options.strategy_fingerprint.map(str::to_string).or_else(|| Some(default_strategy_fingerprint(options))),
        options.attempt_identity,
    )
}

fn default_strategy_fingerprint(options: CrawlOptions<'_>) -> String {
    format!("browser_fallback={:?}", options.browser_fallback)
}

fn validate_resume_strategy(checkpoint: &CrawlCheckpoint, options: CrawlOptions<'_>) -> Result<()> {
    if let Some(expected) = &checkpoint.strategy_fingerprint {
        let actual = options.strategy_fingerprint.map(str::to_string).unwrap_or_else(|| default_strategy_fingerprint(options));
        if expected != &actual {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::REQUEST_INVALID),
                DiagnosticSeverity::Error,
                pd::origin_crawl(),
                Message::new("crawler.diagnostic.resumeStrategyMismatch")
                    .with_arg("checkpoint", MessageArg::Text(expected.clone()))
                    .with_arg("current", MessageArg::Text(actual.clone()))
                    .with_fallback(format!(
                        "resume strategy mismatch: checkpoint `{expected}` does not match current `{actual}`"
                    )),
            );
            return Err(CrawlError::from(diag));
        }
    }
    Ok(())
}

fn last_pause_from_report(report: &CrawlReport) -> (Option<url::Url>, Option<PageChallengeState>) {
    for event in report.events.events().iter().rev() {
        if let CrawlEvent::PausedForOperator { url, challenge_state } = event {
            return (Some(url.clone()), Some(*challenge_state));
        }
    }
    (None, None)
}
