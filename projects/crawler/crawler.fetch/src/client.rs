use std::{
    collections::BTreeMap,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use boost_random::{Random, Secure};
use crawler_types::{CrawlError, FetchArtifact, Result, TransportProvenance, diagnostics as pd};
use url::Url;

use super::{
    FetchRequest, Transport,
    cache::ResponseCache,
    disk_cache::{DiskResponseCache, LayeredResponseCache},
    robots::RobotsPolicy,
    robots_txt::site_key_for_url,
};

/// Maximum retries for retryable HTTP status codes with `Retry-After` or exponential backoff.
const MAX_HTTP_RETRIES: usize = 3;

/// Fraction of the computed backoff used as a symmetric ±jitter window.
/// The final delay is `backoff ± backoff * JITTER_FRACTION`.
const JITTER_FRACTION: f64 = 0.25;

/// Classify an HTTP response status for crawl policy decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpStatusClass {
    /// Normal — proceed with extract pipeline (2xx, informational, or non-error client 3xx).
    Proceed,
    /// Permanent failure — do not retry and do not re-enqueue.
    /// Includes 404 Not Found and 410 Gone.
    PermanentFailure,
    /// Temporary failure — retry after a delay (429, 500, 502, 503).
    Retryable,
    /// Client error — do not retry, but not marked as permanent (401, 403).
    ClientError,
}

/// Classify an HTTP status code into a crawl policy bucket.
pub fn classify_http_status(status: u16) -> HttpStatusClass {
    match status {
        404 | 410 => HttpStatusClass::PermanentFailure,
        429 | 500 | 502 | 503 => HttpStatusClass::Retryable,
        401 | 403 => HttpStatusClass::ClientError,
        _ => HttpStatusClass::Proceed,
    }
}

/// Whether an HTTP status warrants a retry after a delay.
fn is_retryable_status(status: u16) -> bool {
    matches!(classify_http_status(status), HttpStatusClass::Retryable)
}

/// Compute the delay before the next retry attempt.
///
/// Prefers the `Retry-After` header (numeric seconds only); falls back to
/// exponential backoff bounded at 30 seconds with ±25% jitter. Returns `None`
/// when the status is not retryable.
fn compute_retry_delay(artifact: &FetchArtifact, attempt: usize) -> Option<Duration> {
    if !is_retryable_status(artifact.status) {
        return None;
    }
    if let Some(raw) = artifact.response_headers.get("retry-after") {
        if let Ok(seconds) = raw.trim().parse::<u64>() {
            return Some(Duration::from_secs(seconds));
        }
    }
    let base_secs = 2u64.pow(attempt.min(4) as u32).min(30);
    let jittered = apply_jitter(base_secs);
    Some(Duration::from_secs(jittered.max(1)))
}

/// Apply ±25% jitter to a base seconds value.
///
/// Uses the current monotonic clock's nanosecond fraction as a cheap random
/// source so we don't need a dedicated RNG dependency.
fn apply_jitter(base: u64) -> u64 {
    // Sample a uniform f64 in [0, 1) from the OS entropy source.
    let frac = Random::new(Secure::system()).unit_f64().unwrap_or_else(|_| 0.5);
    // Map to [-1, 1], scale by JITTER_FRACTION, apply symmetric ±jitter.
    let offset = (frac - 0.5) * 2.0 * JITTER_FRACTION;
    let jittered = base as f64 * (1.0 + offset);
    jittered.round() as u64
}

/// Fetch orchestration with robots checks and response caching.
pub struct FetchClient<T: Transport> {
    transport: T,
    cache: LayeredResponseCache,
    robots: RobotsPolicy,
    last_site_requests: BTreeMap<String, Instant>,
}

impl<T: Transport> FetchClient<T> {
    /// Build a client around a transport and robots policy.
    pub fn new(transport: T, robots: RobotsPolicy) -> Self {
        Self { transport, cache: LayeredResponseCache::memory_only(), robots, last_site_requests: BTreeMap::new() }
    }

    /// Persist responses under the given cache directory.
    pub fn with_disk_cache_dir(mut self, dir: impl AsRef<Path>) -> Result<Self> {
        self.cache = LayeredResponseCache::with_disk(DiskResponseCache::open(dir)?);
        Ok(self)
    }

    /// Access the in-memory response cache.
    pub fn cache(&self) -> &ResponseCache {
        self.cache.memory()
    }

    /// Mutable in-memory cache access for tests.
    pub fn cache_mut(&mut self) -> &mut ResponseCache {
        self.cache.memory_mut()
    }

    /// Robots policy used for admission checks.
    pub fn robots(&self) -> &RobotsPolicy {
        &self.robots
    }

    /// Whether a site key was recorded after a fetch attempt.
    pub fn has_tracked_site(&self, site: &str) -> bool {
        self.last_site_requests.contains_key(site)
    }

    /// Fetch one URL through robots, cache, and transport.
    pub fn fetch(&mut self, request: &FetchRequest) -> Result<FetchArtifact> {
        if should_resolve_robots(&request.url) {
            self.ensure_site_robots(request)?;
        }

        if !self.robots.is_allowed(&request.url) {
            let diagnostic = boost_diagnostic::Diagnostic::new(
                pd::code(pd::ROBOTS_DENIED),
                DiagnosticSeverity::Warning,
                pd::origin_robots(),
                Message::new("crawler.diagnostic.robotsDenied")
                    .with_arg("url", MessageArg::Text(request.url.to_string()))
                    .with_fallback(format!("robots policy denied `{}`", request.url)),
            );
            return Err(CrawlError::from(diagnostic));
        }

        let request_key = request.url.to_string();
        let cache_key = super::cache_key_for(&request.url, None);

        // Cache hit — drop the cache-layer &mut borrow before attempting revalidation
        // (which needs &mut self for transport fetch and cache put).
        let cached = self.cache.get_by_request_url(&request_key)?.or_else(|| self.cache.get(&cache_key).ok().flatten());

        if let Some(cached) = cached {
            if let Some(revalidated) = self.try_revalidate(request, &cached)? {
                return Ok(revalidated);
            }
            return Ok(with_cache_provenance(cached));
        }

        self.wait_for_crawl_delay(&request.url);
        self.last_site_requests.insert(site_key_for_url(&request.url), Instant::now());
        let mut artifact = self.transport.fetch(request)?;

        for attempt in 0..MAX_HTTP_RETRIES {
            let Some(delay) = compute_retry_delay(&artifact, attempt)
            else {
                break;
            };
            thread::sleep(delay);
            self.wait_for_crawl_delay(&request.url);
            self.last_site_requests.insert(site_key_for_url(&request.url), Instant::now());
            artifact = self.transport.fetch(request)?;
        }

        let key = self.cache.put(artifact)?;
        Ok(self.cache.get(&key)?.expect("artifact inserted into cache"))
    }

    fn try_revalidate(&mut self, request: &FetchRequest, cached: &FetchArtifact) -> Result<Option<FetchArtifact>> {
        let request_url = request.url.to_string();
        let cache_key = super::cache_key_for(&request.url, None);
        let (etag, last_modified) = match self.cache.validators_for_url(&request_url)? {
            Some(v) => v,
            None => match self.cache.validators_for_cache_key(&cache_key) {
                Some(v) => v,
                None => return Ok(None),
            },
        };

        let mut cond_request = request.clone();
        cond_request.headers.insert("if-none-match".into(), etag);
        cond_request.headers.insert("if-modified-since".into(), last_modified);

        self.wait_for_crawl_delay(&request.url);
        self.last_site_requests.insert(site_key_for_url(&request.url), Instant::now());
        match self.transport.fetch(&cond_request) {
            Ok(artifact) if artifact.status == 304 => {
                let mut result = artifact;
                result.body = cached.body.clone();
                result.transport_provenance = TransportProvenance::CacheHit;
                let _ = self.cache.put(result.clone())?;
                Ok(Some(result))
            }
            Ok(artifact) => {
                let key = self.cache.put(artifact.clone())?;
                Ok(Some(self.cache.get(&key)?.expect("artifact inserted")))
            }
            Err(_) => Ok(None),
        }
    }

    fn wait_for_crawl_delay(&self, url: &Url) {
        let Some(delay_seconds) = self.robots.crawl_delay_for_url(url)
        else {
            return;
        };
        let Some(last_request) = self.last_site_requests.get(&site_key_for_url(url))
        else {
            return;
        };
        let delay = Duration::from_secs(u64::from(delay_seconds));
        if let Some(remaining) = delay.checked_sub(last_request.elapsed()) {
            thread::sleep(remaining);
        }
    }

    fn ensure_site_robots(&mut self, request: &FetchRequest) -> Result<()> {
        if self.robots.has_site_rules(&request.url) {
            return Ok(());
        }

        let site_key = site_key_for_url(&request.url);
        let robots_url = robots_txt_url_for(&request.url)?;
        let robots_request = FetchRequest { url: robots_url, headers: BTreeMap::new() };

        match self.transport.fetch(&robots_request) {
            Ok(artifact) if artifact.status == 200 => {
                if let Some(body) = artifact.body_bytes() {
                    let text = String::from_utf8_lossy(body);
                    self.robots.load_site_rules(&site_key, &text);
                }
                else {
                    self.robots.mark_site_unavailable(&site_key);
                }
            }
            Ok(_) => self.robots.mark_site_missing(&site_key),
            Err(_) => self.robots.mark_site_unavailable(&site_key),
        }
        Ok(())
    }
}

fn should_resolve_robots(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https") && !is_robots_txt_url(url)
}

fn is_robots_txt_url(url: &Url) -> bool {
    url.path().eq_ignore_ascii_case("/robots.txt")
}

fn robots_txt_url_for(base: &Url) -> Result<Url> {
    let mut url = base.clone();
    url.set_path("/robots.txt");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn with_cache_provenance(mut artifact: FetchArtifact) -> FetchArtifact {
    artifact.transport_provenance = TransportProvenance::CacheHit;
    artifact
}

/// Convenience fetch with empty headers.
pub fn fetch_url<T: Transport>(client: &mut FetchClient<T>, url: Url) -> Result<FetchArtifact> {
    client.fetch(&FetchRequest { url, headers: BTreeMap::new() })
}
