//! Crawler diagnostic constants and origin constructors.
//!
//! This module freezes the Crawler product namespace on top of
//! [`boost_diagnostic`]'s neutral diagnostic model. Every code emitted by
//! Crawler production code is a dotted identifier drawn from the stable
//! tables below; changing a code string is a breaking wire change.
//!
//! Wire contract version: `diagnostic/v1` (see [`boost_diagnostic::wire::SCHEMA_VERSION`]).

use boost_diagnostic::{DiagnosticCode, DiagnosticOrigin};

/// Crawler namespace — the product origin for every diagnostic this crate emits.
pub const NAMESPACE: &str = "crawler";

// ─── report schema versions (pre-1.0 — bump only on breaking wire changes) ──

/// Crawl / extract report schema (`crawler.report/v0`).
pub const SCHEMA_REPORT: &str = "crawler.report/v0";
/// Checkpoint resume schema (`crawler.checkpoint/v0`).
pub const SCHEMA_CHECKPOINT: &str = "crawler.checkpoint/v0";
/// Inspect / probe schema (`crawler.inspect/v0`).
pub const SCHEMA_INSPECT: &str = "crawler.inspect/v0";

// ─── component origins ──────────────────────────────────────────────────────

/// Fetch transport and caching component origin.
pub fn origin_fetch() -> DiagnosticOrigin {
    DiagnosticOrigin::new(NAMESPACE, "fetch")
}

/// HTTP status classification sub-component.
pub fn origin_fetch_http() -> DiagnosticOrigin {
    DiagnosticOrigin::new(NAMESPACE, "fetch").with_stage("http")
}

/// Frontier admission/discovery component origin.
pub fn origin_frontier() -> DiagnosticOrigin {
    DiagnosticOrigin::new(NAMESPACE, "frontier")
}

/// Orchestration crawl loop component origin.
pub fn origin_crawl() -> DiagnosticOrigin {
    DiagnosticOrigin::new(NAMESPACE, "crawl")
}

/// Content extraction component origin.
pub fn origin_extract() -> DiagnosticOrigin {
    DiagnosticOrigin::new(NAMESPACE, "extract")
}

/// Browser integration component origin.
pub fn origin_browser() -> DiagnosticOrigin {
    DiagnosticOrigin::new(NAMESPACE, "browser")
}

/// Robots policy component origin.
pub fn origin_robots() -> DiagnosticOrigin {
    DiagnosticOrigin::new(NAMESPACE, "robots")
}

/// Politeness / rate limiting component origin.
pub fn origin_politeness() -> DiagnosticOrigin {
    DiagnosticOrigin::new(NAMESPACE, "politeness")
}

/// Persistent store / checkpoint component origin.
pub fn origin_store() -> DiagnosticOrigin {
    DiagnosticOrigin::new(NAMESPACE, "store")
}

// ─── stable diagnostic codes ────────────────────────────────────────────────
//
// Keep codes sorted by domain and severity. Each code MUST be:
//   - dotted, e.g. "crawler.fetch.http-status"
//   - stable across refactors
//   - never re-used after removal

// ── request / input ──

/// URL failed validation before fetch dispatch.
pub const REQUEST_INVALID: &str = "crawler.request.invalid";
/// URL canonicalization rejected the input.
pub const URL_INVALID: &str = "crawler.url.invalid";
/// URL rejected by policy (max length, out-of-scope scheme, private IP, ...).
pub const URL_OUT_OF_POLICY: &str = "crawler.url.out-of-policy";

// ── fetch / transport ──

/// Transport-level failure (DNS, socket, TLS handshake, ...).
pub const FETCH_TRANSPORT_FAILED: &str = "crawler.fetch.transport-failed";
/// Request exceeded configured timeout.
pub const FETCH_TIMEOUT: &str = "crawler.fetch.timeout";
/// Redirect chain exceeded configured limit.
pub const FETCH_REDIRECT_LIMIT: &str = "crawler.fetch.redirect-limit";
/// Non-success HTTP status with structured status code.
pub const FETCH_HTTP_STATUS: &str = "crawler.fetch.http-status";
/// TLS handshake failure.
pub const FETCH_TLS_FAILED: &str = "crawler.fetch.tls-failed";
/// Response body exceeded configured budget.
pub const FETCH_BODY_BUDGET: &str = "crawler.fetch.body-budget";
/// Response body (gzip / brotli / raw) failed to decode.
pub const FETCH_DECODE_FAILED: &str = "crawler.fetch.decode-failed";

// ── robots ──

/// `Robots.txt` denied the target URL.
pub const ROBOTS_DENIED: &str = "crawler.robots.denied";
/// `Robots.txt` could not be fetched (DNS/4xx/5xx — policy decision pending).
pub const ROBOTS_UNAVAILABLE: &str = "crawler.robots.unavailable";
/// `Robots.txt` body failed to parse.
pub const ROBOTS_PARSE_FAILED: &str = "crawler.robots.parse-failed";

// ── politeness ──

/// Host-level politeness enforced a rate-limit window.
pub const POLITENESS_RATE_LIMITED: &str = "crawler.politeness.rate-limited";
/// Host declared a `Crawl-Delay` in robots policy.
pub const POLITENESS_CRAWL_DELAY: &str = "crawler.politeness.crawl-delay";

// ── frontier ──

/// Frontier popped empty — crawl is terminating.
pub const FRONTIER_EMPTY: &str = "crawler.frontier.empty";
/// Claim lease expired before fetch could complete.
pub const FRONTIER_CLAIM_EXPIRED: &str = "crawler.frontier.claim-expired";
/// Orchestration cancelled the run.
pub const RUN_CANCELLED: &str = "crawler.run.cancelled";
/// Orchestration budget exhausted (max pages, max time, ...).
pub const RUN_BUDGET_EXCEEDED: &str = "crawler.run.budget-exceeded";
/// Crawl request budget exhausted with frontier items still pending.
pub const CRAWL_BUDGET_EXHAUSTED: &str = "crawler.crawl.budget-exhausted";
/// Run finished with partial coverage (non-fatal).
pub const RUN_PARTIAL: &str = "crawler.run.partial";

// ── extract ──

/// Media type not supported by the extract pipeline.
pub const EXTRACT_UNSUPPORTED_MEDIA: &str = "crawler.extract.unsupported-media";
/// Extract input failed to parse into an IR document.
pub const EXTRACT_PARSE_FAILED: &str = "crawler.extract.parse-failed";
/// Extract produced a valid IR with known coverage gaps.
pub const EXTRACT_SEMANTIC_PARTIAL: &str = "crawler.extract.semantic-partial";
/// Asset referenced by the page could not be resolved.
pub const EXTRACT_ASSET_UNRESOLVED: &str = "crawler.extract.asset-unresolved";

// ── browser ──

/// No browser backend available for this run.
pub const BROWSER_UNAVAILABLE: &str = "crawler.browser.unavailable";
/// Page presented a challenge (cloudflare, consent wall, ...).
pub const BROWSER_CHALLENGE: &str = "crawler.browser.challenge";
/// Browser session expired before snapshot capture.
pub const BROWSER_SESSION_EXPIRED: &str = "crawler.browser.session-expired";
/// Snapshot could not be deserialized or materialized.
pub const BROWSER_SNAPSHOT_INVALID: &str = "crawler.browser.snapshot-invalid";

// ── cache ──

/// Disk cache read failed (corrupt meta, bad CRC, missing file).
pub const CACHE_READ_FAILED: &str = "crawler.cache.read-failed";
/// Disk cache write failed (quota, permission, path).
pub const CACHE_WRITE_FAILED: &str = "crawler.cache.write-failed";
/// Revalidation request failed for an otherwise-usable cached entry.
pub const CACHE_REVALIDATION_FAILED: &str = "crawler.cache.revalidation-failed";

// ── store ──

/// Persistent store is unreachable or could not be opened.
pub const STORE_UNAVAILABLE: &str = "crawler.store.unavailable";
/// Write conflict — another worker claimed the same frontier item.
pub const STORE_CONFLICT: &str = "crawler.store.conflict";
/// Store data is corrupt (migration mismatch, CRC).
pub const STORE_CORRUPT: &str = "crawler.store.corrupt";
/// Transaction commit failed mid-write.
pub const STORE_TRANSACTION_FAILED: &str = "crawler.store.transaction-failed";
/// Recovery from checkpoint failed to resume a usable state.
pub const STORE_RECOVERY_FAILED: &str = "crawler.store.recovery-failed";

/// Build a [`DiagnosticCode`] from a stable code string.
pub fn code(code: &'static str) -> DiagnosticCode {
    DiagnosticCode::new(code)
}
