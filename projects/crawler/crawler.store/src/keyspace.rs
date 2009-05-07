//! Centralized key encoding for store records.

/// Run metadata key.
pub fn run_meta(run_id: &str) -> String {
    format!("run/{run_id}/meta")
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

/// Report event key.
pub fn report_event(run_id: &str, sequence: u64) -> String {
    format!("run/{run_id}/event/{sequence}")
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
