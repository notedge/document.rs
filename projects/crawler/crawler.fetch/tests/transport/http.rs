//! Extracted from `src/http.rs`.

use std::collections::BTreeMap;
use url::Url;

use crawler_fetch::{FetchRequest, HttpTransport, Transport};

#[test]
fn rejects_non_http_schemes() {
    let transport = HttpTransport::new();
    let request = FetchRequest { url: Url::parse("file:///tmp/page.html").expect("url"), headers: BTreeMap::new() };
    let error = transport.fetch(&request).unwrap_err();
    assert!(error.to_string().contains("http transport"));
}
