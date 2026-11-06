#![warn(missing_docs)]
#![doc = include_str!("readme.md")]

/// Re-export of `crawler-types`.
pub mod types {
    //! Shared crawl contracts, reports, and frontier types.
    pub use crawler_types::*;
}

/// Re-export of `crawler-fetch`.
pub mod fetch {
    //! Transports, robots policy, and response caching.
    pub use crawler_fetch::*;
}

/// Re-export of `crawler-extract`.
pub mod semantic {
    //! Semantic extractors over fetch artifacts.
    pub use crawler_extract::*;
}

/// Re-export of `crawler-store`.
pub mod store {
    //! Store contracts, record codec, and persistence backends.
    pub use crawler_store::*;
}

mod admission;
mod assets;
mod crawl;
mod extract;
mod frontier_scheduler;
mod input;
mod inspect;
mod markdown_project;
mod seed_input;
mod sitemap;
mod transaction;

pub use admission::{admit_frontier_item, seed_frontier};
pub use assets::{fingerprint, hydrate_assets};
pub use crawl::{
    CrawlOptions, CrawlOutput, CrawlState, LOW_YIELD_MIN_CONTENT_BLOCKS, LOW_YIELD_MIN_TEXT_CHARS, LOW_YIELD_SCRIPT_MIN_BYTES,
    LOW_YIELD_SCRIPT_TEXT_RATIO, PageYield, ROBOTS_STORE_TTL, resume_crawl_from_checkpoint,
    resume_crawl_from_checkpoint_with_store, run_crawl, run_crawl_with_options, run_crawl_with_store,
};
pub use crawler_browser::{
    BrowserProvider, BrowserProviderStack, ChallengeBlocked, DEFAULT_SNAPSHOT_DOCUMENT_BUDGET_BYTES,
    DEFAULT_SNAPSHOT_TRANSFER_BUDGET_BYTES, FixtureBrowserProvider, HttpBrowserProvider, build_browser_stack,
    load_fixture_provider_from_dir, load_fixture_provider_from_dirs, load_snapshot_json, parse_browser_dirs,
    path_looks_like_snapshot, run_extract_from_snapshot, save_snapshot_json, snapshot_to_fetch_artifact,
};
pub use crawler_fetch::{
    DEFAULT_FETCH_BODY_BUDGET_BYTES, DiskResponseCache, FetchClient, FetchRequest, FileTransport, HttpTransport, MemoryEntry,
    MemoryTransport, ResponseCache, RobotsPolicy, SeedTransport, Transport, build_artifact, fetch_url, map_transport_error,
    transport_for_url,
};
pub use crawler_frontier::{FrontierQueue, frontier_from_links};
#[cfg(feature = "yydb")]
pub use crawler_store::YydbResponseCache;
pub use crawler_types::{BrowserSnapshot, PageChallengeState};
pub use extract::{Extractor, HtmlExtractor, run_extract, select_extractor};
pub use frontier_scheduler::{EMPTY_HOST_RETRY, HostPolicy, HostScheduler, StoreFrontier};
pub use input::{InputFormat, LoadedInput, load_input, load_input_with_source, parse_input_format};
pub use inspect::{inspect_artifact, inspect_snapshot};
pub use markdown_project::{PublishedPageProject, publish_extracted_page};
pub use seed_input::resolve_crawl_seed_specs;
pub use sitemap::{MAX_SITEMAP_LOCATIONS, locations, sitemap_body};
pub use transaction::commit_page;

use boost_diagnostic::{DiagnosticSeverity, Message};
use crawler_types::{CrawlError, CrawlReport, CrawlRequest, Result, diagnostics as pd};

/// Planned crawl work units without performing network I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrawlPlan {
    /// Normalized request used to build the plan.
    pub request: CrawlRequest,
    /// Seed URLs expanded for the frontier.
    pub frontier: Vec<String>,
}

/// Build a crawl plan from a request.
pub fn plan_crawl(request: CrawlRequest) -> Result<CrawlPlan> {
    let items = seed_frontier(&request)?;
    let frontier = items.iter().map(|item| item.request_url.to_string()).collect();
    Ok(CrawlPlan { request, frontier })
}

/// Initialize an empty report for the primary seed.
pub fn initial_report(request: &CrawlRequest) -> Result<CrawlReport> {
    let seed = request
        .seeds
        .first()
        .ok_or_else(|| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::REQUEST_INVALID),
                DiagnosticSeverity::Error,
                pd::origin_crawl(),
                Message::new("crawler.diagnostic.seedsEmpty").with_fallback("at least one seed is required".to_string()),
            );
            CrawlError::from(diag)
        })?
        .url
        .clone();
    Ok(CrawlReport::empty(seed))
}
