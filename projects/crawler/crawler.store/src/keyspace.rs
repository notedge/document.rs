//! Centralized key encoding for store records.

/// Key holding the native store schema version.
pub fn schema_version() -> &'static str {
    "store/schema_version"
}

/// Key holding the active run identifier.
pub fn active_run() -> &'static str {
    "store/active_run"
}

/// Prefix covering every frontier item of a run.
pub fn frontier_prefix(run_id: &str) -> String {
    format!("run/{run_id}/frontier/")
}

/// Prefix covering every claim record of a run.
pub fn claim_prefix(run_id: &str) -> String {
    format!("run/{run_id}/claim/")
}

/// Prefix covering every page record of a run.
pub fn page_prefix(run_id: &str) -> String {
    format!("run/{run_id}/page/")
}

/// Run metadata key.
pub fn run_meta(run_id: &str) -> String {
    format!("run/{run_id}/meta")
}

/// Run-scoped monotonic fencing counter.
pub fn run_fencing(run_id: &str) -> String {
    format!("run/{run_id}/fencing")
}

/// Frontier item key.
pub fn frontier_item(run_id: &str, item_id: &str) -> String {
    format!("run/{run_id}/frontier/{item_id}")
}

/// Claim state key.
pub fn claim_state(run_id: &str, item_id: &str) -> String {
    format!("run/{run_id}/claim/{item_id}")
}

/// Page state key.
pub fn page_state(run_id: &str, page_id: &str) -> String {
    format!("run/{run_id}/page/{page_id}")
}

/// Metadata key referencing a CAS object attached to a page.
pub fn page_object(run_id: &str, page_id: &str, slot: &str) -> String {
    format!("run/{run_id}/asset/{page_id}-{slot}")
}

/// Report event key.
pub fn report_event(run_id: &str, sequence: u64) -> String {
    format!("run/{run_id}/event/{sequence}")
}

/// Prefix covering every checkpoint view of a run.
pub fn checkpoint_prefix(run_id: &str) -> String {
    format!("run/{run_id}/checkpoint/")
}

/// Checkpoint view key carrying the exported event sequence.
pub fn checkpoint_entry(run_id: &str, sequence: u64) -> String {
    format!("run/{run_id}/checkpoint/{sequence}")
}

/// Response cache record key.
pub fn response_cache(cache_key: &str) -> String {
    format!("cache/response/{cache_key}")
}

/// Robots cache record key.
pub fn robots_cache(site_key: &str) -> String {
    format!("cache/robots/{site_key}")
}

/// Deterministic item id from a canonical request URL.
pub fn item_id_from_url(canonical_request_url: &str) -> String {
    let digest = blake3::hash(canonical_request_url.as_bytes());
    digest.to_hex()[..16].to_string()
}

/// Deterministic run id from a seed URL.
pub fn run_id_from_seed(seed_url: &str) -> String {
    let digest = blake3::hash(seed_url.as_bytes());
    digest.to_hex()[..16].to_string()
}
