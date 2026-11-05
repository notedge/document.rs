//! Regression lanes for PDF chunk granularity.

use super::import_pdf_bytes;

/// Build a minimal PDF whose text is split into one `Tj` per character.
///
/// Chromium prints each glyph with its own `Tj` and `Td`, so a reader that
/// emits one paragraph per text operator amplifies a seven-page report into
/// thousands of blocks. This fixture keeps the same operator shape at a size
/// the test can assert exactly.
fn paginated_glyph_pdf(page_texts: &[&str]) -> Vec<u8> {
    let page_count = page_texts.len();
    let first_page = 3;
    let first_content = first_page + page_count;
    let font_id = first_content + page_count;
    let mut pdf = String::from("%PDF-1.4\n");
    pdf.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    let kids = (0..page_count).map(|index| format!("{} 0 R", first_page + index)).collect::<Vec<_>>().join(" ");
    pdf.push_str(&format!("2 0 obj\n<< /Type /Pages /Kids [{kids}] /Count {page_count} >>\nendobj\n"));
    for index in 0..page_count {
        pdf.push_str(&format!(
            "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {} 0 R >>\nendobj\n",
            first_page + index,
            first_content + index
        ));
    }
    pdf.push_str(&format!(
        "{font_id} 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n"
    ));
    for (index, text) in page_texts.iter().enumerate() {
        let content = glyph_run_stream(text);
        pdf.push_str(&format!(
            "{} 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
            first_content + index,
            content.len()
        ));
    }
    pdf.push_str("trailer\n<< /Root 1 0 R >>\n%%EOF\n");
    pdf.into_bytes()
}

fn glyph_run_stream(text: &str) -> String {
    let mut stream = String::from("BT /F1 12 Tf 20 800 Td\n");
    for character in text.chars() {
        stream.push_str(&format!("({character}) Tj 6 0 Td "));
    }
    stream.push_str("ET");
    stream
}

fn paragraph_texts(graph: &document_ir::DocumentGraph) -> Vec<String> {
    graph
        .blocks
        .iter()
        .filter_map(|node| match &node.block {
            document_ir::Block::Paragraph { content } => Some(
                content
                    .iter()
                    .filter_map(|inline| match inline {
                        document_ir::Inline::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

#[test]
fn glyph_runs_stay_one_chunk_per_source_page() {
    let pages = ["page one text", "page two text"];
    let graph = import_pdf_bytes("glyph-runs", &paginated_glyph_pdf(&pages)).expect("pdf import");

    assert_eq!(graph.blocks.len(), pages.len(), "one paragraph per source page");
    assert_eq!(paragraph_texts(&graph), pages, "page text is preserved verbatim");

    let non_whitespace: usize =
        paragraph_texts(&graph).iter().map(|text| text.chars().filter(|c| !c.is_whitespace()).count()).sum();
    let expected: usize = pages.iter().map(|text| text.chars().filter(|c| !c.is_whitespace()).count()).sum();
    assert_eq!(non_whitespace, expected, "no glyph is dropped");
}
