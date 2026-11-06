use crawler_extract::{Extractor, run_extract};
use crawler_types::{
    BrowserSnapshot, ChallengeOutcome, ChallengePolicy, ExtractBudget, ExtractContext, ExtractResult, PageChallengeState,
    apply_challenge_policy,
};

use crate::snapshot_to_fetch_artifact;

/// Extraction blocked by challenge policy before IR construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeBlocked {
    /// Detected page challenge state.
    pub state: PageChallengeState,
    /// Applied policy outcome.
    pub outcome: ChallengeOutcome,
}

/// Run extraction from a browser snapshot with challenge handling.
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
    let artifact = snapshot_to_fetch_artifact(snapshot);
    Ok(run_extract(&artifact, extractors, budget, ctx))
}
