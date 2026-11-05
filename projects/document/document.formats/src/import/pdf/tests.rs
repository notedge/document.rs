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

/// Build a one-page PDF with extra indirect objects for a descendant font or an
/// encoding CMap. Object ids 10 and above are reserved for `extra`.
fn width_font_pdf(font_dictionary: &str, to_unicode: Option<&str>, extra: &[(u32, String)], content: &str) -> Vec<u8> {
    let to_unicode_reference = if to_unicode.is_some() { " /ToUnicode 6 0 R" } else { "" };
    let mut pdf = String::from("%PDF-1.4\n");
    pdf.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    pdf.push_str("2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    pdf.push_str(&format!(
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>\nendobj\n"
    ));
    pdf.push_str(&format!("4 0 obj\n<< {font_dictionary}{to_unicode_reference} >>\nendobj\n"));
    pdf.push_str(&format!("5 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n", content.len()));
    if let Some(cmap) = to_unicode {
        pdf.push_str(&format!("6 0 obj\n<< /Length {} >>\nstream\n{cmap}\nendstream\nendobj\n", cmap.len()));
    }
    for (id, body) in extra {
        pdf.push_str(&format!("{id} 0 obj\n{body}\nendobj\n"));
    }
    pdf.push_str("trailer\n<< /Root 1 0 R >>\n%%EOF\n");
    pdf.into_bytes()
}

fn contains_control_character(texts: &[String]) -> bool {
    texts.iter().any(|text| text.chars().any(char::is_control))
}

/// Build a one-page PDF whose only font dictionary lives in a compressed
/// `/ObjStm` object stream.
///
/// Writers such as Chromium pack font and page dictionaries into object
/// streams, so a reader that scans only top-level `N 0 obj` blocks cannot see
/// the font and drops every glyph drawn with it.
fn objstm_font_pdf(stream: &[u8], object_count: u32, first: u32) -> Vec<u8> {
    let content = "BT /F1 12 Tf 20 800 Td (Object stream text) Tj ET";
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n");
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    pdf.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
    );
    pdf.extend_from_slice(
        format!("4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n", content.len()).as_bytes(),
    );
    pdf.extend_from_slice(
        format!(
            "6 0 obj\n<< /Type /ObjStm /N {object_count} /First {first} /Filter /FlateDecode /Length {} >>\nstream\n",
            stream.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(stream);
    pdf.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n");
    pdf
}

fn compressed_font_object_stream() -> (Vec<u8>, u32) {
    let header = "5 0 ";
    let body = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";
    let decoded = format!("{header}{body}");
    let compressed = acorn_codec::compression::zlib::Zlib::encode(
        decoded.as_bytes(),
        acorn_codec::compression::deflate::DeflateLevel::default(),
    );
    (compressed, header.len() as u32)
}

#[test]
fn a_cm_transform_maps_page_setup_to_page_units() {
    let font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding";
    let pdf = single_page_font_pdf(
        font,
        "/Font << /F1 4 0 R >>",
        None,
        "q 0.5 0 0 0.5 20 400 cm BT /F1 12 Tf 38 800 Td (Body text in a scaled page.) Tj 0 -24 Td (Second body line.) Tj ET Q",
    );
    let graph = import_pdf_bytes("scaled-page", &pdf).expect("pdf import");
    let page = graph.page_setup.expect("page setup");

    assert_eq!(page.base_font_size, Some(6.0), "the transform scales the font size");
    assert_eq!(page.margin_left, Some(39.0), "the transform maps text x to page x");
}

#[test]
fn line_moves_build_one_paragraph_text() {
    let font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding";
    let pdf = single_page_font_pdf(
        font,
        "/Font << /F1 4 0 R >>",
        None,
        "BT /F1 12 Tf 20 800 Td (first line) Tj 0 -16 Td (second line) Tj ET",
    );
    let graph = import_pdf_bytes("line-break", &pdf).expect("pdf import");
    let texts = paragraph_texts(&graph);

    assert_eq!(texts, ["first line second line"], "adjacent lines of one size belong to one paragraph");
    assert_eq!(
        texts.iter().map(|text| text.chars().filter(|character| !character.is_whitespace()).count()).sum::<usize>(),
        19,
        "line-break reconstruction must not change the visible characters"
    );
}

#[test]
fn a_wider_baseline_gap_starts_a_new_paragraph() {
    let font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding";
    let pdf = single_page_font_pdf(
        font,
        "/Font << /F1 4 0 R >>",
        None,
        "BT /F1 10 Tf 20 800 Td (one) Tj 0 -14 Td (two) Tj 0 -46 Td (three) Tj ET",
    );
    let graph = import_pdf_bytes("paragraph-gap", &pdf).expect("pdf import");

    assert_eq!(paragraph_texts(&graph), ["one two", "three"], "a gap past the leading splits paragraphs");
}

#[test]
fn a_larger_font_size_becomes_a_heading_level() {
    let font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding";
    let pdf = single_page_font_pdf(
        font,
        "/Font << /F1 4 0 R >>",
        None,
        "BT /F1 20 Tf 20 820 Td (Report title) Tj /F1 10 Tf 20 780 Td (Body text carries the body font size for the whole document.) Tj ET",
    );
    let graph = import_pdf_bytes("heading-size", &pdf).expect("pdf import");

    assert!(
        graph.blocks.iter().any(|node| matches!(&node.block, document_ir::Block::Section { level: 1, .. })),
        "the largest font size becomes level one"
    );
    assert!(
        graph.coverage.loss.iter().any(|loss| loss.code == "import.pdf.heading_level_inferred"),
        "an inferred heading level must be reported"
    );
}

#[test]
fn bullet_lines_become_a_list() {
    let font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding";
    let pdf = single_page_font_pdf(
        font,
        "/Font << /F1 4 0 R >>",
        None,
        "BT /F1 10 Tf 20 800 Td (- Alpha) Tj 0 -14 Td (- Beta) Tj ET",
    );
    let graph = import_pdf_bytes("bullet-list", &pdf).expect("pdf import");

    let items = graph.blocks.iter().find_map(|node| match &node.block {
        document_ir::Block::List { ordered: false, items } => Some(items.len()),
        _ => None,
    });
    assert_eq!(items, Some(2), "two bullet lines become two list items");
    assert!(
        graph.coverage.loss.iter().any(|loss| loss.code == "import.pdf.list_structure_inferred"),
        "an inferred list must be reported"
    );
}

#[test]
fn column_gaps_become_a_table() {
    let font = "/Type /Font /Subtype /Type1 /BaseFont /Test /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 70 /Widths [500 500 500 500 500 500]";
    let pdf = width_font_pdf(
        font,
        None,
        &[],
        "BT /F1 10 Tf 1 0 0 1 20 800 Tm (A) Tj 1 0 0 1 100 800 Tm (B) Tj 1 0 0 1 20 786 Tm (C) Tj 1 0 0 1 100 786 Tm (D) Tj ET",
    );
    let graph = import_pdf_bytes("table-gap", &pdf).expect("pdf import");

    let table = graph.blocks.iter().find_map(|node| match &node.block {
        document_ir::Block::Table { rows, .. } => Some(rows.len()),
        _ => None,
    });
    assert_eq!(table, Some(2), "two rows of two cells become a table");
    assert!(
        graph.coverage.loss.iter().any(|loss| loss.code == "import.pdf.table_structure_inferred"),
        "an inferred table must be reported"
    );
}

#[test]
fn small_baseline_shift_stays_on_the_same_line() {
    let font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding";
    let pdf = single_page_font_pdf(font, "/Font << /F1 4 0 R >>", None, "BT /F1 12 Tf 20 800 Td (x) Tj 0 5 Td (2) Tj ET");
    let graph = import_pdf_bytes("superscript", &pdf).expect("pdf import");

    assert_eq!(paragraph_texts(&graph), ["x2"], "a superscript shift must not split the line");
}

#[test]
fn tj_word_gap_becomes_a_space() {
    let font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding";
    let pdf = single_page_font_pdf(font, "/Font << /F1 4 0 R >>", None, "BT /F1 10 Tf 20 800 Td [(alpha)-250(beta)] TJ ET");
    let graph = import_pdf_bytes("tj-gap", &pdf).expect("pdf import");

    assert_eq!(paragraph_texts(&graph), ["alpha beta"], "a `TJ` gap wider than 0.2 em is a word gap");
}

#[test]
fn font_inside_object_stream_is_scanned() {
    let (stream, first) = compressed_font_object_stream();
    let pdf = objstm_font_pdf(&stream, 1, first);
    let graph = import_pdf_bytes("objstm-font", &pdf).expect("pdf import");
    let texts = paragraph_texts(&graph);

    assert_eq!(texts, ["Object stream text"], "the font dictionary inside `/ObjStm` must be reachable");
    assert!(
        !graph.coverage.loss.iter().any(|loss| loss.code == "import.pdf.font_encoding_unresolved"),
        "a font found in an object stream must not be reported as unresolved"
    );
}

#[test]
fn unreadable_object_stream_fails_closed_with_diagnostic() {
    let pdf = objstm_font_pdf(b"not a zlib stream", 1, 4);
    let graph = import_pdf_bytes("objstm-corrupt", &pdf).expect("pdf import");
    let texts = paragraph_texts(&graph);

    assert!(texts.is_empty(), "an unexpanded object stream must not yield undeclared text");
    assert!(
        graph.coverage.loss.iter().any(|loss| loss.code == "import.pdf.object_stream_unreadable"),
        "an object stream that cannot be expanded must be reported"
    );
    assert!(
        graph.coverage.loss.iter().any(|loss| loss.code == "import.pdf.no_extractable_text"),
        "the importer must still fail closed when the fonts are unreachable"
    );
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
fn simple_font_widths_build_a_word_gap() {
    let font =
        "/Type /Font /Subtype /Type1 /BaseFont /Test /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 66 /Widths [500 500]";
    let pdf = width_font_pdf(font, None, &[], "BT /F1 10 Tf 1 0 0 1 20 800 Tm (A) Tj 1 0 0 1 60 800 Tm (B) Tj ET");
    let graph = import_pdf_bytes("simple-width-gap", &pdf).expect("pdf import");

    assert_eq!(paragraph_texts(&graph), ["A B"], "a gap wider than 0.2 em past the declared advance is a word gap");
}

#[test]
fn simple_font_widths_ignore_a_subthreshold_gap() {
    let font =
        "/Type /Font /Subtype /Type1 /BaseFont /Test /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 66 /Widths [500 500]";
    let pdf = width_font_pdf(font, None, &[], "BT /F1 10 Tf 1 0 0 1 20 800 Tm (A) Tj 1 0 0 1 26 800 Tm (B) Tj ET");
    let graph = import_pdf_bytes("simple-width-kerning", &pdf).expect("pdf import");

    assert_eq!(paragraph_texts(&graph), ["AB"], "a gap below 0.2 em is kerning, not a word gap");
}

#[test]
fn type0_w_array_builds_a_word_gap() {
    let descendant = "/Type /Font /Subtype /CIDFontType0 /BaseFont /Test /DW 1000 /W [1 [500] 2 [500]]".to_string();
    let cmap = "2 beginbfchar\n<0001> <0041>\n<0002> <0042>\nendbfchar\n";
    let pdf = width_font_pdf(
        "/Type /Font /Subtype /Type0 /BaseFont /Test /Encoding /Identity-H /DescendantFonts [10 0 R]",
        Some(cmap),
        &[(10, descendant)],
        "BT /F1 10 Tf 1 0 0 1 20 800 Tm <0001> Tj 1 0 0 1 60 800 Tm <0002> Tj ET",
    );
    let graph = import_pdf_bytes("type0-width-gap", &pdf).expect("pdf import");

    assert_eq!(paragraph_texts(&graph), ["A B"], "the `/W` array supplies composite glyph advances");
}

#[test]
fn encoding_cmap_widths_build_a_word_gap() {
    let cmap = "/CIDInit /ProcSet findresource begin\n\
                begincmap\n\
                1 begincodespacerange\n\
                <0001> <0002>\n\
                endcodespacerange\n\
                1 begincidrange\n\
                <0001> <0002> 1\n\
                endcidrange\n\
                1 2 500 W\n\
                endcmap\n\
                end\n";
    let encoding = format!("<< /Length {} >>\nstream\n{cmap}\nendstream", cmap.len());
    let to_unicode = "2 beginbfchar\n<0001> <0041>\n<0002> <0042>\nendbfchar\n";
    let pdf = width_font_pdf(
        "/Type /Font /Subtype /Type0 /BaseFont /Test /Encoding 11 0 R /DescendantFonts [10 0 R]",
        Some(to_unicode),
        &[(10, "/Type /Font /Subtype /CIDFontType0 /BaseFont /Test".to_string()), (11, encoding)],
        "BT /F1 10 Tf 1 0 0 1 20 800 Tm <0001> Tj 1 0 0 1 60 800 Tm <0002> Tj ET",
    );
    let graph = import_pdf_bytes("cmap-width-gap", &pdf).expect("pdf import");

    assert_eq!(paragraph_texts(&graph), ["A B"], "an embedded encoding CMap must map codes to CIDs and supply `W` widths");
}

#[test]
fn missing_widths_do_not_synthesise_a_space() {
    let cmap = "2 beginbfchar\n<0001> <0041>\n<0002> <0042>\nendbfchar\n";
    let pdf = width_font_pdf(
        "/Type /Font /Subtype /Type0 /BaseFont /Test /Encoding /Identity-H",
        Some(cmap),
        &[],
        "BT /F1 10 Tf 1 0 0 1 20 800 Tm <0001> Tj 1 0 0 1 60 800 Tm <0002> Tj ET",
    );
    let graph = import_pdf_bytes("missing-widths", &pdf).expect("pdf import");

    assert_eq!(
        paragraph_texts(&graph),
        ["AB"],
        "without a declared width the geometric gap must stay unevaluated instead of being estimated"
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
