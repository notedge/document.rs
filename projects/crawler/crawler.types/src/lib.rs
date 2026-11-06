#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("readme.md")]

pub mod diagnostics;

mod artifact;
mod browser;
mod challenge;
mod error;
mod events;
mod extract;
mod frontier;
mod inspect;
mod request;
mod result;
mod url;

pub use crate::{
    artifact::{BodyStorage, FetchArtifact, HttpStatus, RedirectHop, TransportProvenance},
    browser::{
        AuthProfile, BrowserActionKind, BrowserSnapshot, ChallengePolicy, CredentialRef, PageChallengeState, SessionPersistence,
    },
    challenge::{ChallengeOutcome, apply_challenge_policy},
    diagnostics::{
        NAMESPACE as DIAGNOSTIC_NAMESPACE, code, origin_browser, origin_crawl, origin_extract, origin_fetch, origin_fetch_http,
        origin_frontier, origin_politeness, origin_robots, origin_store,
    },
    error::{CrawlError, Result},
    events::{BrowserFallbackTrigger, CrawlEvent, EventLog, PageTransaction},
    extract::{
        ExtractBudget, ExtractContext, ExtractInput, ExtractOutcome, ExtractResult, ExtractStatus, ProbeResult, ProbeStatus,
    },
    frontier::{AdmissionDecision, DiscoverySource, FrontierItem, LinkCandidate, LinkRelation},
    inspect::{InspectReport, InspectStage},
    request::{BrowserFallbackPolicy, CrawlBudget, CrawlRequest, CrawlScope, CrawlSeed, PolitenessProfile},
    result::{CrawlBudgetStatus, CrawlPageStatus, CrawlReport},
    url::{MAX_URL_LENGTH, canonicalize_parsed_url, canonicalize_request_url, now_epoch, site_key},
};

/// Notedown document semantic IR — Crawler extraction hub type.
pub use document_ir;
