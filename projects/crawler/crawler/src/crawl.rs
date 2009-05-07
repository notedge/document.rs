use std::collections::{BTreeMap, HashSet};

use crate::browser::{BrowserProvider, snapshot_to_fetch_artifact};
use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_fetch::{FetchClient, FetchRequest, RobotsPolicy, Transport, site_key_for_url};
use crawler_types::{
    AdmissionDecision, BrowserFallbackPolicy, ChallengeOutcome, ChallengePolicy, CrawlBudgetStatus, CrawlCheckpoint,
    CrawlError, CrawlEvent, CrawlReport, CrawlRequest, ExtractBudget, ExtractContext, ExtractStatus, FetchArtifact,
    FrontierItem, PageChallengeState, Result, apply_challenge_policy, diagnostics as pd,
};

use crate::{
    admission::{admit_frontier_item, seed_frontier},
    extract::{Extractor, run_extract},
    frontier::{FrontierQueue, frontier_from_links},
    initial_report, sitemap,
    transaction::commit_page,
};
use crawler_types::PageTransaction;

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
    let mut state = CrawlState {
        request: request.clone(),
        queue: FrontierQueue::new(),
        requests_used: 0,
        report: initial_report(&request)?,
        last_paused_item: None,
    };

    for item in seed_frontier(&request)? {
        state.report.events.push(CrawlEvent::Discovered { url: item.request_url.clone(), depth: item.depth });
        state.queue.push(item);
    }

    run_crawl_loop(state, transport, extractors, options)
}

/// Resume a crawl from a saved checkpoint.
pub fn resume_crawl_from_checkpoint<T: Transport>(
    checkpoint: CrawlCheckpoint,
    transport: T,
    extractors: &[&dyn Extractor],
    options: CrawlOptions<'_>,
) -> Result<CrawlOutput> {
    validate_resume_strategy(&checkpoint, options)?;
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
    let resume_options = CrawlOptions { attempt_identity: checkpoint.attempt_identity.saturating_add(1), ..options };
    run_crawl_loop(state, transport, extractors, resume_options)
}

fn run_crawl_loop<T: Transport>(
    mut state: CrawlState,
    transport: T,
    extractors: &[&dyn Extractor],
    options: CrawlOptions<'_>,
) -> Result<CrawlOutput> {
    let request = state.request.clone();
    state.report.budget_status = CrawlBudgetStatus::Completed;
    state.report.remaining_frontier = 0;
    let mut client = FetchClient::new(transport, RobotsPolicy::from_profile(request.politeness));
    if let Some(cache_dir) = options.cache_dir {
        client = client.with_disk_cache_dir(cache_dir)?;
    }
    let mut committed_pages = Vec::new();
    let mut artifacts = Vec::new();
    let extract_budget = ExtractBudget::default();
    let mut sitemap_discovered = HashSet::new();

    while let Some(item) = state.queue.pop() {
        if state.requests_used >= request.budget.max_requests {
            state.queue.requeue(item);
            record_budget_exhausted(&mut state);
            break;
        }

        let decision = admit_frontier_item(&item, &request);
        state.report.events.push(CrawlEvent::Admitted { url: item.request_url.clone(), decision: decision.clone() });
        if !matches!(decision, AdmissionDecision::Admitted) {
            state.report.skipped += 1;
            continue;
        }

        state.requests_used += 1;
        let fetch_request = FetchRequest { url: item.request_url.clone(), headers: BTreeMap::new() };
        let artifact = match client.fetch(&fetch_request) {
            Ok(artifact) => {
                // Classify HTTP status before feeding into extract pipeline.
                let status_class = crawler_fetch::classify_http_status(artifact.status);
                match status_class {
                    crawler_fetch::HttpStatusClass::PermanentFailure => {
                        record_skipped(&mut state, &item, &format!("permanent-http-failure:{}", artifact.status));
                        continue;
                    }
                    crawler_fetch::HttpStatusClass::ClientError => {
                        // 401/403 — don't extract, don't retry.
                        record_skipped(&mut state, &item, &format!("client-http-error:{}", artifact.status));
                        continue;
                    }
                    crawler_fetch::HttpStatusClass::Proceed | crawler_fetch::HttpStatusClass::Retryable => artifact,
                }
            }
            Err(error) => {
                record_crawl_diagnostic(&mut state, &error);
                let fetch_error = error.to_string();
                match try_browser_fallback(&item, options) {
                    Some((snapshot, browser_engine)) => {
                        state.report.events.push(CrawlEvent::BrowserFallback {
                            url: item.request_url.clone(),
                            fetch_error,
                            browser_engine,
                        });
                        match resolve_browser_snapshot(snapshot, options.challenge_policy) {
                            BrowserPageAction::Proceed(artifact) => artifact,
                            BrowserPageAction::Skip(reason) => {
                                record_skipped(&mut state, &item, &reason);
                                continue;
                            }
                            BrowserPageAction::Pause(challenge_state) => {
                                record_paused(&mut state, &item, challenge_state);
                                continue;
                            }
                            BrowserPageAction::Fail(reason) => {
                                state.report.failed += 1;
                                state.report.events.push(CrawlEvent::Failed { url: item.request_url.clone(), reason });
                                continue;
                            }
                        }
                    }
                    None => {
                        state.report.failed += 1;
                        state.report.events.push(CrawlEvent::Failed { url: item.request_url.clone(), reason: fetch_error });
                        continue;
                    }
                }
            }
        };

        let host_key = site_key_for_url(&item.request_url);
        if sitemap_discovered.insert(host_key) {
            if !client.robots().sitemaps_for_url(&item.request_url).is_empty() {
                sitemap::discover(&mut state, &item, &mut client, &mut sitemap_discovered);
            }
        }

        process_fetched_item(
            &mut state,
            &item,
            artifact,
            extractors,
            extract_budget,
            &mut committed_pages,
            &mut artifacts,
            request.budget.max_depth,
        );
    }

    if state.last_paused_item.is_some() {
        state.report.budget_status = CrawlBudgetStatus::Cancelled;
        state.report.remaining_frontier = state.queue.len() as u32;
    }
    let checkpoint = if state.last_paused_item.is_some() { Some(build_checkpoint(&state, options)) } else { None };

    Ok(CrawlOutput { report: state.report, committed_pages, artifacts, checkpoint })
}

fn try_browser_fallback(item: &FrontierItem, options: CrawlOptions<'_>) -> Option<(crawler_types::BrowserSnapshot, String)> {
    if options.browser_fallback != BrowserFallbackPolicy::OnFetchFailure {
        return None;
    }
    let browser = options.browser?;
    let snapshot = browser.capture_snapshot(&item.request_url).ok()?;
    let engine = snapshot.browser_engine.clone();
    Some((snapshot, engine))
}

enum BrowserPageAction {
    Proceed(FetchArtifact),
    Skip(String),
    Pause(PageChallengeState),
    Fail(String),
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

fn process_fetched_item(
    state: &mut CrawlState,
    item: &FrontierItem,
    artifact: FetchArtifact,
    extractors: &[&dyn Extractor],
    extract_budget: ExtractBudget,
    committed_pages: &mut Vec<String>,
    artifacts: &mut Vec<FetchArtifact>,
    max_depth: u32,
) {
    let cache_hit = artifact.transport_provenance == crawler_types::TransportProvenance::CacheHit;
    state.report.events.push(CrawlEvent::Fetched { url: artifact.final_url.clone(), status: artifact.status, cache_hit });
    artifacts.push(artifact.clone());

    let extract = run_extract(
        &artifact,
        extractors,
        extract_budget,
        &ExtractContext { page_identity: Some(item.request_url.to_string()) },
    );
    state.report.events.push(CrawlEvent::Extracted {
        url: extract.source_url.clone(),
        status: extract.status,
        extractor: extract.extractor.clone(),
    });

    if matches!(extract.status, ExtractStatus::Failed | ExtractStatus::Unsupported) {
        state.report.failed += 1;
        return;
    }

    state.report.extracted += 1;
    let discovered = extract.discovered_links.clone();
    let page_identity = item.request_url.to_string();
    let transaction = PageTransaction { page_identity: page_identity.clone(), extract, discovered_links: discovered.clone() };
    match commit_page(transaction) {
        Ok(committed_identity) => {
            state.report.committed += 1;
            state.report.events.push(CrawlEvent::Committed { page_identity: committed_identity.clone() });
            committed_pages.push(committed_identity);
            if state.last_paused_item.as_ref().is_some_and(|paused| paused.request_url == item.request_url) {
                state.last_paused_item = None;
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

    for next in frontier_from_links(&mut state.queue, item, &discovered, max_depth) {
        state.report.events.push(CrawlEvent::Discovered { url: next.request_url.clone(), depth: next.depth });
    }
}

fn record_skipped(state: &mut CrawlState, item: &FrontierItem, reason: &str) {
    state.report.skipped += 1;
    state.report.events.push(CrawlEvent::Skipped { url: item.request_url.clone(), reason: reason.into() });
}

/// Mark the run budget-exhausted and keep the pending frontier visible.
fn record_budget_exhausted(state: &mut CrawlState) {
    let remaining = state.queue.len() as u32;
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
