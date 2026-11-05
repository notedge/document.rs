//! A1 reference parsing and formatting.

use super::model::CellRange;

/// Parses `A1` or `$A$1` into a one-based row and column.
pub(crate) fn parse_reference(text: &str) -> Option<(u32, u32)> {
    let trimmed = text.trim().replace('$', "");
    let split = trimmed.find(|character: char| character.is_ascii_digit())?;
    if split == 0 {
        return None;
    }
    let column = parse_column(&trimmed[..split])?;
    let row: u32 = trimmed[split..].parse().ok()?;
    if row == 0 {
        return None;
    }
    Some((row, column))
}

/// Parses `A1:B4` into a range; a single reference becomes a one-cell range.
pub(crate) fn parse_range(text: &str) -> Option<CellRange> {
    let text = text.trim();
    let (start, end) = match text.split_once(':') {
        Some((start, end)) => (start, end),
        None => (text, text),
    };
    let (start_row, start_column) = parse_reference(start)?;
    let (end_row, end_column) = parse_reference(end)?;
    Some(CellRange {
        start_row: start_row.min(end_row),
        end_row: start_row.max(end_row),
        start_column: start_column.min(end_column),
        end_column: start_column.max(end_column),
    })
}

/// Parses column letters such as `AB` into a one-based index.
pub(crate) fn parse_column(text: &str) -> Option<u32> {
    let mut column = 0u32;
    for character in text.chars() {
        if !character.is_ascii_alphabetic() {
            return None;
        }
        column = column.checked_mul(26)?;
        column = column.checked_add(character.to_ascii_uppercase() as u32 - 'A' as u32 + 1)?;
    }
    if column == 0 {
        None
    }
    else {
        Some(column)
    }
}

/// Formats a one-based column index as letters.
pub(crate) fn column_letters(mut column: u32) -> String {
    let mut letters = String::new();
    while column > 0 {
        let remainder = ((column - 1) % 26) as u8;
        letters.push((b'A' + remainder) as char);
        column = (column - 1) / 26;
    }
    letters.chars().rev().collect()
}

/// Formats a one-based row and column as `A1`.
pub(crate) fn reference(row: u32, column: u32) -> String {
    format!("{}{row}", column_letters(column))
}
