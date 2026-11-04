//! PDF tests for the bundled body fonts: embedded programs, style faces, and
//! the monospaced face that replaces base-14 `Courier`.

use document_compose::math::FontStyle;
use document_presentation::display::{DisplayItem, DisplayPage, DisplayPresentation, DisplayTextRun};
use document_renderer_pdf::{PdfFontPolicy, render_pdf, render_pdf_with_font_policy};

fn text(family: &str, style: FontStyle, value: &str) -> DisplayItem {
    DisplayItem::TextRun(DisplayTextRun {
        x: 72.0,
        y: 100.0,
        size: 12.0,
        width: value.chars().count() as f64 * 12.0,
        font_family: family.to_string(),
        style,
        text: value.to_string(),
        href: None,
        title: None,
    })
}

fn render(items: Vec<DisplayItem>) -> String {
    let presentation = DisplayPresentation { pages: vec![DisplayPage { width: 595.0, height: 842.0, items }] };
    render_pdf(&presentation).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).expect("pdf render")
}

/// Every distinct `/BaseFont` name in the file, in object order.
///
/// A subset names itself twice: once on the Type0 parent and once on the
/// CIDFontType2 descendant.
fn base_fonts(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut cursor = 0;
    let needle = "/BaseFont /";
    while let Some(found) = text[cursor..].find(needle) {
        let start = cursor + found + needle.len();
        let name: String = text[start..].chars().take_while(|ch| !ch.is_whitespace() && *ch != '/').collect();
        cursor = start + name.len();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

#[test]
fn body_text_embeds_a_type0_font_program() {
    let bytes = render(vec![text("serif", FontStyle::Normal, "Embedded body text")]);

    assert!(!bytes.contains("/BaseFont /Times-Roman"), "body text must not fall back to the base-14 face");
    assert!(bytes.contains("/Type0"), "text uses a Type0 composite font");
    assert!(bytes.contains("/CIDFontType2"), "the descendant is a CIDFontType2");
    assert!(bytes.contains("/Identity-H"), "the encoding stays Identity-H for two-byte CIDs");
    assert!(bytes.contains("/FontFile2"), "the subset reaches the file as a FontFile2 stream");
    assert!(bytes.contains("/ToUnicode"), "a ToUnicode CMap keeps extraction real Unicode");
    assert!(bytes.contains("startxref"), "the xref table survives the added objects");

    let fonts = base_fonts(&bytes);
    assert_eq!(fonts.len(), 1, "Latin-only text needs one program: {fonts:?}");
    let latin = &fonts[0];
    assert_eq!(latin.len(), "AAAAAA+Tinos-Regular".len(), "every name is a tagged subset");
    assert!(latin.ends_with("+Tinos-Regular"), "the embedded family is the bundled Latin face, not a placeholder: {latin}");
}

#[test]
fn cjk_and_latin_split_across_two_programs() {
    let bytes = render(vec![text("serif", FontStyle::Normal, "正文 Mixed body 文本")]);

    let fonts = base_fonts(&bytes);
    assert_eq!(fonts.len(), 2, "Han and Latin come from separate bundled faces: {fonts:?}");
    assert!(fonts.iter().any(|name| name.ends_with("+Tinos-Regular")), "Latin face present: {fonts:?}");
    assert!(fonts.iter().any(|name| name.ends_with("+NotoSerifSC-Regular")), "CJK face present: {fonts:?}");
    assert_eq!(bytes.matches("/ToUnicode").count(), 2, "one CMap per subset");
}

#[test]
fn bold_runs_reach_the_bold_face() {
    let bytes = render(vec![
        text("serif", FontStyle::Normal, "Regular weight"),
        text("serif", FontStyle::Bold, "Bold weight"),
    ]);

    let fonts = base_fonts(&bytes);
    assert!(fonts.iter().any(|name| name.ends_with("+Tinos-Bold")), "bold Latin face present: {fonts:?}");
    assert!(fonts.iter().any(|name| name.ends_with("+Tinos-Regular")), "regular Latin face present: {fonts:?}");
}

#[test]
fn monospaced_runs_use_the_bundled_mono_face() {
    let bytes = render(vec![text("monospace", FontStyle::Normal, "fn main() {}")]);

    let fonts = base_fonts(&bytes);
    assert_eq!(fonts.len(), 1, "monospace text needs one program: {fonts:?}");
    assert!(fonts[0].ends_with("+NotoSansMono-Regular"), "code keeps its fixed advance from a bundled face: {fonts:?}");
    assert!(bytes.contains("/FontFile2"), "the mono program is embedded, so a reader cannot substitute Courier");
}

#[test]
fn monospaced_runs_fall_back_to_the_prose_face_for_han_characters() {
    let bytes = render(vec![text("monospace", FontStyle::Normal, "注释 // comment")]);

    let fonts = base_fonts(&bytes);
    assert_eq!(fonts.len(), 2, "no mono CJK face exists, so Han text borrows the prose face: {fonts:?}");
    assert!(fonts.iter().any(|name| name.ends_with("+NotoSansMono-Regular")), "Latin part stays monospaced: {fonts:?}");
    assert!(fonts.iter().any(|name| name.ends_with("+NotoSerifSC-Regular")), "CJK part uses the prose face: {fonts:?}");
}

#[test]
fn kerning_pairs_switch_the_run_to_tj() {
    let bytes = render(vec![text("serif", FontStyle::Normal, "AV T")]);

    assert!(bytes.contains("] TJ"), "a kerned run paints through `TJ`: {bytes}");
    // `A` + `V` is -264 of 2048 units, so the next glyph moves 128.906
    // thousandths of an em left.
    assert!(bytes.contains("128.906"), "the `A`+`V` adjustment reaches the content stream");

    let plain = render(vec![text("serif", FontStyle::Normal, "qq")]);
    assert!(!plain.contains(" TJ"), "an unpaired run keeps the plain `Tj` form");
}

#[test]
fn no_base14_face_reaches_a_bundled_document() {
    let bytes = render(vec![
        text("serif", FontStyle::Normal, "Body"),
        text("monospace", FontStyle::Normal, "code()"),
    ]);

    for base in ["/BaseFont /Times-Roman", "/BaseFont /Helvetica", "/BaseFont /Courier"] {
        assert!(!bytes.contains(base), "a bundled document must not reference {base}");
    }
}

#[test]
fn explicit_policy_still_bypasses_the_bundle() {
    let presentation =
        DisplayPresentation { pages: vec![DisplayPage { width: 595.0, height: 842.0, items: vec![text("serif", FontStyle::Normal, "Plain")] }] };
    let bytes = render_pdf_with_font_policy(&presentation, &PdfFontPolicy::Base14Only).expect("pdf render");
    let text = String::from_utf8_lossy(&bytes);

    assert!(text.contains("/BaseFont /Times-Roman"), "an explicit policy overrides the bundled default");
    assert!(!text.contains("/FontFile2"), "the override embeds nothing");
}
