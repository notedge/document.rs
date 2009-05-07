use serde::{Deserialize, Serialize};
use url::Url;

/// Opaque credential reference. Secrets never appear in logs, reports, or IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    /// Stable reference identifier provided by the host secret provider.
    pub id: String,
}

/// How an authenticated browser session may persist across runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPersistence {
    /// Destroy the browser context after the run.
    None,
    /// Keep encrypted ephemeral session material for the current host only.
    EncryptedEphemeral,
    /// Persist only an external vault handle and expiry metadata.
    ExternalVault,
}

/// Explicit login profile for controlled browser collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthProfile {
    /// Origins that may receive credentials or post-login navigation.
    pub allowed_origins: Vec<String>,
    /// Login entry URL or route.
    pub login_entry: Url,
    /// Verifiable success condition, such as a DOM marker or final origin.
    pub success_condition: String,
    /// Credential references supplied by the host.
    pub credential_refs: Vec<CredentialRef>,
    /// Login flow timeout in milliseconds.
    pub timeout_ms: u64,
    /// Maximum login attempts per run.
    pub max_attempts: u32,
    /// Session persistence strategy.
    pub persistence: SessionPersistence,
}

/// High-level page state used for challenge handling and reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageChallengeState {
    /// Page is ready for normal extraction.
    Normal,
    /// Login is required before content can be collected.
    LoginRequired,
    /// A challenge or second factor requires operator action.
    ChallengeRequired,
    /// The site is rate limiting automated access.
    RateLimited,
    /// Access was denied by the origin.
    AccessDenied,
    /// Consent or cookie banner must be resolved first.
    ConsentRequired,
    /// Rendering is unstable or incomplete.
    UnstableRender,
    /// The browser flow is unsupported by the current profile.
    UnsupportedBrowserFlow,
}

/// Policy for responding to challenge states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengePolicy {
    /// Stop the run and report the challenge.
    Stop,
    /// Pause and wait for operator authorization or a new snapshot.
    PauseForOperator,
    /// Reuse an already authorized external session only.
    UseExistingSession,
    /// Fall back to plain HTTP fetch when allowed.
    FallbackHttp,
    /// Skip the page and continue the frontier.
    SkipPage,
}

/// Restricted browser interaction kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserActionKind {
    /// Navigate to a URL.
    Goto,
    /// Click a selector.
    Click,
    /// Fill a selector using a credential reference.
    Fill,
    /// Select an option.
    Select,
    /// Scroll a container.
    Scroll,
    /// Wait for a selector, network idle, or timeout.
    WaitFor,
    /// Capture a browser snapshot.
    Capture,
}

/// Browser snapshot handed from `BrowserProvider` to Crawler extractors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSnapshot {
    /// Requested navigation URL.
    pub requested_url: Url,
    /// Final URL after redirects and client-side navigation.
    pub final_url: Url,
    /// Rendered HTML or DOM snapshot bytes as UTF-8 text.
    pub document_html: String,
    /// Unix epoch seconds when the snapshot was captured.
    pub captured_at_epoch: u64,
    /// Browser engine label, such as `chromium-120`.
    pub browser_engine: String,
    /// Profile identifier for provenance.
    pub profile_id: String,
    /// Detected challenge state for the page.
    pub challenge_state: PageChallengeState,
}
