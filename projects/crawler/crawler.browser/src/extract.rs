use crawler_extract::{Extractor, run_extract};
use crawler_types::{
    BrowserSnapshot, ChallengeOutcome, ChallengePolicy, ExtractBudget, ExtractContext, ExtractResult, ExtractStatus,
    PageChallengeState, apply_challenge_policy,
    document_ir::{LossMarker, SemanticStatus},
};

use crate::{provider::validate_browser_boundary, snapshot_to_fetch_artifact};

/// Extraction blocked by challenge policy before IR construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeBlocked {
    /// Detected page challenge state.
    pub state: PageChallengeState,
    /// Applied policy outcome.
    pub outcome: ChallengeOutcome,
}

/// Run extraction from a browser snapshot with challenge handling.
///
/// Only the top-level document enters the IR. Frames, shadow roots and
/// downloads stay provenance unless an explicit boundary profile consumes them,
/// and every skipped boundary is reported as a semantic loss marker.
pub fn run_extract_from_snapshot(
    snapshot: &BrowserSnapshot,
    policy: ChallengePolicy,
    extractors: &[&dyn Extractor],
    budget: ExtractBudget,
    ctx: &ExtractContext,
) -> Result<ExtractResult, ChallengeBlocked> {
    let outcome = apply_challenge_policy(snapshot.challenge_state, policy);
    if outcome != ChallengeOutcome::Proceed {
        return Err(ChallengeBlocked { state: snapshot.challenge_state, outcome });
    }
    if validate_browser_boundary(snapshot).is_err() {
        return Err(ChallengeBlocked { state: PageChallengeState::UnsupportedBrowserFlow, outcome: ChallengeOutcome::Stop });
    }
    let artifact = snapshot_to_fetch_artifact(snapshot);
    let mut result = run_extract(&artifact, extractors, budget, ctx);
    let boundary_losses = boundary_losses(snapshot);
    if !boundary_losses.is_empty() {
        result.losses.extend(boundary_losses);
        if result.status == ExtractStatus::Complete {
            result.status = ExtractStatus::Partial;
        }
    }
    Ok(result)
}

/// Report every boundary the default top-level extraction does not consume.
fn boundary_losses(snapshot: &BrowserSnapshot) -> Vec<LossMarker> {
    let mut losses = Vec::new();
    for frame in &snapshot.boundary.frames {
        if !frame.same_origin {
            // Cross-origin frames are metadata by contract, not a coverage loss.
            continue;
        }
        losses.push(LossMarker {
            code: "crawler.browser.frame_boundary".into(),
            message: format!("same-origin iframe `{}` awaits an explicit frame profile", frame.url),
            status: SemanticStatus::Partial,
        });
    }
    for root in &snapshot.boundary.shadow_roots {
        let message = if root.open {
            format!("open shadow root on `{}` awaits an explicit shadow profile", root.host)
        }
        else {
            format!("closed shadow root on `{}` is unsupported and cannot be read", root.host)
        };
        losses.push(LossMarker {
            code: "crawler.browser.shadow_root_boundary".into(),
            message,
            status: SemanticStatus::Partial,
        });
    }
    for download in &snapshot.boundary.downloads {
        losses.push(LossMarker {
            code: "crawler.browser.download_not_materialized".into(),
            message: format!("download `{}` was recorded as provenance only", download.url),
            status: SemanticStatus::Partial,
        });
    }
    losses
}
