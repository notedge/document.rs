//! Extracted from `src/robots_txt.rs`.

use crawler_fetch::{RobotsGroup, RobotsTxt, path_matches_rule, site_key_for_url};
use url::Url;
#[test]
fn parses_user_agent_groups_and_sitemaps() {
    let body = r#"
User-agent: *
Disallow: /admin/
Allow: /admin/public/

User-agent: crawler
Disallow: /secret
Crawl-delay: 2

Sitemap: https://example.com/sitemap.xml
"#;
    let doc = RobotsTxt::parse(body);
    assert_eq!(doc.groups.len(), 2);
    assert_eq!(doc.sitemaps, vec!["https://example.com/sitemap.xml"]);
    assert_eq!(doc.crawl_delay_for_agent("crawler"), Some(2));
}

#[test]
fn longest_prefix_rule_wins() {
    let body = r#"
User-agent: *
Disallow: /docs
Allow: /docs/public
"#;
    let doc = RobotsTxt::parse(body);
    assert!(!doc.is_path_allowed("crawler", "/docs/private"));
    assert!(doc.is_path_allowed("crawler", "/docs/public/page"));
}

#[test]
fn wildcard_and_end_anchor_rules_match() {
    let body = r#"
User-agent: *
Disallow: /*.pdf$
Allow: /public/*.pdf$
"#;
    let doc = RobotsTxt::parse(body);
    assert!(!doc.is_path_allowed("bot", "/report.pdf"));
    assert!(doc.is_path_allowed("bot", "/public/guide.pdf"));
}

#[test]
fn wildcard_rules_support_multiple_stars_and_end_anchor() {
    assert!(path_matches_rule("/private/*/export/*", "/private/team/export/report"));
    assert!(!path_matches_rule("/private/*/export/*", "/private/team/import/report"));
    assert!(path_matches_rule("/api/*/v1", "/api/team/v1"));
    assert!(path_matches_rule("/api/*/v1", "/api/team/v1/extra"));
    assert!(!path_matches_rule("/api/*/v1$", "/api/team/v1/extra"));
    assert!(path_matches_rule("/api/*/v1$", "/api/team/v1"));
    assert!(path_matches_rule("/api/*$", "/api/team/v1/extra"));
}

#[test]
fn equal_specificity_allow_rule_wins_over_disallow() {
    let group = RobotsGroup {
        agents: vec!["*".into()],
        allow: vec!["/private/*/export".into()],
        disallow: vec!["/private/*/export".into()],
        crawl_delay: None,
    };
    assert!(group.is_path_allowed("/private/team/export"));
}

#[test]
fn developer_agent_uses_matching_group() {
    let body = r#"
User-agent: crawler
Disallow: /private
"#;
    let doc = RobotsTxt::parse(body);
    assert!(!doc.is_path_allowed("crawler", "/private/page"));
    assert!(doc.is_path_allowed("crawler", "/open/page"));
}

#[test]
fn site_key_includes_port() {
    let url = url::Url::parse("https://example.com:8443/page").expect("url");
    assert_eq!(site_key_for_url(&url), "https://example.com:8443");
}
