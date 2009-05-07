//! Extracted from `src/browser/http.rs`.

use crawler::HttpBrowserProvider;
use url::Url;
#[test]
fn snapshot_url_appends_page_query() {
    let provider = HttpBrowserProvider::new(Url::parse("http://127.0.0.1:9333/snapshot").expect("url"));
    let page = Url::parse("https://example.com/private").expect("url");
    let request = provider.snapshot_url(&page);
    assert_eq!(request.as_str(), "http://127.0.0.1:9333/snapshot?url=https%3A%2F%2Fexample.com%2Fprivate");
}
