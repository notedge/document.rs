use serde::{Deserialize, Serialize};

use crate::browser::{ChallengePolicy, PageChallengeState};

/// Result of applying a challenge policy to a page state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeOutcome {
    /// Continue with extraction.
    Proceed,
    /// Stop the run and report the challenge.
    Stop,
    /// Skip the page and continue when applicable.
    Skip,
    /// Pause for operator-provided snapshot or authorization.
    PauseForOperator,
    /// Fall back to plain HTTP fetch when allowed.
    FallbackHttp,
}

/// Apply a challenge policy to a detected page state.
pub fn apply_challenge_policy(state: PageChallengeState, policy: ChallengePolicy) -> ChallengeOutcome {
    if state == PageChallengeState::Normal {
        return ChallengeOutcome::Proceed;
    }
    match policy {
        ChallengePolicy::Stop => ChallengeOutcome::Stop,
        ChallengePolicy::PauseForOperator => ChallengeOutcome::PauseForOperator,
        ChallengePolicy::UseExistingSession => ChallengeOutcome::Stop,
        ChallengePolicy::FallbackHttp => ChallengeOutcome::FallbackHttp,
        ChallengePolicy::SkipPage => ChallengeOutcome::Skip,
    }
}
