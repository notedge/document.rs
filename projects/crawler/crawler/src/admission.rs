use crawler_types::{AdmissionDecision, CrawlRequest, CrawlSeed, FrontierItem, Result, canonicalize_request_url, site_key};

/// Stable dotted reason codes for frontier rejection.
pub const REJECT_UNSUPPORTED_SCHEME: &str = "crawler.reject.unsupported-scheme";
/// URL host is outside the run scope allow-list.
pub const REJECT_OUT_OF_SCOPE: &str = "crawler.reject.out-of-scope";
pub const REJECT_DEPTH_EXCEEDED: &str = "crawler.reject.depth-exceeded";

/// Evaluate whether a frontier item may proceed to fetch.
pub fn admit_frontier_item(item: &FrontierItem, request: &CrawlRequest) -> AdmissionDecision {
    if item.request_url.scheme() != "http" && item.request_url.scheme() != "https" && item.request_url.scheme() != "file" {
        return AdmissionDecision::Rejected { reason: REJECT_UNSUPPORTED_SCHEME.into() };
    }

    if request.scope.as_ref().is_some_and(|scope| !scope.allows(&item.request_url)) {
        return AdmissionDecision::Rejected { reason: REJECT_OUT_OF_SCOPE.into() };
    }

    if item.depth > request.budget.max_depth {
        return AdmissionDecision::Rejected { reason: REJECT_DEPTH_EXCEEDED.into() };
    }

    AdmissionDecision::Admitted
}

/// Build seed frontier items from a crawl request.
pub fn seed_frontier(request: &CrawlRequest) -> Result<Vec<FrontierItem>> {
    let mut items = Vec::with_capacity(request.seeds.len());
    for seed in &request.seeds {
        items.push(seed_to_frontier(seed)?);
    }
    Ok(items)
}

fn seed_to_frontier(seed: &CrawlSeed) -> Result<FrontierItem> {
    let request_url = canonicalize_request_url(seed.url.as_str())?;
    Ok(FrontierItem::seed(request_url.clone(), site_key(&request_url)))
}
