use document_convert::{supported_routes, supports_route};

const SOURCES: &[&str] = &["docx", "doc", "pdf", "epub", "html", "markdown", "notedown"];

#[test]
fn every_source_routes_to_deliverable_writers() {
    for from in SOURCES {
        assert!(supports_route(from, "markdown"), "{from} -> markdown");
        assert!(supports_route(from, "html"), "{from} -> html");
        assert!(supports_route(from, "epub"), "{from} -> epub");
    }
}

#[test]
fn unavailable_exports_are_not_advertised() {
    // `document.formats::capabilities()` marks both exports `Unavailable`:
    // the conservative DOCX exporter rejects `Math` / `RawHtml` / opaque
    // blocks, and `export::pdf` cannot deliver CJK or typeset math at all.
    for from in SOURCES {
        assert!(!supports_route(from, "docx"), "{from} -> docx");
        assert!(!supports_route(from, "pdf"), "{from} -> pdf");
    }
}

#[test]
fn unwired_or_unknown_formats_are_rejected() {
    assert!(!supports_route("markdown", "tex"));
    assert!(!supports_route("tex", "markdown"));
    assert!(!supports_route("markdown", "markdown-project"));
}

#[test]
fn supported_routes_is_the_filtered_cross_product() {
    let routes = supported_routes();
    assert_eq!(routes.len(), SOURCES.len() * 3);
    assert!(routes.iter().all(|(from, to)| supports_route(from, to)));
    assert!(!routes.iter().any(|(_, to)| to == "docx" || to == "pdf"));
}
