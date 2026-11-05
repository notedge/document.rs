use std::collections::HashMap;

use super::{
    encoding::{DecodedRun, decode_with_font},
    fonts::ToUnicodeMap,
    images::parse_inline_jpeg,
    syntax::{
        hex_bytes, is_pdf_number_start, literal_close, pdf_delimiter, pdf_operator_at, skip_content_space, skip_pdf_comment,
        skip_pdf_number, skip_space,
    },
    types::{ContentEvent, ExtractedText},
};

/// Separation owed to the next text run because the previous operator moved the
/// text position.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Separation {
    None,
    Space,
    Line,
}

/// A baseline move of seven tenths of an em starts a new line.
///
/// Superscripts and subscripts shift the baseline by a third to two thirds of
/// an em, while real line advances are around one em or more. A measured
/// superscript here sits at 0.62 em, so 0.7 em keeps it on the same line and
/// still catches every line advance in the regression samples.
const LINE_BREAK_EM: f32 = 0.7;

/// A word gap is a fifth of an em, measured either as a `TJ` displacement or as
/// the distance from the previous run's declared end to the next run's start.
///
/// Small negative `TJ` numbers are kerning between adjacent glyphs. The
/// geometric gap is `next_start - (previous_start + advance)`, where `advance`
/// comes from the font width tables (`/Widths`, `/W` or CMap `W`). When either
/// run has no declared width the gap is not evaluated and no space is
/// synthesised, so a font without width data keeps its current output.
const WORD_SPACE_EM: f32 = 0.2;

// Writers place every glyph with its own `Td`, and those advances are glyph
// widths rather than gaps. The geometric check therefore subtracts the
// previous run's declared advance instead of treating a `Td` move as a gap.

pub(crate) fn extract_text_operators(stream: &[u8], font_maps: &HashMap<String, ToUnicodeMap>) -> ExtractedText {
    let mut events = Vec::new();
    let mut spacing_adjustments = false;
    let mut unresolved_fonts = std::collections::BTreeSet::new();
    let mut cursor = 0;
    let mut current_font = None;
    let mut font_size = 0.0f32;
    let mut text_position: Option<(f32, f32)> = None;
    let mut text_block_start = true;
    // End of the previous glyph run on the current baseline, in text-space
    // units. `None` means the run had no declared advance, so the next gap is
    // unknown and must not be turned into a space.
    let mut pen_end: Option<f32> = None;
    // Whether `text_position` was set by a positioning operator since the last
    // text run. Without one, the next run starts where the pen already is.
    let mut positioned = false;
    let mut separation = Separation::None;
    let mut last_char: Option<char> = None;
    while cursor < stream.len() {
        if stream[cursor..].starts_with(b"BI")
            && (cursor == 0 || stream[cursor - 1].is_ascii_whitespace())
            && stream.get(cursor + 2).is_some_and(|byte| byte.is_ascii_whitespace())
        {
            if let Some((image, end)) = parse_inline_jpeg(stream, cursor) {
                events.push(ContentEvent::InlineImage(image));
                cursor = end;
                continue;
            }
        }
        match stream[cursor] {
            b'/' => {
                if let Some((font, size, end)) = text_font_operator(stream, cursor) {
                    current_font = Some(font);
                    font_size = size;
                    cursor = end;
                }
                else if let Some((name, end)) = resource_operator(stream, cursor, "Do") {
                    events.push(ContentEvent::NamedImage(name));
                    cursor = end;
                }
                else {
                    cursor += 1;
                }
            }
            b'(' => {
                let Some(close) = literal_close(stream, cursor + 1)
                else {
                    break;
                };
                let after = skip_content_space(stream, close + 1);
                if pdf_operator_at(stream, after, b"Tj") {
                    let decoded = decode_with_font(&stream[cursor + 1..close], current_font.as_deref(), font_maps, true);
                    let start = run_start(positioned, text_position, pen_end);
                    note_geometric_gap(start, pen_end, font_size, &mut separation);
                    let advance = push_decoded_text(
                        &mut events,
                        &mut unresolved_fonts,
                        current_font.as_deref(),
                        decoded,
                        Separation::None,
                        &mut separation,
                        &mut last_char,
                    );
                    pen_end = pen_after_run(start, advance, font_size);
                    positioned = false;
                }
                else if pdf_operator_at(stream, after, b"'") || pdf_operator_at(stream, after, b"\"") {
                    let decoded = decode_with_font(&stream[cursor + 1..close], current_font.as_deref(), font_maps, true);
                    push_decoded_text(
                        &mut events,
                        &mut unresolved_fonts,
                        current_font.as_deref(),
                        decoded,
                        Separation::Line,
                        &mut separation,
                        &mut last_char,
                    );
                    text_position = None;
                    pen_end = None;
                    positioned = false;
                }
                cursor = close + 1;
            }
            b'<' if stream.get(cursor + 1) != Some(&b'<') => {
                let Some(close) =
                    stream[cursor + 1..].iter().position(|byte| *byte == b'>').map(|position| cursor + 1 + position)
                else {
                    break;
                };
                let after = skip_content_space(stream, close + 1);
                let text = &stream[cursor + 1..close];
                if pdf_operator_at(stream, after, b"Tj") {
                    let decoded = decode_with_font(&hex_bytes(text), current_font.as_deref(), font_maps, false);
                    let start = run_start(positioned, text_position, pen_end);
                    note_geometric_gap(start, pen_end, font_size, &mut separation);
                    let advance = push_decoded_text(
                        &mut events,
                        &mut unresolved_fonts,
                        current_font.as_deref(),
                        decoded,
                        Separation::None,
                        &mut separation,
                        &mut last_char,
                    );
                    pen_end = pen_after_run(start, advance, font_size);
                    positioned = false;
                }
                else if pdf_operator_at(stream, after, b"'") || pdf_operator_at(stream, after, b"\"") {
                    let decoded = decode_with_font(&hex_bytes(text), current_font.as_deref(), font_maps, false);
                    push_decoded_text(
                        &mut events,
                        &mut unresolved_fonts,
                        current_font.as_deref(),
                        decoded,
                        Separation::Line,
                        &mut separation,
                        &mut last_char,
                    );
                    text_position = None;
                    pen_end = None;
                    positioned = false;
                }
                cursor = close + 1;
            }
            b'[' => {
                let mut position = cursor + 1;
                let mut run = String::new();
                let mut advance = Some(0.0f32);
                while position < stream.len() && stream[position] != b']' {
                    if stream[position] == b'(' {
                        let Some(end) = literal_close(stream, position + 1)
                        else {
                            break;
                        };
                        push_decoded_run(
                            &mut run,
                            &mut advance,
                            &mut unresolved_fonts,
                            current_font.as_deref(),
                            decode_with_font(&stream[position + 1..end], current_font.as_deref(), font_maps, true),
                        );
                        position = end + 1;
                    }
                    else if stream[position] == b'<' && stream.get(position + 1) != Some(&b'<') {
                        let Some(relative) = stream[position + 1..].iter().position(|byte| *byte == b'>')
                        else {
                            break;
                        };
                        let end = position + 1 + relative;
                        push_decoded_run(
                            &mut run,
                            &mut advance,
                            &mut unresolved_fonts,
                            current_font.as_deref(),
                            decode_with_font(&hex_bytes(&stream[position + 1..end]), current_font.as_deref(), font_maps, false),
                        );
                        position = end + 1;
                    }
                    else if stream[position] == b'%' {
                        position = skip_pdf_comment(stream, position);
                    }
                    else if is_pdf_number_start(stream[position]) {
                        spacing_adjustments = true;
                        let start = position;
                        position = skip_pdf_number(stream, position);
                        if let Ok(value) = std::str::from_utf8(&stream[start..position]).unwrap_or("").parse::<f32>() {
                            // A negative `TJ` number moves the pen right, so the
                            // same signed displacement extends the run advance.
                            advance = advance.map(|total| total + -value / 1000.0);
                            if font_size > 0.0 {
                                // Only a displacement wider than the threshold
                                // is a word gap, smaller ones are kerning.
                                let gap = -value / 1000.0 * font_size;
                                if gap >= WORD_SPACE_EM * font_size {
                                    if run.is_empty() {
                                        separation = separation.max(Separation::Space);
                                    }
                                    else if !run.ends_with(|character: char| character.is_whitespace()) {
                                        run.push(' ');
                                    }
                                }
                            }
                        }
                    }
                    else {
                        position += 1;
                    }
                }
                if position < stream.len() && pdf_operator_at(stream, skip_content_space(stream, position + 1), b"TJ") {
                    let start = run_start(positioned, text_position, pen_end);
                    note_geometric_gap(start, pen_end, font_size, &mut separation);
                    push_text_event(&mut events, run, &mut separation, &mut last_char);
                    pen_end = pen_after_run(start, advance, font_size);
                    positioned = false;
                }
                cursor = position.saturating_add(1);
            }
            b'B' if (cursor == 0 || pdf_delimiter(stream[cursor - 1])) && pdf_operator_at(stream, cursor, b"BT") => {
                text_block_start = true;
                // The text matrix restarts, but writers split one visual line
                // into several `BT`/`ET` blocks. The pen end therefore survives
                // the block boundary, and only a line move suppresses the gap.
                positioned = false;
                cursor += 2;
            }
            b'T' if (cursor == 0 || pdf_delimiter(stream[cursor - 1])) && pdf_operator_at(stream, cursor, b"T*") => {
                separation = Separation::Line;
                text_position = None;
                pen_end = None;
                positioned = false;
                text_block_start = false;
                cursor += 2;
            }
            _ if is_pdf_number_start(stream[cursor]) => {
                if let Some((values, operator, end)) = text_position_operator(stream, cursor) {
                    match operator {
                        TextPosition::Matrix if values.len() >= 6 => {
                            note_text_position(&mut text_position, (values[4], values[5]), font_size, &mut separation);
                            text_block_start = false;
                            positioned = true;
                        }
                        TextPosition::Translate if values.len() >= 2 => {
                            // `Td` and `TD` are relative to the text line matrix.
                            // The matrix starts at the origin for the first `Td`
                            // of a text block, afterwards each move accumulates.
                            let (tx, ty) = (values[0], values[1]);
                            let next = if text_block_start {
                                (tx, ty)
                            }
                            else {
                                let (x, y) = text_position.unwrap_or((0.0, 0.0));
                                (x + tx, y + ty)
                            };
                            note_text_position(&mut text_position, next, font_size, &mut separation);
                            text_block_start = false;
                            positioned = true;
                        }
                        _ => {}
                    }
                    cursor = end;
                }
                else {
                    cursor = skip_pdf_number(stream, cursor);
                }
            }
            b'%' => cursor = skip_pdf_comment(stream, cursor),
            _ => cursor += 1,
        }
    }
    ExtractedText { events, spacing_adjustments, unresolved_fonts }
}

/// Where the next run starts on the current baseline.
///
/// A positioning operator sets the start from the text matrix. Without one the
/// pen already sits at the previous run's end, so the gap is zero.
///
/// A word gap is measured inside one fragment: the previous run's advance and
/// the next run's start come from the same line, and a line move wins over a
/// space. Font switches are allowed because math and table fragments change
/// fonts between adjacent words.
fn run_start(positioned: bool, text_position: Option<(f32, f32)>, pen_end: Option<f32>) -> Option<f32> {
    if positioned { text_position.map(|(x, _)| x) } else { pen_end }
}

/// Turn a geometric gap between glyph runs into a word space.
///
/// The gap is `next_start - previous_end`, with both ends measured in text-space
/// units and compared against `WORD_SPACE_EM × font_size`. A line break wins
/// over a space, and an unknown width on either side leaves the gap unevaluated.
fn note_geometric_gap(start: Option<f32>, pen_end: Option<f32>, font_size: f32, separation: &mut Separation) {
    if *separation == Separation::Line || font_size <= 0.0 {
        return;
    }
    let (Some(start), Some(end)) = (start, pen_end)
    else {
        return;
    };
    if start - end >= WORD_SPACE_EM * font_size {
        *separation = (*separation).max(Separation::Space);
    }
}



/// Pen end after a run, or `None` when its start or advance is unknown.
fn pen_after_run(start: Option<f32>, advance: Option<f32>, font_size: f32) -> Option<f32> {
    match (start, advance) {
        (Some(start), Some(advance)) if font_size > 0.0 => Some(start + advance * font_size),
        _ => None,
    }
}

/// Emit one decoded run and return its advance in em units.
///
/// `None` means either no text was emitted or the font has no usable encoding.
/// A decoded run with an unknown advance also returns `None`, which makes the
/// caller clear the pen end and stop geometric gap detection.
fn push_decoded_text(
    events: &mut Vec<ContentEvent>,
    unresolved_fonts: &mut std::collections::BTreeSet<String>,
    font: Option<&str>,
    decoded: Option<DecodedRun>,
    owed: Separation,
    separation: &mut Separation,
    last_char: &mut Option<char>,
) -> Option<f32> {
    match decoded {
        Some(DecodedRun { text, advance }) => {
            *separation = (*separation).max(owed);
            push_text_event(events, text, separation, last_char);
            advance
        }
        None => {
            if let Some(font) = font {
                unresolved_fonts.insert(font.to_owned());
            }
            None
        }
    }
}

fn push_decoded_run(
    run: &mut String,
    advance: &mut Option<f32>,
    unresolved_fonts: &mut std::collections::BTreeSet<String>,
    font: Option<&str>,
    decoded: Option<DecodedRun>,
) {
    match decoded {
        Some(DecodedRun { text, advance: run_advance }) => {
            run.push_str(&text);
            *advance = match (*advance, run_advance) {
                (Some(total), Some(run_advance)) => Some(total + run_advance),
                _ => None,
            };
        }
        None => {
            *advance = None;
            if let Some(font) = font {
                unresolved_fonts.insert(font.to_owned());
            }
        }
    }
}

fn text_font_operator(stream: &[u8], cursor: usize) -> Option<(String, f32, usize)> {
    let name_end =
        stream[cursor + 1..].iter().position(|byte| byte.is_ascii_whitespace()).map(|position| cursor + 1 + position)?;
    let name = std::str::from_utf8(&stream[cursor + 1..name_end]).ok()?.to_owned();
    let number_start = skip_space(stream, name_end);
    let number_end =
        stream[number_start..].iter().position(|byte| byte.is_ascii_whitespace()).map(|position| number_start + position)?;
    let size = std::str::from_utf8(&stream[number_start..number_end]).ok()?.parse().ok()?;
    let operator = skip_space(stream, number_end);
    pdf_operator_at(stream, operator, b"Tf").then_some((name, size, operator + 2))
}

enum TextPosition {
    Translate,
    Matrix,
}

/// Parse a numeric prefix followed by `Td`, `TD` or `Tm`.
fn text_position_operator(stream: &[u8], cursor: usize) -> Option<(Vec<f32>, TextPosition, usize)> {
    let mut values = Vec::new();
    let mut position = cursor;
    while values.len() < 6 {
        let start = skip_space(stream, position);
        if start >= stream.len() || !is_pdf_number_start(stream[start]) {
            break;
        }
        let end = skip_pdf_number(stream, start);
        values.push(std::str::from_utf8(&stream[start..end]).ok()?.parse().ok()?);
        position = end;
    }
    if values.is_empty() {
        return None;
    }
    let operator = skip_content_space(stream, position);
    if pdf_operator_at(stream, operator, b"Tm") {
        Some((values, TextPosition::Matrix, operator + 2))
    }
    else if pdf_operator_at(stream, operator, b"Td") || pdf_operator_at(stream, operator, b"TD") {
        Some((values, TextPosition::Translate, operator + 2))
    }
    else {
        None
    }
}

fn note_text_position(text_position: &mut Option<(f32, f32)>, next: (f32, f32), font_size: f32, separation: &mut Separation) {
    if let Some((_, y)) = *text_position
        && font_size > 0.0
        && (next.1 - y).abs() >= LINE_BREAK_EM * font_size
    {
        *separation = Separation::Line;
    }
    *text_position = Some(next);
}

fn resource_operator(stream: &[u8], cursor: usize, expected: &str) -> Option<(String, usize)> {
    let name_end =
        stream[cursor + 1..].iter().position(|byte| byte.is_ascii_whitespace()).map(|position| cursor + 1 + position)?;
    let name = std::str::from_utf8(&stream[cursor + 1..name_end]).ok()?.to_owned();
    let operator_start = skip_space(stream, name_end);
    let operator_end = operator_start.checked_add(expected.len())?;
    pdf_operator_at(stream, operator_start, expected.as_bytes()).then_some((name, operator_end))
}
fn push_text_event(
    events: &mut Vec<ContentEvent>,
    mut text: String,
    separation: &mut Separation,
    last_char: &mut Option<char>,
) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = *last_char {
        match *separation {
            Separation::Line if last != '\n' => text.insert(0, '\n'),
            Separation::Space if !last.is_whitespace() => text.insert(0, ' '),
            _ => {}
        }
    }
    *separation = Separation::None;
    *last_char = text.chars().last();
    events.push(ContentEvent::Text(text));
}
