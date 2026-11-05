use std::collections::HashMap;

use crate::FormatError;

use super::{
    encoding::win_ansi_encoding,
    objects::{
        body_contains_name, body_reference, dictionary_subtype_is, page_resource_sections, reference_ids,
        resource_font_references,
    },
    streams::decode_stream,
    syntax::{
        dictionary_value, hex_bytes, is_pdf_number_start, pdf_delimiter, skip_pdf_number, skip_space,
        split_pdf_syntax_tokens,
    },
    types::PdfStream,
};

#[derive(Clone, Debug, Default)]
pub(crate) struct ToUnicodeMap {
    entries: HashMap<Vec<u8>, String>,
    max_code_len: usize,
    code_lengths: Vec<usize>,
    winansi_fallback: bool,
    unresolved: bool,
    /// Glyph advances in em units, keyed by glyph id. Simple fonts key by the
    /// single-byte character code, composite fonts key by CID.
    widths: HashMap<u32, f32>,
    /// `/DW` or `/MissingWidth` when the font declares a default explicitly.
    default_width: Option<f32>,
    /// Source code to CID mapping from an embedded encoding CMap. Identity
    /// encodings leave this empty and the code value is the CID.
    cid_map: HashMap<Vec<u8>, u32>,
}

impl ToUnicodeMap {
    /// Decode one text string and sum the glyph advances it covers.
    ///
    /// `None` means the font has no usable encoding information, so the caller
    /// must fail closed instead of emitting raw control characters. The second
    /// value is the advance in em units, and it is `None` when any glyph in the
    /// string has no declared width. Callers must then skip geometric gap
    /// detection instead of estimating a width.
    pub(crate) fn decode_run(&self, bytes: &[u8]) -> Option<(String, Option<f32>)> {
        if self.unresolved || (self.entries.is_empty() && !self.winansi_fallback) {
            return None;
        }
        if self.entries.is_empty() && self.winansi_fallback {
            let advance = bytes.iter().fold(Some(0.0f32), |total, byte| {
                combine_advance(total, self.advance_for(std::slice::from_ref(byte)))
            });
            return Some((bytes.iter().map(|byte| win_ansi_encoding(*byte)).collect(), advance));
        }
        let mut output = String::new();
        let mut advance = Some(0.0f32);
        let mut cursor = 0;
        while cursor < bytes.len() {
            let mut found = None;
            for length in (1..=self.max_code_len.min(bytes.len() - cursor)).rev() {
                if !self.code_lengths.is_empty() && !self.code_lengths.contains(&length) {
                    continue;
                }
                if let Some(value) = self.entries.get(&bytes[cursor..cursor + length]) {
                    found = Some((length, value));
                    break;
                }
            }
            if let Some((length, value)) = found {
                output.push_str(value);
                advance = combine_advance(advance, self.advance_for(&bytes[cursor..cursor + length]));
                cursor += length;
            }
            else {
                output.push('\u{FFFD}');
                advance = combine_advance(advance, self.advance_for(&bytes[cursor..cursor + 1]));
                cursor += 1;
            }
        }
        Some((output, advance))
    }

    /// Advance for one encoded glyph, in em units.
    ///
    /// A missing width stays missing. Declared defaults from `/DW` or
    /// `/MissingWidth` are used only when the font dictionary spells them out.
    fn advance_for(&self, code: &[u8]) -> Option<f32> {
        let value = code.iter().fold(0u32, |value, byte| (value << 8) | u32::from(*byte));
        let cid = self.cid_map.get(code).copied().unwrap_or(value);
        self.widths.get(&cid).copied().or(self.default_width)
    }
}

fn combine_advance(total: Option<f32>, next: Option<f32>) -> Option<f32> {
    match (total, next) {
        (Some(total), Some(next)) => Some(total + next),
        _ => None,
    }
}

pub(crate) fn page_font_maps(
    bodies: &[(u32, Vec<u8>)],
    streams: &[PdfStream<'_>],
) -> Result<HashMap<String, ToUnicodeMap>, FormatError> {
    let mut result = HashMap::new();
    for resources in page_resource_sections(bodies) {
        let Some(font_start) = resources.windows(5).position(|window| window == b"/Font")
        else {
            continue;
        };
        let section = &resources[font_start + 5..];
        for (name, object_id) in resource_font_references(section) {
            let Some((_, font_body)) = bodies.iter().find(|(id, _)| *id == object_id)
            else {
                continue;
            };
            let to_unicode_map = if let Some(to_unicode) = body_reference(font_body, b"/ToUnicode") {
                match streams.iter().find(|stream| stream.object_id == Some(to_unicode)) {
                    Some(stream) => parse_to_unicode_cmap(&decode_stream(stream.dictionary, stream.data)?),
                    None => None,
                }
            }
            else {
                None
            };
            let mut map = to_unicode_map.unwrap_or_else(|| {
                if body_contains_name(font_body, b"/WinAnsiEncoding") {
                    ToUnicodeMap { winansi_fallback: true, ..ToUnicodeMap::default() }
                }
                else {
                    ToUnicodeMap { unresolved: true, ..ToUnicodeMap::default() }
                }
            });
            apply_font_widths(&mut map, font_body, bodies, streams);
            result.insert(name, map);
        }
    }
    Ok(result)
}

/// Attach glyph advance widths to a decoded font map.
///
/// Simple fonts declare `/Widths` indexed from `/FirstChar` plus an optional
/// `/MissingWidth`. Composite fonts declare `/DW` and a `/W` array keyed by CID.
/// An embedded encoding CMap may carry its own `W` widths and `begincidchar` /
/// `begincidrange` mappings. A font that declares no width keeps an empty table,
/// so `decode_run` reports an unknown advance instead of estimating one.
fn apply_font_widths(
    map: &mut ToUnicodeMap,
    font_body: &[u8],
    bodies: &[(u32, Vec<u8>)],
    streams: &[PdfStream<'_>],
) {
    if dictionary_subtype_is(font_body, b"/Type0") {
        if let Some(descendant) = body_reference(font_body, b"/DescendantFonts").and_then(|id| font_object(bodies, id)) {
            if let Some(default) = dictionary_value(descendant, b"/DW") {
                map.default_width = Some(default as f32 / 1000.0);
            }
            if let Some(array) = array_after(descendant, b"/W", bodies) {
                apply_type0_widths(&array, map);
            }
        }
        if let Some(encoding) =
            body_reference(font_body, b"/Encoding").and_then(|id| streams.iter().find(|stream| stream.object_id == Some(id)))
            && let Ok(decoded) = decode_stream(encoding.dictionary, encoding.data)
        {
            apply_cmap_widths(&decoded, map);
        }
    }
    else {
        if let Some(first) = dictionary_value(font_body, b"/FirstChar")
            && let Some(array) = array_after(font_body, b"/Widths", bodies)
        {
            for (offset, width) in array_numbers(&array).into_iter().enumerate() {
                map.widths.insert(first as u32 + offset as u32, width / 1000.0);
            }
        }
        if let Some(descriptor) =
            body_reference(font_body, b"/FontDescriptor").and_then(|id| font_object(bodies, id))
            && let Some(missing) = dictionary_value(descriptor, b"/MissingWidth")
        {
            map.default_width = Some(missing as f32 / 1000.0);
        }
    }
}

fn font_object<'a>(bodies: &'a [(u32, Vec<u8>)], id: u32) -> Option<&'a [u8]> {
    bodies.iter().find(|(object, _)| *object == id).map(|(_, body)| body.as_slice())
}

/// Return the width array that follows `key`, either inline or through a
/// reference to a top-level or object-stream array object.
fn array_after(body: &[u8], key: &[u8], bodies: &[(u32, Vec<u8>)]) -> Option<Vec<u8>> {
    let position = body.windows(key.len()).position(|window| window == key)?;
    let start = skip_space(body, position + key.len());
    if body.get(start) == Some(&b'[') {
        return Some(body[start..].to_vec());
    }
    let id = *reference_ids(&body[start..]).first()?;
    let target = font_object(bodies, id)?;
    let start = target.iter().position(|byte| *byte == b'[')?;
    Some(target[start..].to_vec())
}

fn array_numbers(array: &[u8]) -> Vec<f32> {
    let mut values = Vec::new();
    let mut cursor = array.iter().position(|byte| *byte == b'[').map_or(0, |position| position + 1);
    while cursor < array.len() && array[cursor] != b']' {
        if is_pdf_number_start(array[cursor]) {
            let end = skip_pdf_number(array, cursor);
            if let Some(value) = parse_f32(&array[cursor..end]) {
                values.push(value);
            }
            cursor = end;
        }
        else {
            cursor += 1;
        }
    }
    values
}

/// Apply a composite font `/W` array.
///
/// The array interleaves two forms: `c [w1 w2 ...]` for consecutive CIDs and
/// `cfirst clast w` for a CID range. Widths are in thousandths of an em.
fn apply_type0_widths(array: &[u8], map: &mut ToUnicodeMap) {
    let tokens = split_pdf_syntax_tokens(array).collect::<Vec<_>>();
    let mut index = 0;
    while index < tokens.len() {
        let Some(first) = parse_u32(tokens[index])
        else {
            index += 1;
            continue;
        };
        if token_is(tokens.get(index + 1), b"[") {
            let mut offset = 0u32;
            let mut position = index + 2;
            while position < tokens.len() && !token_is(tokens.get(position), b"]") {
                if let Some(width) = parse_f32(tokens[position]) {
                    map.widths.insert(first.saturating_add(offset), width / 1000.0);
                    offset = offset.saturating_add(1);
                }
                position += 1;
            }
            index = position.saturating_add(1);
            if token_is(tokens.get(index), b"W") {
                index += 1;
            }
        }
        else if let (Some(last), Some(width)) =
            (tokens.get(index + 1).and_then(|token| parse_u32(token)), tokens.get(index + 2).and_then(|token| parse_f32(token)))
        {
            if first <= last && last - first < 65_536 {
                for cid in first..=last {
                    map.widths.insert(cid, width / 1000.0);
                }
            }
            index += 3;
            if token_is(tokens.get(index), b"W") {
                index += 1;
            }
        }
        else {
            index += 1;
        }
    }
}

/// Read `W` widths and `begincidchar` / `begincidrange` mappings from an
/// embedded encoding CMap.
///
/// `W` accepts `c [w1 w2 ...]`, `cfirst clast w` and a single `c w w`. Identity
/// encodings have no CID mapping, so the code value stands in for the CID.
fn apply_cmap_widths(bytes: &[u8], map: &mut ToUnicodeMap) {
    let tokens = cmap_tokens(bytes);
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        if token_is(Some(&token), b"begincidchar") {
            let mut position = index + 1;
            while position + 1 < tokens.len() && !token_is(tokens.get(position), b"endcidchar") {
                if let (Some(code), Some(cid)) = (hex_token(tokens[position]), parse_u32(tokens[position + 1])) {
                    map.cid_map.insert(code, cid);
                }
                position += 2;
            }
            index = position;
        }
        else if token_is(Some(&token), b"begincidrange") {
            let mut position = index + 1;
            while position + 2 < tokens.len() && !token_is(tokens.get(position), b"endcidrange") {
                if let (Some(start), Some(end), Some(cid)) =
                    (hex_token(tokens[position]), hex_token(tokens[position + 1]), parse_u32(tokens[position + 2]))
                {
                    add_cid_range(map, &start, &end, cid);
                }
                position += 3;
            }
            index = position;
        }
        else if let Some(first) = parse_u32(token) {
            if token_is(tokens.get(index + 1), b"[") {
                let mut position = index + 2;
                let mut offset = 0u32;
                while position < tokens.len() && !token_is(tokens.get(position), b"]") {
                    if let Some(width) = parse_f32(tokens[position]) {
                        map.widths.insert(first.saturating_add(offset), width / 1000.0);
                        offset = offset.saturating_add(1);
                    }
                    position += 1;
                }
                let operator = tokens.get(position + 1).copied();
                if operator.is_some_and(|operator| is_token(operator, b"W") || is_token(operator, b"w")) {
                    index = position.saturating_add(2);
                }
                else {
                    index = position.saturating_add(1);
                }
                continue;
            }
            if let (Some(last), Some(width)) =
                (tokens.get(index + 1).and_then(|token| parse_u32(token)), tokens.get(index + 2).and_then(|token| parse_f32(token)))
                && token_is(tokens.get(index + 3), b"W")
            {
                if first <= last && last - first < 65_536 {
                    for cid in first..=last {
                        map.widths.insert(cid, width / 1000.0);
                    }
                }
                index += 4;
                continue;
            }
            if let (Some(width), Some(operator)) = (
                tokens.get(index + 1).and_then(|token| parse_f32(token)),
                tokens.get(index + 2).copied(),
            ) && is_token(operator, b"w")
            {
                map.widths.insert(first, width / 1000.0);
                index += 3;
                continue;
            }
        }
        index += 1;
    }
}

fn add_cid_range(map: &mut ToUnicodeMap, start: &[u8], end: &[u8], cid: u32) {
    if start.is_empty() || start.len() != end.len() || start.len() > 4 {
        return;
    }
    let start_value = start.iter().fold(0u32, |value, byte| (value << 8) | u32::from(*byte));
    let end_value = end.iter().fold(0u32, |value, byte| (value << 8) | u32::from(*byte));
    if end_value < start_value || end_value - start_value >= 65_536 {
        return;
    }
    for offset in 0..=end_value - start_value {
        let mut code = vec![0u8; start.len()];
        for (position, byte) in code.iter_mut().enumerate() {
            *byte = ((start_value + offset) >> (8 * (start.len() - position - 1))) as u8;
        }
        map.cid_map.insert(code, cid.saturating_add(offset));
    }
}

/// Split a CMap into tokens while keeping `<hex>` sequences whole and keeping
/// `[` and `]` as their own tokens even when glued to numbers.
fn cmap_tokens(bytes: &[u8]) -> Vec<&[u8]> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if byte.is_ascii_whitespace() || byte == 0 {
            cursor += 1;
            continue;
        }
        if byte == b'%' {
            cursor = bytes[cursor..]
                .iter()
                .position(|byte| matches!(byte, b'\r' | b'\n'))
                .map_or(bytes.len(), |offset| cursor + offset);
            continue;
        }
        let start = cursor;
        if byte == b'<' {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor] != b'>' {
                cursor += 1;
            }
            if cursor < bytes.len() {
                cursor += 1;
            }
        }
        else if byte == b'[' || byte == b']' || byte == b'/' {
            cursor += 1;
        }
        else {
            while cursor < bytes.len() && !pdf_delimiter(bytes[cursor]) {
                cursor += 1;
            }
        }
        if cursor == start {
            cursor += 1;
        }
        tokens.push(&bytes[start..cursor]);
    }
    tokens
}

fn hex_token(token: &[u8]) -> Option<Vec<u8>> {
    let value = token.strip_prefix(b"<")?.strip_suffix(b">")?;
    Some(hex_bytes(value))
}

fn is_token(token: &[u8], expected: &[u8]) -> bool {
    token == expected
}

fn token_is(token: Option<&&[u8]>, expected: &[u8]) -> bool {
    token.is_some_and(|token| *token == expected)
}

fn parse_u32(token: &[u8]) -> Option<u32> {
    std::str::from_utf8(token).ok()?.parse().ok()
}

fn parse_f32(token: &[u8]) -> Option<f32> {
    std::str::from_utf8(token).ok()?.parse().ok()
}

pub(crate) fn parse_to_unicode_cmap(bytes: &[u8]) -> Option<ToUnicodeMap> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut map = ToUnicodeMap::default();
    let tokens = pdf_tokens(text).collect::<Vec<_>>();
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            "begincodespacerange" => {
                let count =
                    tokens.get(index.wrapping_sub(1)).and_then(|value| value.parse::<usize>().ok()).unwrap_or(usize::MAX);
                index += 1;
                for _ in 0..count {
                    if index + 1 >= tokens.len() || tokens[index] == "endcodespacerange" {
                        break;
                    }
                    if let (Some(start), Some(end)) = (parse_hex_token(tokens[index]), parse_hex_token(tokens[index + 1])) {
                        if !start.is_empty() && start.len() == end.len() && !map.code_lengths.contains(&start.len()) {
                            map.code_lengths.push(start.len());
                        }
                    }
                    index += 2;
                }
            }
            "beginbfchar" => {
                let count =
                    tokens.get(index.wrapping_sub(1)).and_then(|value| value.parse::<usize>().ok()).unwrap_or(usize::MAX);
                index += 1;
                for _ in 0..count {
                    if index + 1 >= tokens.len() || tokens[index] == "endbfchar" {
                        break;
                    }
                    if let (Some(source), Some(destination)) =
                        (parse_hex_token(tokens[index]), parse_hex_token(tokens[index + 1]))
                    {
                        add_cmap_entry(&mut map, source, destination);
                    }
                    index += 2;
                }
            }
            "beginbfrange" => {
                let count =
                    tokens.get(index.wrapping_sub(1)).and_then(|value| value.parse::<usize>().ok()).unwrap_or(usize::MAX);
                index += 1;
                for _ in 0..count {
                    if index + 2 >= tokens.len() || tokens[index] == "endbfrange" {
                        break;
                    }
                    let source_start = parse_hex_token(tokens[index]);
                    let source_end = parse_hex_token(tokens[index + 1]);
                    if let (Some(start), Some(end)) = (source_start, source_end) {
                        let destination = tokens[index + 2];
                        if destination.starts_with('<') {
                            if let Some(base) = parse_hex_token(destination) {
                                add_cmap_range(&mut map, start, end, base);
                            }
                        }
                        else if destination == "[" {
                            let mut source = start.clone();
                            let mut destination_index = index + 3;
                            while source <= end && destination_index < tokens.len() && tokens[destination_index] != "]" {
                                if let Some(value) = parse_hex_token(tokens[destination_index]) {
                                    add_cmap_entry(&mut map, source.clone(), value);
                                }
                                increment_code(&mut source);
                                destination_index += 1;
                            }
                            index = destination_index;
                        }
                    }
                    index += 3;
                }
            }
            _ => {}
        }
        index += 1;
    }
    (!map.entries.is_empty()).then_some(map)
}

pub(crate) fn pdf_tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split_ascii_whitespace().filter(|token| !token.is_empty())
}

fn parse_hex_token(token: &str) -> Option<Vec<u8>> {
    let value = token.strip_prefix('<')?.strip_suffix('>')?;
    Some(hex_bytes(value.as_bytes()))
}

fn add_cmap_entry(map: &mut ToUnicodeMap, source: Vec<u8>, destination: Vec<u8>) {
    map.max_code_len = map.max_code_len.max(source.len());
    map.entries.insert(source, decode_utf16be_or_utf8(&destination));
}

fn add_cmap_range(map: &mut ToUnicodeMap, start: Vec<u8>, end: Vec<u8>, mut destination: Vec<u8>) {
    if start.is_empty() || start.len() != end.len() || destination.is_empty() || destination.len() % 2 != 0 || start.len() > 4 {
        return;
    }
    let source_start = start.iter().fold(0u32, |value, byte| (value << 8) | u32::from(*byte));
    let source_end = end.iter().fold(0u32, |value, byte| (value << 8) | u32::from(*byte));
    if source_end < source_start || source_end - source_start >= 65_536 {
        return;
    }
    for offset in 0..=source_end.saturating_sub(source_start) {
        let source_value = source_start + offset;
        let source_len = start.len();
        let mut source = vec![0u8; source_len];
        for (index, byte) in source.iter_mut().enumerate() {
            *byte = (source_value >> (8 * (source_len - index - 1))) as u8;
        }
        add_cmap_entry(map, source, destination.clone());
        if offset < source_end.saturating_sub(source_start) && !increment_unicode(&mut destination) {
            break;
        }
    }
}

fn increment_code(value: &mut Vec<u8>) {
    for byte in value.iter_mut().rev() {
        if *byte < 0xff {
            *byte += 1;
            return;
        }
        *byte = 0;
    }
}

pub(crate) fn increment_unicode(value: &mut Vec<u8>) -> bool {
    if value.len() < 2 || value.len() % 2 != 0 {
        return false;
    }
    for index in (0..value.len()).rev().step_by(2) {
        if index == 0 {
            break;
        }
        let current = u16::from_be_bytes([value[index - 1], value[index]]);
        if current < 0xffff {
            let next = current + 1;
            let bytes = next.to_be_bytes();
            value[index - 1] = bytes[0];
            value[index] = bytes[1];
            return true;
        }
        value[index - 1] = 0;
        value[index] = 0;
    }
    false
}

fn decode_utf16be_or_utf8(value: &[u8]) -> String {
    if value.len() % 2 == 0 {
        let units = value.chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect::<Vec<_>>();
        if let Ok(text) = String::from_utf16(&units) {
            return text;
        }
    }
    String::from_utf8_lossy(value).into_owned()
}
