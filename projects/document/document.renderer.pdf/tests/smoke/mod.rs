//! PDF smoke: the writer emits a parseable byte stream.

use document_presentation::display::{
    DisplayAnchor, DisplayItem, DisplayPage, DisplayPresentation, DisplayTextRun,
};
use document_renderer_pdf::{PdfFontPolicy, render_pdf_with_font_policy};

#[test]
fn pdf_writer_emits_a_parseable_document() {
    let presentation = DisplayPresentation {
        pages: vec![DisplayPage {
            width: 595.0,
            height: 842.0,
            items: vec![DisplayItem::TextRun(DisplayTextRun {
                x: 72.0,
                y: 100.0,
                size: 12.0,
                width: 120.0,
                font_family: "serif".to_string(),
                style: document_compose::math::FontStyle::Normal,
                text: "PDF smoke".to_string(),
                href: None,
                anchor: None,
                title: None,
                color: None,
                numeric: false,
                artifact: false,
            })],
            anchors: Vec::new(),
        }],
    };

    // ASCII text needs no embedded face, so the base-14 policy keeps the smoke
    // test free of host font bytes.
    let bytes = render_pdf_with_font_policy(&presentation, &PdfFontPolicy::Base14Only).expect("pdf render");
    assert!(bytes.starts_with(b"%PDF-"), "missing pdf header");
    assert!(bytes.ends_with(b"%%EOF\n") || bytes.ends_with(b"%%EOF"), "missing pdf trailer");
    assert!(bytes.windows(4).any(|window| window == b"xref"), "missing xref table");
}

fn linked_run(text: &str, href: Option<&str>, anchor: Option<&str>) -> DisplayTextRun {
    DisplayTextRun {
        x: 72.0,
        y: 700.0,
        size: 12.0,
        width: 60.0,
        font_family: "serif".to_string(),
        style: document_compose::math::FontStyle::Normal,
        text: text.to_string(),
        href: href.map(str::to_string),
        anchor: anchor.map(str::to_string),
        title: None,
        color: None,
        numeric: false,
        artifact: false,
    }
}

#[test]
fn pdf_writer_emits_a_destination_annotation_for_an_internal_anchor() {
    let presentation = DisplayPresentation {
        pages: vec![DisplayPage {
            width: 595.0,
            height: 842.0,
            items: vec![DisplayItem::TextRun(linked_run("jump", Some("#target"), None))],
            anchors: vec![DisplayAnchor { name: "target".to_string(), y: 100.0 }],
        }],
    };

    let bytes = render_pdf_with_font_policy(&presentation, &PdfFontPolicy::Base14Only).expect("pdf render");
    let pdf = String::from_utf8_lossy(&bytes);
    assert!(pdf.contains("/Subtype /Link"), "the fragment link needs an annotation: {pdf}");
    assert!(pdf.contains("/Dest ["), "the annotation needs a page destination: {pdf}");
    // The anchor top is flipped into PDF user space: 842 - 100 = 742.
    assert!(pdf.contains("/XYZ 0 742 0]"), "the destination keeps the anchor top: {pdf}");
    assert!(!pdf.contains("/S /URI"), "an internal fragment must not become a URI action: {pdf}");
}

#[test]
fn pdf_writer_leaves_runs_without_a_destination_unlinked() {
    let presentation = DisplayPresentation {
        pages: vec![DisplayPage {
            width: 595.0,
            height: 842.0,
            items: vec![DisplayItem::TextRun(linked_run("plain words", None, None))],
            anchors: Vec::new(),
        }],
    };

    let bytes = render_pdf_with_font_policy(&presentation, &PdfFontPolicy::Base14Only).expect("pdf render");
    let pdf = String::from_utf8_lossy(&bytes);
    assert!(!pdf.contains("/Subtype /Link"), "no destination means no annotation: {pdf}");
    assert!(!pdf.contains("/Dest ["), "no destination means no jump: {pdf}");
}
