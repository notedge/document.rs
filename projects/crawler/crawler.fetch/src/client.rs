use std::{
    collections::BTreeMap,
    thread,
    time::{Duration, Instant},
};

use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_types::{CrawlError, FetchArtifact, Result, TransportProvenance, diagnostics as pd};
use url::Url;

use super::{FetchRequest, Transport, cache::ResponseCache, robots::RobotsPolicy, robots_txt::site_key_for_url};

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

/// Delay requested by a `Retry-After` header, in numeric seconds.
///
/// The transport never sleeps on retries: the orchestrator requeues the
/// frontier item and delays only the affected host by this value.
pub fn retry_after_delay(artifact: &FetchArtifact) -> Option<Duration> {
    let raw = artifact.response_headers.get("retry-after")?;
    raw.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// Fetch orchestration with robots checks and response caching.
pub struct FetchClient<T: Transport> {
    transport: T,
    cache: ResponseCache,
    robots: RobotsPolicy,
    last_site_requests: BTreeMap<String, Instant>,
    robots_updates: Vec<(String, String)>,
}

impl<T: Transport> FetchClient<T> {
    /// Build a client around a transport and robots policy.
    pub fn new(transport: T, robots: RobotsPolicy) -> Self {
        Self {
            transport,
            cache: ResponseCache::default(),
            robots,
            last_site_requests: BTreeMap::new(),
            robots_updates: Vec::new(),
        }
    }

    /// Access the in-memory response cache.
    pub fn cache(&self) -> &ResponseCache {
        &self.cache
    }

    /// Mutable in-memory cache access for tests.
    pub fn cache_mut(&mut self) -> &mut ResponseCache {
        &mut self.cache
    }

    /// Robots policy used for admission checks.
    pub fn robots(&self) -> &RobotsPolicy {
        &self.robots
    }

    /// Seed robots rules from a persistent cache instead of re-fetching them.
    pub fn load_site_robots(&mut self, site_key: &str, body: &str) {
        self.robots.load_site_rules(site_key, body);
    }

    /// Drain robots bodies fetched during this client's lifetime for persistence.
    pub fn take_robots_updates(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.robots_updates)
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

        let cache_key = super::cache_key_for(&request.url, None);

        // Clone the hit before revalidation because revalidation needs `&mut self`.
        let cached = self.cache.get(&cache_key).cloned();

        if let Some(cached) = cached {
            if let Some(revalidated) = self.try_revalidate(request, &cached)? {
                return Ok(revalidated);
            }
            return Ok(with_cache_provenance(cached));
        }

        self.wait_for_crawl_delay(&request.url);
        self.last_site_requests.insert(site_key_for_url(&request.url), Instant::now());
        let artifact = self.transport.fetch(request)?;

        // Only successful responses are cacheable. Retryable and error
        // statuses stay out of the cache so the orchestrator's retry
        // revalidates them against the network instead of replaying a cached
        // failure.
        if (200..300).contains(&artifact.status) {
            let key = self.cache.put(artifact);
            return Ok(self.cache.get(&key).cloned().expect("artifact inserted into cache"));
        }
        Ok(artifact)
    }

    fn try_revalidate(&mut self, request: &FetchRequest, cached: &FetchArtifact) -> Result<Option<FetchArtifact>> {
        let cache_key = super::cache_key_for(&request.url, None);
        let (etag, last_modified) = match self.cache.validators_for(&cache_key) {
            Some(v) => v,
            None => return Ok(None),
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
                let _ = self.cache.put(result.clone());
                Ok(Some(result))
            }
            Ok(artifact) => {
                let _ = self.cache.put(artifact.clone());
                Ok(Some(artifact))
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
                    let text = String::from_utf8_lossy(body).into_owned();
                    self.robots.load_site_rules(&site_key, &text);
                    self.robots_updates.push((site_key.clone(), text));
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
