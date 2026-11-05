use std::fs;

use document_formats::export::{
    docx::export_docx_bytes, epub::export_epub_bytes, html::export_html, pdf::export_pdf_bytes,
};
use document_ir::{Block, DocumentGraph, DocumentId, Inline};

use super::{DetectionConfidence, Format, codes, import_auto, import_bytes, import_file, probe};

fn sample_graph(text: &str) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(7));
    graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: text.to_owned() }] });
    graph
}

#[test]
fn probe_keeps_a_zip_package_ambiguous_without_a_hint() {
    let detection = probe(b"PK\x03\x04", None);
    assert_eq!(detection.confidence, DetectionConfidence::Ambiguous);
    assert_eq!(detection.candidates, vec!["docx", "epub"]);
}

#[test]
fn auto_refuses_an_ambiguous_zip_package() {
    let error = import_auto("pack.bin", b"PK\x03\x04", None).expect_err("a zip without a hint must not be imported");
    assert_eq!(error.code(), codes::AMBIGUOUS_FORMAT);
    assert!(error.message().contains("docx / epub"));
}

#[test]
fn auto_refuses_unknown_text() {
    let error = import_auto("source.bin", b"plain words", None).expect_err("unknown text must not default to a format");
    assert_eq!(error.code(), codes::UNKNOWN_FORMAT);
}

#[test]
fn auto_refuses_a_text_language_profile() {
    let error = import_auto("note.md", b"# Heading\n", Some("md")).expect_err("markdown belongs to its own profile");
    assert_eq!(error.code(), codes::UNSUPPORTED_FORMAT);
    assert!(error.message().contains("markdown"));
}

#[test]
fn declared_format_must_match_the_probe() {
    let error = import_bytes(Format::Docx, "source.docx", b"%PDF-1.4\n").expect_err("PDF bytes cannot be declared DOCX");
    assert_eq!(error.code(), codes::FORMAT_MISMATCH);
}

#[test]
fn imports_html_and_keeps_loss_visible() {
    let source = b"<!doctype html><html><body><h1>Probe title</h1><p>Probe body text.</p></body></html>";
    let imported = import_bytes(Format::Html, "source.html", source).expect("HTML should import");
    assert_eq!(imported.format, Format::Html);
    assert_eq!(imported.detection.format, Some("html"));
    assert!(!imported.report.lossy.is_empty(), "HTML import reports its partial coverage");
    assert!(format!("{:?}", imported.graph).contains("Probe title"));
}

#[test]
fn docx_roundtrip_keeps_paragraph_text() {
    let bytes = export_docx_bytes(&sample_graph("Roundtrip DOCX sentence")).expect("DOCX export");
    let imported = import_bytes(Format::Docx, "roundtrip.docx", &bytes).expect("DOCX import");
    assert_eq!(imported.format, Format::Docx);
    assert!(format!("{:?}", imported.graph).contains("Roundtrip DOCX sentence"));
    assert!(imported.report.lossy.iter().any(|item| item.code == "import.docx.partial_coverage"));
}

#[test]
fn epub_roundtrip_keeps_paragraph_text() {
    let bytes = export_epub_bytes(&sample_graph("Roundtrip EPUB sentence")).expect("EPUB export");
    let imported = import_bytes(Format::Epub, "roundtrip.epub", &bytes).expect("EPUB import");
    assert_eq!(imported.format, Format::Epub);
    assert!(format!("{:?}", imported.graph).contains("Roundtrip EPUB sentence"));
    assert!(imported.report.lossy.iter().any(|item| item.code == "import.epub.partial_coverage"));
}

#[test]
fn html_roundtrip_keeps_paragraph_text() {
    let html = export_html(&sample_graph("Roundtrip HTML sentence")).expect("HTML export");
    let imported = import_bytes(Format::Html, "roundtrip.html", html.as_bytes()).expect("HTML import");
    assert_eq!(imported.format, Format::Html);
    assert!(format!("{:?}", imported.graph).contains("Roundtrip HTML sentence"));
}

#[test]
fn pdf_roundtrip_keeps_text_content() {
    let bytes = export_pdf_bytes(&sample_graph("Roundtrip PDF sentence")).expect("PDF export");
    let imported = import_bytes(Format::Pdf, "roundtrip.pdf", &bytes).expect("PDF import");
    assert_eq!(imported.format, Format::Pdf);
    assert!(format!("{:?}", imported.graph).contains("Roundtrip PDF sentence"));
}

#[test]
fn file_import_routes_by_extension() {
    let path = std::env::temp_dir().join(format!("document-input-formats-{}.html", std::process::id()));
    fs::write(&path, b"<!doctype html><html><body><p>File routed.</p></body></html>").expect("temp source");
    let imported = import_file(&path, None).expect("HTML file should import");
    fs::remove_file(&path).expect("temp source removed");
    assert_eq!(imported.format, Format::Html);
    assert!(format!("{:?}", imported.graph).contains("File routed."));
}

#[test]
fn file_import_reports_a_missing_file() {
    let error = import_file("does-not-exist.docx", None).expect_err("missing input must fail");
    assert_eq!(error.code(), codes::FILE_READ_FAILED);
}
