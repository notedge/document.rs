//! Extracted from `src/url.rs`.

use crawler_types::{MAX_URL_LENGTH, canonicalize_request_url, site_key};

#[test]
fn lowercases_host_and_keeps_query() {
    let url = canonicalize_request_url("HTTPS://Example.COM/docs?id=1").expect("url");
    assert_eq!(url.as_str(), "https://example.com/docs?id=1");
}

#[test]
fn strips_fragment_to_avoid_frontier_duplicates() {
    let url_a = canonicalize_request_url("https://example.com/page#section").expect("a");
    let url_b = canonicalize_request_url("https://example.com/page").expect("b");
    assert_eq!(url_a.as_str(), "https://example.com/page");
    assert_eq!(url_a, url_b);
}

#[test]
fn strips_default_port() {
    let url = canonicalize_request_url("https://example.com:443/").expect("url");
    assert_eq!(url.as_str(), "https://example.com/");
}

#[test]
fn rejects_urls_beyond_max_length() {
    let mut long_url = String::from("https://example.com/");
    long_url.push_str(&"a".repeat(MAX_URL_LENGTH));
    let error = canonicalize_request_url(&long_url).expect_err("should reject over-length");
    assert!(error.to_string().contains("exceeds maximum length"));
}

#[test]
fn site_key_includes_non_default_port() {
    let url = canonicalize_request_url("http://example.com:8080/a").expect("url");
    assert_eq!(site_key(&url), "example.com:8080");
}
