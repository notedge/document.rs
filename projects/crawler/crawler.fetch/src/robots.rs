use std::collections::BTreeMap;

use crawler_types::PolitenessProfile;
use url::Url;

use super::robots_txt::{RobotsTxt, site_key_for_url};

/// How site robots rules were obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SiteRobotsState {
    /// Parsed from a fetched `robots.txt`.
    Loaded,
    /// Fetch failed or body was unreadable. Fallback paths may still apply.
    Unavailable,
    /// Missing document or non-success status. Treat as allow-all from robots.
    Missing,
}

/// Cached robots rules for one site key.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedSiteRobots {
    state: SiteRobotsState,
    document: RobotsTxt,
}

/// Robots policy evaluation for a crawl profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobotsPolicy {
    profile: PolitenessProfile,
    user_agent: String,
    fallback_disallowed: Vec<String>,
    sites: BTreeMap<String, CachedSiteRobots>,
}

impl RobotsPolicy {
    /// Build policy from a politeness profile.
    pub fn from_profile(profile: PolitenessProfile) -> Self {
        Self::with_user_agent(profile, "crawler")
    }

    /// Build policy with an explicit crawler user-agent token.
    pub fn with_user_agent(profile: PolitenessProfile, user_agent: impl Into<String>) -> Self {
        let fallback_disallowed = match profile {
            PolitenessProfile::Conservative => vec!["/admin".into(), "/private".into()],
            PolitenessProfile::Balanced => vec!["/admin".into()],
            PolitenessProfile::Developer => Vec::new(),
        };
        Self { profile, user_agent: user_agent.into(), fallback_disallowed, sites: BTreeMap::new() }
    }

    /// Politeness profile backing this policy.
    pub fn profile(&self) -> PolitenessProfile {
        self.profile
    }

    /// Crawler user-agent token used for group selection.
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// Whether site robots rules were already resolved for the URL host.
    pub fn has_site_rules(&self, url: &Url) -> bool {
        self.sites.contains_key(&site_key_for_url(url))
    }

    /// Parse and cache robots rules for a site key.
    pub fn load_site_rules(&mut self, site_key: &str, body: &str) {
        self.sites.insert(
            site_key.to_string(),
            CachedSiteRobots { state: SiteRobotsState::Loaded, document: RobotsTxt::parse(body) },
        );
    }

    /// Mark a site as having no usable `robots.txt`.
    pub fn mark_site_missing(&mut self, site_key: &str) {
        self.sites
            .insert(site_key.to_string(), CachedSiteRobots { state: SiteRobotsState::Missing, document: RobotsTxt::default() });
    }

    /// Mark robots fetch as failed for a site.
    pub fn mark_site_unavailable(&mut self, site_key: &str) {
        self.sites.insert(
            site_key.to_string(),
            CachedSiteRobots { state: SiteRobotsState::Unavailable, document: RobotsTxt::default() },
        );
    }

    /// Crawl-delay from loaded robots for the URL site, if any.
    pub fn crawl_delay_for_url(&self, url: &Url) -> Option<u32> {
        let site = self.sites.get(&site_key_for_url(url))?;
        if !matches!(site.state, SiteRobotsState::Loaded) {
            return None;
        }
        site.document.crawl_delay_for_agent(&self.user_agent)
    }

    /// Sitemap declarations from the loaded robots document for this site.
    pub fn sitemaps_for_url(&self, url: &Url) -> &[String] {
        self.sites.get(&site_key_for_url(url)).map(|site| site.document.sitemaps.as_slice()).unwrap_or(&[])
    }

    /// Whether fetching the URL is allowed under this policy.
    pub fn is_allowed(&self, url: &Url) -> bool {
        if url.scheme() != "http" && url.scheme() != "https" && url.scheme() != "file" {
            return false;
        }
        if url.scheme() == "file" {
            return true;
        }

        let path = url.path();
        if let Some(site) = self.sites.get(&site_key_for_url(url)) {
            if matches!(site.state, SiteRobotsState::Loaded) {
                return site.document.is_path_allowed(&self.user_agent, path);
            }
        }

        if matches!(self.profile, PolitenessProfile::Developer) {
            return true;
        }

        !self.fallback_disallowed.iter().any(|prefix| path.starts_with(prefix))
    }
}
