use crawler_types::{BrowserSnapshot, ChallengePolicy, ExtractBudget, InspectStage, apply_challenge_policy};

use crawler_extract::Extractor;

pub use crawler_extract::inspect_artifact;

/// Inspect a browser snapshot without committing IR.
pub fn inspect_snapshot(
    snapshot: &BrowserSnapshot,
    policy: ChallengePolicy,
    extractors: &[&dyn Extractor],
    stage: InspectStage,
    budget: ExtractBudget,
) -> crawler_types::InspectReport {
    let mut report =
        crawler_extract::inspect_artifact(&crate::browser::snapshot_to_fetch_artifact(snapshot), extractors, stage, budget);
    report.challenge_state = Some(snapshot.challenge_state);
    report.challenge_outcome = Some(apply_challenge_policy(snapshot.challenge_state, policy));
    report
}
