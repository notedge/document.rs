use super::syntax::{skip_space, split_pdf_syntax_tokens, split_pdf_whitespace, trim_pdf_brackets};

pub(crate) fn page_resource_sections(bytes: &[u8]) -> Vec<Vec<u8>> {
    let bodies = object_bodies(bytes);
    let mut result = Vec::new();
    if let Some((_, catalog)) = bodies.iter().find(|(_, body)| dictionary_type_is(body, b"/Catalog")) {
        if let Some(pages) = body_reference(catalog, b"/Pages") {
            collect_page_resources(&bodies, pages, None, &mut result, &mut Vec::new());
        }
    }
    if result.is_empty() {
        for (_, body) in bodies.iter().filter(|(_, body)| dictionary_type_is(body, b"/Page")) {
            if let Some(resources) = resource_dictionary(body, &bodies) {
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
pub(crate) fn page_content_groups(bytes: &[u8]) -> Vec<Vec<u32>> {
    if let Some(catalog) = object_bodies(bytes).into_iter().find(|(_, body)| dictionary_type_is(body, b"/Catalog")) {
        if let Some(pages) = body_reference(&catalog.1, b"/Pages") {
            let mut result = Vec::new();
            collect_page_content_groups(bytes, pages, &mut result, &mut Vec::new());
            if !result.is_empty() {
                return result;
            }
        }
    }
    page_content_groups_by_scan(bytes)
}

fn collect_page_content_groups(bytes: &[u8], object: u32, result: &mut Vec<Vec<u32>>, visited: &mut Vec<u32>) {
    if visited.contains(&object) {
        return;
    }
    visited.push(object);
    let Some(body) = object_bodies(bytes).into_iter().find(|(id, _)| *id == object).map(|(_, body)| body)
    else {
        return;
    };
    if dictionary_type_is(&body, b"/Page") {
        let contents = body_reference_values(&body, b"/Contents").unwrap_or_default();
        let mut group = Vec::new();
        for id in contents {
            if !group.contains(&id) {
                group.push(id);
            }
        }
        result.push(group);
        return;
    }
    if let Some(kids) = body_reference_values(&body, b"/Kids") {
        for child in kids {
            collect_page_content_groups(bytes, child, result, visited);
        }
    }
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

fn page_content_groups_by_scan(bytes: &[u8]) -> Vec<Vec<u32>> {
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(relative) = bytes[search..].windows(4).position(|window| window == b" obj") {
        let marker = search + relative;
        if object_id_before_obj(bytes, marker).is_none() {
            search = marker + 4;
            continue;
        }
        let object_start = marker + 4;
        let end = bytes[marker + 4..]
            .windows(6)
            .position(|window| window == b"endobj")
            .map_or(bytes.len(), |position| marker + 4 + position);
        let body = &bytes[object_start..end];
        if dictionary_type_is(body, b"/Page") {
            let mut group = Vec::new();
            if let Some(contents) = body.windows(9).position(|window| window == b"/Contents") {
                for id in reference_ids(&body[contents + 9..]) {
                    if !group.contains(&id) {
                        group.push(id);
                    }
                }
            }
            result.push(group);
        }
        search = end.saturating_add(6);
        if search >= bytes.len() {
            break;
        }
    }
    result
}

pub(crate) fn object_id_before_obj(bytes: &[u8], marker: usize) -> Option<u32> {
    let mut tokens = split_pdf_whitespace(&bytes[..marker]).rev();
    let _generation = tokens.next()?;
    std::str::from_utf8(tokens.next()?).ok()?.parse().ok()
}

pub(crate) fn reference_ids(bytes: &[u8]) -> Vec<u32> {
    let end = bytes.iter().position(|byte| *byte == b'>').unwrap_or(bytes.len());
    let tokens = split_pdf_whitespace(&bytes[..end]).map(trim_pdf_brackets);
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
