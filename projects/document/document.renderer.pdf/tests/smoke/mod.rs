//! PDF smoke: the writer emits a parseable byte stream.

use document_presentation::display::{DisplayItem, DisplayPage, DisplayPresentation, DisplayTextRun};
use document_renderer_pdf::render_pdf;

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
                title: None,
            })],
        }],
    };

    let bytes = render_pdf(&presentation).expect("pdf render");
    assert!(bytes.starts_with(b"%PDF-"), "missing pdf header");
    assert!(bytes.ends_with(b"%%EOF\n") || bytes.ends_with(b"%%EOF"), "missing pdf trailer");
    assert!(bytes.windows(4).any(|window| window == b"xref"), "missing xref table");
}
