//! Extracted from `src/memory.rs`.

use std::collections::BTreeMap;

use crawler_fetch::{FetchRequest, MemoryEntry, MemoryTransport, Transport};
use url::Url;

#[test]
fn serves_registered_response() {
    let mut transport = MemoryTransport::new();
    transport.insert(
        "https://example.com/page.html",
        MemoryEntry {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/html".into())]),
            body: b"<html></html>".to_vec(),
        },
    );
    let artifact = transport
        .fetch(&FetchRequest { url: Url::parse("https://example.com/page.html").expect("url"), headers: BTreeMap::new() })
        .expect("artifact");
    assert_eq!(artifact.status, 200);
    assert_eq!(artifact.content_type.as_deref(), Some("text/html"));
}
