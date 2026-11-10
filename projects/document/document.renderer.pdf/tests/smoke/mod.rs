//! PDF smoke: the writer emits a parseable byte stream.

use document_presentation::display::{DisplayAnchor, DisplayItem, DisplayPage, DisplayPresentation, DisplayTextRun};
use document_renderer_pdf::{PdfFontPolicy, render_pdf_with_font_policy, render_pdf_with_font_policy_and_diagnostics};

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

/// Lay one `glyf`-free sfnt whose `cmap` and `hmtx` cover `advances`, with the
/// PostScript name `TestSerif-Regular`, so a provided named face embeds under a
/// `/BaseFont` the host can recognize instead of a placeholder.
fn named_face_font(advances: &[(char, u16)]) -> Vec<u8> {
    let num_glyphs = advances.len() as u16 + 1;

    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    head[18..20].copy_from_slice(&2048u16.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[4..6].copy_from_slice(&2048i16.to_be_bytes());
    hhea[6..8].copy_from_slice(&(-528i16).to_be_bytes());
    hhea[34..36].copy_from_slice(&num_glyphs.to_be_bytes());

    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&num_glyphs.to_be_bytes());

    let mut hmtx = Vec::new();
    hmtx.extend_from_slice(&0u16.to_be_bytes());
    hmtx.extend_from_slice(&0i16.to_be_bytes());
    for (_, advance) in advances {
        hmtx.extend_from_slice(&advance.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }

    let mut loca = Vec::new();
    for _ in 0..=num_glyphs {
        loca.extend_from_slice(&0u16.to_be_bytes());
    }

    let mut entries: Vec<(u16, u16)> =
        advances.iter().enumerate().map(|(index, (ch, _))| (*ch as u16, index as u16 + 1)).collect();
    entries.sort_by_key(|(code, _)| *code);

    assemble_sfnt(&[
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"maxp", maxp),
        (*b"hmtx", hmtx),
        (*b"cmap", cmap_table(&entries)),
        (*b"loca", loca),
        (*b"glyf", Vec::new()),
        (*b"name", postscript_name_table("TestSerif-Regular")),
    ])
}

/// Lay tables out into one sfnt with a table directory.
fn assemble_sfnt(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let count = tables.len() as u16;
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&(count * 16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + tables.len() * 16 + body.len();
    }
    out.extend(body);
    out
}

/// One format 4 `cmap` mapping each code point to its glyph id.
fn cmap_table(entries: &[(u16, u16)]) -> Vec<u8> {
    let segments = entries.len() + 1;
    let mut subtable = Vec::new();
    subtable.extend_from_slice(&4u16.to_be_bytes());
    subtable.extend_from_slice(&((16 + segments * 8) as u16).to_be_bytes());
    subtable.extend_from_slice(&0u16.to_be_bytes());
    subtable.extend_from_slice(&((segments * 2) as u16).to_be_bytes());
    let mut power = 1usize;
    let mut log = 0usize;
    while power * 2 <= segments {
        power *= 2;
        log += 1;
    }
    subtable.extend_from_slice(&((power * 2) as u16).to_be_bytes());
    subtable.extend_from_slice(&(log as u16).to_be_bytes());
    subtable.extend_from_slice(&(((segments - power) * 2) as u16).to_be_bytes());
    for (code, _) in entries {
        subtable.extend_from_slice(&code.to_be_bytes());
    }
    subtable.extend_from_slice(&0xFFFFu16.to_be_bytes());
    subtable.extend_from_slice(&0u16.to_be_bytes());
    for (code, _) in entries {
        subtable.extend_from_slice(&code.to_be_bytes());
    }
    subtable.extend_from_slice(&0xFFFFu16.to_be_bytes());
    for (code, glyph) in entries {
        let delta = i32::from(*glyph) - i32::from(*code);
        subtable.extend_from_slice(&(delta as i16).to_be_bytes());
    }
    subtable.extend_from_slice(&1i16.to_be_bytes());
    for _ in 0..segments {
        subtable.extend_from_slice(&0u16.to_be_bytes());
    }

    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&12u32.to_be_bytes());
    out.extend(subtable);
    out
}

/// One `name` table whose only record is the Windows Unicode PostScript name.
fn postscript_name_table(name: &str) -> Vec<u8> {
    let utf16: Vec<u8> = name.encode_utf16().flat_map(|unit| unit.to_be_bytes()).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&18u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0x0409u16.to_be_bytes());
    out.extend_from_slice(&6u16.to_be_bytes());
    out.extend_from_slice(&(utf16.len() as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&utf16);
    out
}

#[test]
fn provided_named_family_embeds_under_its_registered_face() {
    let text = "Hamburgefons";
    let advances: Vec<(char, u16)> = text.chars().map(|ch| (ch, 1000u16)).collect();
    let face = document_fonts::BodyFace::new(
        document_fonts::BodyFamily::Named("Test Serif"),
        document_fonts::BodyScript::Latin,
        document_fonts::BodyFaceStyle::Regular,
        named_face_font(&advances),
    );
    let set = std::sync::Arc::new(document_fonts::BodyFaceSet::new(vec![face]).expect("one face"));
    let presentation = DisplayPresentation {
        pages: vec![DisplayPage {
            width: 595.0,
            height: 842.0,
            items: vec![DisplayItem::TextRun(DisplayTextRun {
                x: 72.0,
                y: 100.0,
                size: 12.0,
                width: 120.0,
                font_family: "Test Serif".to_string(),
                style: document_compose::math::FontStyle::Normal,
                text: text.to_string(),
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

    let outcome = render_pdf_with_font_policy_and_diagnostics(&presentation, &PdfFontPolicy::Provided(set))
        .expect("the provided named face serves the page");
    let pdf = String::from_utf8_lossy(&outcome.pdf);
    assert!(pdf.contains("+TestSerif-Regular"), "the subset names the registered face");
    assert!(!pdf.contains("Times"), "a named family must not fall to the base-14 Times path");
    assert!(outcome.diagnostics.is_empty(), "the chain covers every character: {:?}", outcome.diagnostics);
}
