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

/// Build a one-page PDF whose font resource may be glued to delimiters.
fn single_page_font_pdf(font_dictionary: &str, font_section: &str, to_unicode: Option<&str>, content: &str) -> Vec<u8> {
    let to_unicode_reference = if to_unicode.is_some() { " /ToUnicode 6 0 R" } else { "" };
    let mut pdf = String::from("%PDF-1.4\n");
    pdf.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    pdf.push_str("2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    pdf.push_str(&format!(
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << {font_section} >> /Contents 5 0 R >>\nendobj\n"
    ));
    pdf.push_str(&format!("4 0 obj\n<< {font_dictionary}{to_unicode_reference} >>\nendobj\n"));
    pdf.push_str(&format!("5 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n", content.len()));
    if let Some(cmap) = to_unicode {
        pdf.push_str(&format!("6 0 obj\n<< /Length {} >>\nstream\n{cmap}\nendstream\nendobj\n", cmap.len()));
    }
    pdf.push_str("trailer\n<< /Root 1 0 R >>\n%%EOF\n");
    pdf.into_bytes()
}

fn contains_control_character(texts: &[String]) -> bool {
    texts.iter().any(|text| text.chars().any(char::is_control))
}

#[test]
fn glued_font_resource_uses_tounicode_cmap() {
    let cmap = "/CIDInit /ProcSet findresource begin\n\
                begincmap\n\
                2 beginbfchar\n\
                <0003> <0020>\n\
                <002B> <0048>\n\
                endbfchar\n\
                1 beginbfrange\n\
                <0013> <001D> <0030>\n\
                endbfrange\n\
                endcmap\n\
                end\n";
    let pdf = single_page_font_pdf(
        "/Type /Font /Subtype /Type0 /BaseFont /Test /Encoding /Identity-H",
        "/Font<</F1 4 0 R>>",
        Some(cmap),
        "BT /F1 12 Tf 20 800 Td <001300140003002B> Tj ET",
    );
    let graph = import_pdf_bytes("glued-font", &pdf).expect("pdf import");
    let texts = paragraph_texts(&graph);

    assert_eq!(texts, ["01 H"], "the glued `/F1` reference must resolve the CMap");
    assert!(!contains_control_character(&texts), "no raw control character may reach the graph");
}

#[test]
fn unresolved_font_fails_closed_with_diagnostic() {
    let pdf = single_page_font_pdf(
        "/Type /Font /Subtype /Type0 /BaseFont /Test /Encoding /Identity-H",
        "/Font << /F1 4 0 R >>",
        None,
        "BT /F1 12 Tf 20 800 Td <00130014> Tj ET",
    );
    let graph = import_pdf_bytes("unresolved-font", &pdf).expect("pdf import");
    let texts = paragraph_texts(&graph);

    assert!(!contains_control_character(&texts), "fallback bytes must not surface as control characters");
    assert!(
        graph.coverage.loss.iter().any(|loss| loss.code == "import.pdf.font_encoding_unresolved"),
        "an unresolved font must be reported"
    );
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
