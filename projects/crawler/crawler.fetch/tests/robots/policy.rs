//! Extracted from `src/robots.rs`.

use crawler_fetch::RobotsPolicy;
use crawler_types::PolitenessProfile;
use url::Url;

#[test]
fn conservative_blocks_admin_paths_without_robots() {
    let policy = RobotsPolicy::from_profile(PolitenessProfile::Conservative);
    let url = Url::parse("https://example.com/admin/settings").expect("url");
    assert!(!policy.is_allowed(&url));
}

#[test]
fn loaded_robots_denies_even_for_developer_profile() {
    let mut policy = RobotsPolicy::from_profile(PolitenessProfile::Developer);
    policy.load_site_rules("https://example.com:443", "User-agent: *\nDisallow: /secret\n");
    let url = Url::parse("https://example.com/secret/page").expect("url");
    assert!(!policy.is_allowed(&url));
    let open = Url::parse("https://example.com/open/page").expect("url");
    assert!(policy.is_allowed(&open));
}

#[test]
fn missing_robots_falls_back_for_conservative_profile() {
    let mut policy = RobotsPolicy::from_profile(PolitenessProfile::Conservative);
    policy.mark_site_missing("https://example.com:443");
    let url = Url::parse("https://example.com/private/data").expect("url");
    assert!(!policy.is_allowed(&url));
}
