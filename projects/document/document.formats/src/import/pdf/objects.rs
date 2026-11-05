use super::{
    streams::{decode_stream, stream_contents},
    syntax::{skip_space, split_pdf_syntax_tokens, split_pdf_whitespace, trim_pdf_brackets},
};

pub(crate) fn page_resource_sections(bodies: &[(u32, Vec<u8>)]) -> Vec<Vec<u8>> {
    let mut result = Vec::new();
    if let Some((_, catalog)) = bodies.iter().find(|(_, body)| dictionary_type_is(body, b"/Catalog")) {
        if let Some(pages) = body_reference(catalog, b"/Pages") {
            collect_page_resources(bodies, pages, None, &mut result, &mut Vec::new());
        }
    }
    if result.is_empty() {
        for (_, body) in bodies.iter().filter(|(_, body)| dictionary_type_is(body, b"/Page")) {
            if let Some(resources) = resource_dictionary(body, bodies) {
                result.push(resources);
            }
        }
    }
    result
}

fn collect_page_resources(
    bodies: &[(u32, Vec<u8>)],
    object: u32,
    inherited: Option<Vec<u8>>,
    result: &mut Vec<Vec<u8>>,
    visited: &mut Vec<u32>,
) {
    if visited.contains(&object) {
        return;
    }
    visited.push(object);
    let Some((_, body)) = bodies.iter().find(|(id, _)| *id == object)
    else {
        return;
    };
    let resources = resource_dictionary(body, bodies).or(inherited);
    if dictionary_type_is(body, b"/Page") {
        if let Some(resources) = resources {
            result.push(resources);
        }
        return;
    }
    for child in body_reference_values(body, b"/Kids").unwrap_or_default() {
        collect_page_resources(bodies, child, resources.clone(), result, visited);
    }
}

fn resource_dictionary(body: &[u8], bodies: &[(u32, Vec<u8>)]) -> Option<Vec<u8>> {
    let position = body.windows(10).position(|window| window == b"/Resources")?;
    let start = skip_space(body, position + 10);
    if body.get(start..start + 2) == Some(b"<<") {
        let end = dictionary_end(body, start)?;
        return Some(body[start..end].to_vec());
    }
    let id = reference_ids(body.get(start..)?).first().copied()?;
    let resource_body = bodies.iter().find(|(object, _)| *object == id).map(|(_, body)| body)?;
    let start = resource_body.windows(2).position(|window| window == b"<<")?;
    let end = dictionary_end(resource_body, start)?;
    Some(resource_body[start..end].to_vec())
}

fn dictionary_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut cursor = start;
    while cursor + 1 < bytes.len() {
        if bytes[cursor..].starts_with(b"<<") {
            depth += 1;
            cursor += 2;
            continue;
        }
        if bytes[cursor..].starts_with(b">>") {
            depth = depth.checked_sub(1)?;
            cursor += 2;
            if depth == 0 {
                return Some(cursor);
            }
            continue;
        }
        cursor += 1;
    }
    None
}

pub(crate) fn resource_font_references(section: &[u8]) -> Vec<(String, u32)> {
    let mut result = Vec::new();
    let tokens = split_pdf_syntax_tokens(section).collect::<Vec<_>>();
    for index in 0..tokens.len().saturating_sub(3) {
        let token = tokens[index];
        let Some(name) = token.strip_prefix(b"/")
        else {
            continue;
        };
        if name.is_empty() || tokens[index + 2] != b"0" || tokens[index + 3] != b"R" {
            continue;
        }
        let Ok(object_id) = std::str::from_utf8(tokens[index + 1]).unwrap_or("").parse()
        else {
            continue;
        };
        result.push((String::from_utf8_lossy(name).into_owned(), object_id));
    }
    result
}
pub(crate) fn object_id(bytes: &[u8], dictionary_start: usize) -> Option<u32> {
    let tokens = split_pdf_whitespace(&bytes[..dictionary_start]).rev();
    let mut tokens = tokens.skip(1);
    let _generation = tokens.next()?;
    std::str::from_utf8(tokens.next()?).ok()?.parse().ok()
}

/// Content-stream object ids grouped by source page, in page-tree order.
///
/// Each inner vector is one source page. This keeps the `/Pages` boundary
/// visible to the importer so text from different pages is never merged into
/// one paragraph and text on one page is never split per glyph run.
pub(crate) fn page_content_groups(bodies: &[(u32, Vec<u8>)]) -> Vec<Vec<u32>> {
    if let Some((_, catalog)) = bodies.iter().find(|(_, body)| dictionary_type_is(body, b"/Catalog")) {
        if let Some(pages) = body_reference(catalog, b"/Pages") {
            let mut result = Vec::new();
            collect_page_content_groups(bodies, pages, &mut result, &mut Vec::new());
            if !result.is_empty() {
                return result;
            }
        }
    }
    page_content_groups_by_scan(bodies)
}

fn collect_page_content_groups(bodies: &[(u32, Vec<u8>)], object: u32, result: &mut Vec<Vec<u32>>, visited: &mut Vec<u32>) {
    if visited.contains(&object) {
        return;
    }
    visited.push(object);
    let Some(body) = bodies.iter().find(|(id, _)| *id == object).map(|(_, body)| body)
    else {
        return;
    };
    if dictionary_type_is(body, b"/Page") {
        let contents = body_reference_values(body, b"/Contents").unwrap_or_default();
        let mut group = Vec::new();
        for id in contents {
            if !group.contains(&id) {
                group.push(id);
            }
        }
        result.push(group);
        return;
    }
    if let Some(kids) = body_reference_values(body, b"/Kids") {
        for child in kids {
            collect_page_content_groups(bodies, child, result, visited);
        }
    }
}

/// Page box sizes in page-tree order, in points.
///
/// A `/MediaBox` may be inherited from a `/Pages` node, so the walk carries the
/// nearest declared box down the tree. A page without any box is skipped rather
/// than given a guessed size.
pub(crate) fn page_media_boxes(bodies: &[(u32, Vec<u8>)]) -> Vec<(f64, f64)> {
    let mut result = Vec::new();
    if let Some((_, catalog)) = bodies.iter().find(|(_, body)| dictionary_type_is(body, b"/Catalog")) {
        if let Some(pages) = body_reference(catalog, b"/Pages") {
            collect_page_boxes(bodies, pages, None, &mut result, &mut Vec::new());
            if !result.is_empty() {
                return result;
            }
        }
    }
    bodies
        .iter()
        .filter(|(_, body)| dictionary_type_is(body, b"/Page"))
        .filter_map(|(_, body)| media_box(body, bodies))
        .collect()
}

fn collect_page_boxes(
    bodies: &[(u32, Vec<u8>)],
    object: u32,
    inherited: Option<(f64, f64)>,
    result: &mut Vec<(f64, f64)>,
    visited: &mut Vec<u32>,
) {
    if visited.contains(&object) {
        return;
    }
    visited.push(object);
    let Some(body) = bodies.iter().find(|(id, _)| *id == object).map(|(_, body)| body)
    else {
        return;
    };
    let page_box = media_box(body, bodies).or(inherited);
    if dictionary_type_is(body, b"/Page") {
        if let Some(page_box) = page_box {
            result.push(page_box);
        }
        return;
    }
    for child in body_reference_values(body, b"/Kids").unwrap_or_default() {
        collect_page_boxes(bodies, child, page_box, result, visited);
    }
}

/// Reads one `/MediaBox` as `(width, height)` in points.
fn media_box(body: &[u8], bodies: &[(u32, Vec<u8>)]) -> Option<(f64, f64)> {
    let position = body.windows(9).position(|window| window == b"/MediaBox")?;
    let start = skip_space(body, position + 9);
    let slice = body.get(start..)?;
    if slice.first() == Some(&b'[') {
        let end = slice.iter().position(|byte| *byte == b']')?;
        return parse_media_box(&slice[1..end]);
    }
    let id = reference_ids(slice).first().copied()?;
    let referred = bodies.iter().find(|(object, _)| *object == id).map(|(_, body)| body)?;
    let start = referred.iter().position(|byte| *byte == b'[')?;
    let end = referred.get(start..)?.iter().position(|byte| *byte == b']')? + start;
    parse_media_box(&referred[start + 1..end])
}

fn parse_media_box(values: &[u8]) -> Option<(f64, f64)> {
    let numbers = split_pdf_whitespace(values)
        .filter_map(|token| std::str::from_utf8(token).ok()?.parse::<f64>().ok())
        .collect::<Vec<_>>();
    let [x0, y0, x1, y1] = numbers.as_slice()
    else {
        return None;
    };
    let width = x1 - x0;
    let height = y1 - y0;
    (width > 0.0 && height > 0.0).then_some((width, height))
}

pub(crate) fn body_reference(body: &[u8], key: &[u8]) -> Option<u32> {
    body_reference_values(body, key)?.into_iter().next()
}

pub(crate) fn dictionary_type_is(body: &[u8], expected: &[u8]) -> bool {
    let Some(position) = body.windows(5).position(|window| window == b"/Type")
    else {
        return false;
    };
    let start = skip_space(body, position + 5);
    let Some(value) = body.get(start..start + expected.len())
    else {
        return false;
    };
    value == expected
        && body.get(start + expected.len()).is_none_or(|byte| byte.is_ascii_whitespace() || b"/<>[]()".contains(byte))
}

pub(crate) fn dictionary_subtype_is(body: &[u8], expected: &[u8]) -> bool {
    let Some(position) = body.windows(8).position(|window| window == b"/Subtype")
    else {
        return false;
    };
    let start = skip_space(body, position + 8);
    let Some(value) = body.get(start..start + expected.len())
    else {
        return false;
    };
    value == expected
        && body.get(start + expected.len()).is_none_or(|byte| byte.is_ascii_whitespace() || b"/<>[]()".contains(byte))
}

pub(crate) fn body_reference_values(body: &[u8], key: &[u8]) -> Option<Vec<u32>> {
    let position = body.windows(key.len()).position(|window| window == key)?;
    let values = reference_ids(body.get(position + key.len()..)?);
    (!values.is_empty()).then_some(values)
}

pub(crate) fn object_bodies(bytes: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(relative) = bytes[search..].windows(4).position(|window| window == b" obj") {
        let marker = search + relative;
        let Some(id) = object_id_before_obj(bytes, marker)
        else {
            search = marker + 4;
            continue;
        };
        let start = marker + 4;
        let end =
            bytes[start..].windows(6).position(|window| window == b"endobj").map_or(bytes.len(), |position| start + position);
        result.push((id, bytes[start..end].to_vec()));
        search = end.saturating_add(6);
        if search >= bytes.len() {
            break;
        }
    }
    result
}

fn page_content_groups_by_scan(bodies: &[(u32, Vec<u8>)]) -> Vec<Vec<u32>> {
    bodies
        .iter()
        .filter(|(_, body)| dictionary_type_is(body, b"/Page"))
        .map(|(_, body)| body_reference_values(body, b"/Contents").unwrap_or_default())
        .collect()
}

/// Object bodies discovered in the file, either as top-level `N 0 obj` blocks
/// or inside a `/ObjStm` compressed object stream.
pub(crate) struct BodyScan {
    pub(crate) bodies: Vec<(u32, Vec<u8>)>,
    pub(crate) failures: Vec<ObjectStreamFailure>,
}

/// An `/ObjStm` stream that could not be decoded or whose header does not
/// describe the advertised objects.
pub(crate) struct ObjectStreamFailure {
    pub(crate) object_id: u32,
    pub(crate) reason: String,
}

/// Collect object bodies from the top-level scan and from `/ObjStm` streams.
///
/// Cross-reference streams may pack font, page and resource dictionaries into
/// a compressed object stream, where a `N 0 obj` text scan cannot see them.
/// Every object stream is decoded and expanded so those dictionaries remain
/// reachable. A stream that cannot be expanded is reported instead of being
/// ignored, because the fonts it holds would otherwise silently lose text.
///
/// A top-level body wins when the same object id appears both directly and in
/// an object stream, matching the reader precedence in PDF 32000-1 §7.5.7.
pub(crate) fn scan_object_bodies(bytes: &[u8]) -> BodyScan {
    let mut bodies = object_bodies(bytes);
    let mut failures = Vec::new();
    for stream in stream_contents(bytes) {
        if !dictionary_type_is(stream.dictionary, b"/ObjStm") {
            continue;
        }
        let object_id = stream.object_id.unwrap_or(0);
        match expand_object_stream(stream.dictionary, stream.data) {
            Ok(expanded) => {
                for (id, body) in expanded {
                    if !bodies.iter().any(|(existing, _)| *existing == id) {
                        bodies.push((id, body));
                    }
                }
            }
            Err(reason) => failures.push(ObjectStreamFailure { object_id, reason }),
        }
    }
    BodyScan { bodies, failures }
}

/// Expand `/N` objects from an object stream.
///
/// The decoded stream starts with a header of `N` pairs `object-number
/// byte-offset` relative to `/First`, followed by the object bodies in the same
/// order. Object `i` ends where object `i + 1` begins, and the last object ends
/// at the end of the decoded stream.
fn expand_object_stream(dictionary: &[u8], data: &[u8]) -> Result<Vec<(u32, Vec<u8>)>, String> {
    let count = integer_entry(dictionary, b"/N").ok_or_else(|| "missing `/N`".to_string())?;
    let first = integer_entry(dictionary, b"/First").ok_or_else(|| "missing `/First`".to_string())?;
    let decoded = decode_stream(dictionary, data).map_err(|error| format!("stream decode failed: {error}"))?;
    if first > decoded.len() {
        return Err(format!("`/First` {first} exceeds decoded length {}", decoded.len()));
    }
    let header = std::str::from_utf8(&decoded[..first]).map_err(|_| "header is not ASCII".to_string())?;
    let numbers = header
        .split_ascii_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "header is not an integer pair list".to_string())?;
    if numbers.len() < count.saturating_mul(2) {
        return Err(format!("header lists {} entries but `/N` declares {count}", numbers.len()));
    }
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let id = numbers[index * 2];
        let offset = numbers[index * 2 + 1] as usize;
        let start = first
            .checked_add(offset)
            .filter(|start| *start <= decoded.len())
            .ok_or_else(|| format!("object {id} offset {offset} is outside the stream"))?;
        let next = numbers.get(index * 2 + 3).map_or(decoded.len(), |value| first.saturating_add(*value as usize));
        if next < start || next > decoded.len() {
            return Err(format!("object {id} extent is invalid"));
        }
        result.push((id, decoded[start..next].to_vec()));
    }
    Ok(result)
}

/// Read an integer entry such as `/N` or `/First` from a compact dictionary
/// where the next key may touch the value without whitespace.
fn integer_entry(dictionary: &[u8], key: &[u8]) -> Option<usize> {
    let tokens = split_pdf_syntax_tokens(dictionary).collect::<Vec<_>>();
    tokens.windows(2).find(|pair| pair[0] == key).and_then(|pair| std::str::from_utf8(pair[1]).ok()?.parse().ok())
}

pub(crate) fn object_id_before_obj(bytes: &[u8], marker: usize) -> Option<u32> {
    let mut tokens = split_pdf_whitespace(&bytes[..marker]).rev();
    let _generation = tokens.next()?;
    std::str::from_utf8(tokens.next()?).ok()?.parse().ok()
}

pub(crate) fn reference_ids(bytes: &[u8]) -> Vec<u32> {
    let end = bytes.iter().position(|byte| *byte == b'>').unwrap_or(bytes.len());
    let tokens = split_pdf_syntax_tokens(&bytes[..end]).map(trim_pdf_brackets);
    let mut ids = Vec::new();
    let mut tokens = tokens.peekable();
    while let Some(token) = tokens.next() {
        if let Ok(id) = std::str::from_utf8(token).unwrap_or("").parse::<u32>() {
            if tokens.next().is_some_and(|generation| generation == b"0")
                && tokens.next().is_some_and(|reference| reference == b"R")
            {
                ids.push(id);
            }
        }
    }
    ids
}
pub(crate) fn body_contains_name(body: &[u8], name: &[u8]) -> bool {
    body.windows(name.len()).any(|window| window == name)
}
