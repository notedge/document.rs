//! Excel serial dates and number-format display.

use super::model::{CellStyle, CellValue, DateKind, DateSystem, ExcelDate, WorkbookStyles};

/// Converts an Excel serial number into a structured date.
pub(crate) fn excel_date(serial: f64, system: DateSystem, format: &str) -> ExcelDate {
    let kind = detect_date_kind(format).unwrap_or(DateKind::Date);
    let (year, month, day, hour, minute, second) = serial_parts(serial, system);
    let iso = match kind {
        DateKind::Date => format!("{year:04}-{month:02}-{day:02}"),
        DateKind::Time => format!("{hour:02}:{minute:02}:{second:02}"),
        DateKind::DateTime => format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}"),
    };
    ExcelDate { serial, iso, kind }
}

fn serial_parts(serial: f64, system: DateSystem) -> (i32, u32, u32, u32, u32, u32) {
    let mut days = serial.floor() as i64;
    let mut seconds = ((serial - serial.floor()) * 86_400.0).round() as i64;
    if seconds >= 86_400 {
        seconds -= 86_400;
        days += 1;
    }
    let (mut year, mut month, mut day) = match system {
        DateSystem::Date1904 => civil_from_days(days + days_from_civil(1904, 1, 1)),
        DateSystem::Date1900 => {
            let base = days_from_civil(1899, 12, 31);
            // Serial 60 is the nonexistent 1900-02-29. Keep later serials aligned
            // with the epoch Excel actually prints.
            let adjusted = if days < 60 { days } else { days - 1 };
            civil_from_days(base + adjusted)
        }
    };
    if year < 1900 {
        year = 1900;
        month = 1;
        day = 1;
    }
    let hour = (seconds / 3_600) as u32;
    seconds %= 3_600;
    let minute = (seconds / 60) as u32;
    let second = (seconds % 60) as u32;
    (year, month, day, hour, minute, second)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i64;
    let day = day as i64;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe as i64 * 365 + (yoe / 4) as i64 - (yoe / 100) as i64 + doy;
    era as i64 * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    ((year + if month <= 2 { 1 } else { 0 }) as i32, month as u32, day as u32)
}

/// Detects whether a number format code expresses a date, time or both.
pub(crate) fn detect_date_kind(code: &str) -> Option<DateKind> {
    let tokens = date_tokens(code);
    let has_date = tokens.iter().any(|token| matches!(token, 'y' | 'd'));
    let has_time = tokens.iter().any(|token| matches!(token, 'h' | 's')) || code.contains("[h]") || code.contains("[m]") && !has_date;
    match (has_date, has_time) {
        (true, true) => Some(DateKind::DateTime),
        (true, false) => Some(DateKind::Date),
        (false, true) => Some(DateKind::Time),
        (false, false) => None,
    }
}

/// Whether a format code is a date or time format.
pub(crate) fn is_date_format(code: &str) -> bool {
    detect_date_kind(code).is_some()
}

/// Converts an ISO-like textual date into the serial-based model.
pub(crate) fn date_from_iso(text: &str, format: &str, system: DateSystem) -> Option<ExcelDate> {
    let kind = detect_date_kind(format).unwrap_or(DateKind::Date);
    let text = text.trim();
    let (date_part, time_part) = match text.split_once('T') {
        Some((date, time)) => (Some(date), Some(time)),
        None if text.contains('-') => (Some(text), None),
        None => (None, Some(text)),
    };
    let mut year = 0i32;
    let mut month = 0u32;
    let mut day = 0u32;
    if let Some(date) = date_part {
        let mut parts = date.split('-');
        year = parts.next()?.parse().ok()?;
        month = parts.next()?.parse().ok()?;
        day = parts.next()?.parse().ok()?;
    }
    let mut hour = 0u32;
    let mut minute = 0u32;
    let mut second = 0u32;
    if let Some(time) = time_part {
        let mut parts = time.split(':');
        hour = parts.next()?.parse().ok()?;
        minute = parts.next()?.parse().ok()?;
        second = parts.next().unwrap_or("0").split('.').next()?.parse().ok()?;
    }
    let days = if date_part.is_some() {
        let offset = match system {
            DateSystem::Date1900 => days_from_civil(1899, 12, 30),
            DateSystem::Date1904 => days_from_civil(1904, 1, 1),
        };
        days_from_civil(year, month, day) - offset
    }
    else {
        0
    };
    let serial = days as f64 + (hour as f64 * 3600.0 + minute as f64 * 60.0 + second as f64) / 86_400.0;
    Some(ExcelDate { serial, iso: match kind {
        DateKind::Date => format!("{year:04}-{month:02}-{day:02}"),
        DateKind::Time => format!("{hour:02}:{minute:02}:{second:02}"),
        DateKind::DateTime => format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}"),
    }, kind })
}

fn date_tokens(code: &str) -> Vec<char> {
    let mut tokens = Vec::new();
    let mut in_quote = false;
    let mut in_bracket = false;
    for character in code.chars() {
        match character {
            '"' => in_quote = !in_quote,
            '[' if !in_quote => in_bracket = true,
            ']' if !in_quote => in_bracket = false,
            'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' if !in_quote && !in_bracket => {
                tokens.push(character.to_ascii_lowercase())
            }
            _ => {}
        }
    }
    tokens
}

/// One displayed cell value plus whether the number format was approximated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Displayed {
    pub text: String,
    pub approximated: bool,
}

/// Renders a cell for the grid view.
pub(crate) fn display_cell(value: &CellValue, style: &CellStyle, styles: &WorkbookStyles) -> Displayed {
    match value {
        CellValue::Empty => Displayed { text: String::new(), approximated: false },
        CellValue::Text(text) => Displayed { text: text.clone(), approximated: false },
        CellValue::SharedString { text, .. } => Displayed { text: text.clone(), approximated: false },
        CellValue::Boolean(value) => Displayed { text: if *value { "TRUE".into() } else { "FALSE".into() }, approximated: false },
        CellValue::Error { code } => Displayed { text: code.clone(), approximated: false },
        CellValue::Date(date) => Displayed { text: render_date(date, style.number_format.as_deref()), approximated: false },
        CellValue::Number(number) => render_number(*number, style, styles),
    }
}

fn render_date(date: &ExcelDate, format: Option<&str>) -> String {
    let Some(format) = format
    else {
        return date.iso.clone();
    };
    let (year, month, day, hour, minute, second) = iso_parts(&date.iso, date.kind);
    let mut output = String::new();
    let characters: Vec<char> = format.chars().collect();
    let mut index = 0;
    let use_twelve_hour = format.to_ascii_uppercase().contains("AM/PM");
    while index < characters.len() {
        let character = characters[index];
        match character {
            '"' => {
                index += 1;
                while index < characters.len() && characters[index] != '"' {
                    output.push(characters[index]);
                    index += 1;
                }
                index += 1;
            }
            '[' => {
                while index < characters.len() && characters[index] != ']' {
                    index += 1;
                }
                index += 1;
            }
            '\\' => {
                index += 1;
                if let Some(literal) = characters.get(index) {
                    output.push(*literal);
                }
                index += 1;
            }
            'y' | 'Y' => {
                let count = run_length(&characters, index);
                output.push_str(&if count <= 2 { format!("{:02}", year % 100) } else { format!("{year:04}") });
                index += count;
            }
            'd' | 'D' => {
                let count = run_length(&characters, index);
                output.push_str(&if count == 1 { day.to_string() } else { format!("{day:02}") });
                index += count;
            }
            'h' | 'H' => {
                let count = run_length(&characters, index);
                let value = if use_twelve_hour {
                    let value = if hour % 12 == 0 { 12 } else { hour % 12 };
                    value
                }
                else {
                    hour
                };
                output.push_str(&if count == 1 { value.to_string() } else { format!("{value:02}") });
                index += count;
            }
            's' | 'S' => {
                let count = run_length(&characters, index);
                output.push_str(&if count == 1 { second.to_string() } else { format!("{second:02}") });
                index += count;
            }
            'm' | 'M' => {
                let count = run_length(&characters, index);
                let hour_before = characters[..index].iter().rev().any(|character| matches!(character, 'h' | 'H'));
                let second_after = characters[index + count..].iter().any(|character| matches!(character, 's' | 'S'));
                if hour_before || second_after {
                    output.push_str(&if count == 1 { minute.to_string() } else { format!("{minute:02}") });
                }
                else {
                    output.push_str(&if count == 1 { month.to_string() } else { format!("{month:02}") });
                }
                index += count;
            }
            'A' | 'a' if format[index..].to_ascii_uppercase().starts_with("AM/PM") => {
                output.push_str(if hour < 12 { "AM" } else { "PM" });
                index += 5;
            }
            _ => {
                output.push(character);
                index += 1;
            }
        }
    }
    output
}

fn iso_parts(iso: &str, kind: DateKind) -> (i32, u32, u32, u32, u32, u32) {
    let (date, time) = match iso.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => match kind {
            DateKind::Time => ("0000-00-00", Some(iso)),
            _ => (iso, None),
        },
    };
    let mut date_parts = date.split('-').map(|part| part.parse::<u32>().unwrap_or(0));
    let year = date_parts.next().unwrap_or(0) as i32;
    let month = date_parts.next().unwrap_or(0);
    let day = date_parts.next().unwrap_or(0);
    let mut time_parts = time.unwrap_or("00:00:00").split(':').map(|part| part.parse::<u32>().unwrap_or(0));
    let hour = time_parts.next().unwrap_or(0);
    let minute = time_parts.next().unwrap_or(0);
    let second = time_parts.next().unwrap_or(0);
    (year, month, day, hour, minute, second)
}

fn run_length(characters: &[char], index: usize) -> usize {
    let character = characters[index].to_ascii_lowercase();
    characters[index..].iter().take_while(|next| next.to_ascii_lowercase() == character).count()
}

fn render_number(number: f64, style: &CellStyle, styles: &WorkbookStyles) -> Displayed {
    let Some(format) = style.number_format.as_deref()
    else {
        return Displayed { text: general_number(number), approximated: false };
    };
    if format.eq_ignore_ascii_case("general") {
        return Displayed { text: general_number(number), approximated: false };
    }
    if format.trim() == "@" {
        return Displayed { text: general_number(number), approximated: false };
    }
    if is_date_format(format) {
        let system = DateSystem::Date1900;
        let date = excel_date(number, system, format);
        return Displayed { text: render_date(&date, Some(format)), approximated: false };
    }
    let _ = styles;
    format_plain_number(number, format)
}

fn general_number(number: f64) -> String {
    if !number.is_finite() {
        return "#NUM!".into();
    }
    if number.fract() == 0.0 && number.abs() < 1e15 {
        return format!("{number:.0}");
    }
    let mut text = format!("{number:.10}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn format_plain_number(number: f64, format: &str) -> Displayed {
    let sections: Vec<&str> = format.split(';').collect();
    let section = if number < 0.0 && sections.len() > 1 { sections[1] } else { sections[0] };
    let mut approximated = false;
    if format.contains('?') || format.contains("E+") || format.contains("E-") || format.contains("/") {
        approximated = true;
    }
    let raw_section = section;
    let section = strip_sections(raw_section);
    let quoted = collect_literals(raw_section);
    let percent = section.matches('%').count() > 0;
    let per_mille = section.contains('‰');
    let grouping = section.split('.').next().unwrap_or("").contains(',');
    let decimals = decimals_after_point(&section);
    let mut value = number.abs();
    if percent {
        value *= 100.0;
    }
    if per_mille {
        value *= 1000.0;
    }
    let mut body = if decimals > 0 {
        format!("{value:.decimals$}")
    }
    else {
        format!("{value:.0}")
    };
    if grouping {
        body = group_thousands(&body, decimals);
    }
    let negative = number < 0.0 && sections.len() == 1;
    let wrapped = section.contains('(') && section.contains(')');
    let mut output = String::new();
    if negative && !wrapped {
        output.push('-');
    }
    if wrapped && negative {
        output.push('(');
    }
    output.push_str(&quoted.prefix);
    output.push_str(&body);
    output.push_str(&quoted.suffix);
    if percent {
        output.push('%');
    }
    if per_mille {
        output.push('‰');
    }
    if wrapped && negative {
        output.push(')');
    }
    Displayed { text: output, approximated }
}

#[derive(Default)]
struct Literals {
    prefix: String,
    suffix: String,
}

/// Splits a format into literal text outside the numeric placeholder.
fn collect_literals(section: &str) -> Literals {
    // Quoted text and bare currency symbols are the only literals this
    // formatter reproduces. Everything before the first placeholder becomes the
    // prefix, everything after it becomes the suffix.
    let mut literals = Literals::default();
    let mut in_quote = false;
    let mut in_bracket = false;
    let mut placeholder = false;
    let mut characters = section.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' => in_quote = !in_quote,
            '[' if !in_quote => in_bracket = true,
            ']' if !in_quote => in_bracket = false,
            '\\' if !in_quote && !in_bracket => {
                if let Some(literal) = characters.next() {
                    finish_literal(&mut literals, literal, placeholder);
                }
            }
            '0' | '#' if !in_quote && !in_bracket => placeholder = true,
            '$' | '€' | '¥' | '£' if !in_bracket => finish_literal(&mut literals, character, placeholder),
            ' ' if !in_quote && !in_bracket => finish_literal(&mut literals, character, placeholder),
            _ if in_quote => finish_literal(&mut literals, character, placeholder),
            _ => {}
        }
    }
    literals
}

fn finish_literal(literals: &mut Literals, character: char, placeholder: bool) {
    if placeholder {
        literals.suffix.push(character);
    }
    else {
        literals.prefix.push(character);
    }
}

fn strip_sections(section: &str) -> String {
    let mut output = String::new();
    let mut in_quote = false;
    let mut in_bracket = false;
    for character in section.chars() {
        match character {
            '"' => in_quote = !in_quote,
            '[' if !in_quote => in_bracket = true,
            ']' if !in_quote => in_bracket = false,
            _ => {
                if !in_bracket || matches!(character, 'h' | 'H' | 'm' | 'M' | 's' | 'S') {
                    output.push(character);
                }
            }
        }
    }
    output
}

fn decimals_after_point(section: &str) -> usize {
    let Some((_, fractional)) = section.split_once('.')
    else {
        return 0;
    };
    fractional
        .chars()
        .take_while(|character| matches!(character, '0' | '#'))
        .filter(|character| *character == '0')
        .count()
}

fn group_thousands(body: &str, decimals: usize) -> String {
    let (integer, fractional) = match body.split_once('.') {
        Some((integer, fractional)) => (integer, Some(fractional)),
        None => (body, None),
    };
    let mut grouped = String::new();
    for (index, character) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(character);
    }
    match fractional {
        Some(fractional) if decimals > 0 => format!("{grouped}.{fractional}"),
        _ => grouped,
    }
}
