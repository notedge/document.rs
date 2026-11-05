//! Rebuild flow structure from visual PDF lines.
//!
//! A PDF text layer carries glyph runs positioned on baselines, not paragraphs,
//! headings, lists or tables. This module turns the rebuilt lines into the
//! coarsest structural reading that geometry can support, and every inference is
//! reported by the importer as a loss marker rather than presented as authored
//! semantics.

use document_ir::PageSetup;

use super::types::{ExtractedLine, PageItem, PdfImage};

/// A gap wider than eight tenths of an em between two runs is a cell boundary.
///
/// Word gaps are a fifth of an em and justified text stays below four tenths,
/// so a wider gap is an explicit column separator rather than a space.
const TABLE_GAP_EM: f32 = 0.8;

/// Heading text longer than this many characters is a paragraph in a large font.
const HEADING_MAX_CHARS: usize = 120;

/// Font sizes at least this much larger than the body size are heading sizes.
const HEADING_SIZE_RATIO: f32 = 1.12;

/// Document-wide font sizes used to rank headings and group paragraphs.
pub(crate) struct StyleProfile {
    body_size: f32,
    /// Heading sizes in descending order, so index zero is level one.
    heading_sizes: Vec<f32>,
}

impl StyleProfile {
    pub(crate) fn from_pages(pages: &[Vec<PageItem>]) -> Self {
        let mut weights = std::collections::BTreeMap::<i32, usize>::new();
        let mut sizes = Vec::new();
        for page in pages {
            for item in page {
                let PageItem::Line(line) = item
                else {
                    continue;
                };
                if line.size <= 0.0 {
                    continue;
                }
                sizes.push(line.size);
                let key = (line.size * 10.0).round() as i32;
                *weights.entry(key).or_default() += line.text.chars().filter(|character| !character.is_whitespace()).count();
            }
        }
        let body_size =
            weights.into_iter().max_by(|left, right| left.1.cmp(&right.1)).map(|(size, _)| size as f32 / 10.0).unwrap_or(10.0);
        let mut heading_sizes = Vec::new();
        sizes.sort_by(|left, right| right.partial_cmp(left).unwrap_or(std::cmp::Ordering::Equal));
        for size in sizes {
            if size < body_size * HEADING_SIZE_RATIO {
                continue;
            }
            if heading_sizes.iter().any(|known: &f32| (*known - size).abs() < 0.5) {
                continue;
            }
            heading_sizes.push(size);
        }
        heading_sizes.truncate(6);
        Self { body_size, heading_sizes }
    }

    fn heading_level(&self, size: f32) -> Option<u8> {
        self.heading_sizes.iter().position(|heading| (heading - size).abs() < 0.5).map(|index| (index + 1) as u8)
    }

    /// Body text size in points, measured from glyph sizes alone.
    pub(crate) fn body_size(&self) -> f32 {
        self.body_size
    }
}

/// One list item with nested lists below it.
pub(crate) struct FlowItem {
    pub(crate) text: String,
    pub(crate) number: Option<u32>,
    pub(crate) children: Vec<FlowList>,
}

/// A flat round of list items; nesting lives in `children`.
pub(crate) struct FlowList {
    pub(crate) ordered: bool,
    pub(crate) items: Vec<FlowItem>,
}

/// Block-shaped output of the layout pass.
pub(crate) enum FlowBlock {
    Section { level: u8, text: String },
    Paragraph { text: String },
    List(FlowList),
    Table { rows: Vec<Vec<String>> },
    NamedImage(String),
    InlineImage(PdfImage),
}

/// Split one page's ordered items into flow blocks.
///
/// Images interrupt the text flow, so each contiguous run of lines is laid out
/// on its own and never grouped across an image.
pub(crate) fn page_flow(items: &[PageItem], profile: &StyleProfile) -> Vec<FlowBlock> {
    let mut blocks = Vec::new();
    let mut paragraph: Option<Paragraph> = None;
    let mut lines: Vec<ExtractedLine> = Vec::new();
    for item in items {
        match item {
            PageItem::Line(line) => lines.push(line.clone()),
            PageItem::NamedImage(name) => {
                flush_paragraph(&mut blocks, &mut paragraph);
                layout_lines(&mut blocks, &mut paragraph, &lines, profile);
                lines.clear();
                blocks.push(FlowBlock::NamedImage(name.clone()));
            }
            PageItem::InlineImage(image) => {
                flush_paragraph(&mut blocks, &mut paragraph);
                layout_lines(&mut blocks, &mut paragraph, &lines, profile);
                lines.clear();
                blocks.push(FlowBlock::InlineImage(image.clone()));
            }
        }
    }
    layout_lines(&mut blocks, &mut paragraph, &lines, profile);
    flush_paragraph(&mut blocks, &mut paragraph);
    blocks
}

fn layout_lines(
    blocks: &mut Vec<FlowBlock>,
    paragraph: &mut Option<Paragraph>,
    lines: &[ExtractedLine],
    profile: &StyleProfile,
) {
    let leading = estimate_leading(lines, profile.body_size);
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        if let Some(level) = profile.heading_level(line.size)
            && line.text.chars().count() <= HEADING_MAX_CHARS
            && line.text.chars().any(char::is_alphabetic)
        {
            flush_paragraph(blocks, paragraph);
            blocks.push(FlowBlock::Section { level, text: line.text.clone() });
            index += 1;
            continue;
        }
        if list_marker(line).is_some() {
            flush_paragraph(blocks, paragraph);
            let (list, consumed) = collect_list(&lines[index..], profile.body_size);
            blocks.push(FlowBlock::List(list));
            index += consumed;
            continue;
        }
        if let Some((rows, consumed, header_start)) = table_at(&lines[index..]) {
            flush_paragraph(blocks, paragraph);
            for line in &lines[index..index + header_start] {
                append_line(paragraph, line, profile.body_size, leading, blocks);
            }
            flush_paragraph(blocks, paragraph);
            blocks.push(FlowBlock::Table { rows });
            index += consumed;
            continue;
        }
        append_line(paragraph, line, profile.body_size, leading, blocks);
        index += 1;
    }
}

struct Paragraph {
    text: String,
    size: f32,
    y: Option<f32>,
}

fn flush_paragraph(blocks: &mut Vec<FlowBlock>, paragraph: &mut Option<Paragraph>) {
    if let Some(paragraph) = paragraph.take()
        && !paragraph.text.is_empty()
    {
        blocks.push(FlowBlock::Paragraph { text: paragraph.text });
    }
}

fn append_line(
    paragraph: &mut Option<Paragraph>,
    line: &ExtractedLine,
    body_size: f32,
    leading: f32,
    blocks: &mut Vec<FlowBlock>,
) {
    let Some(current) = paragraph.as_mut()
    else {
        *paragraph = Some(Paragraph { text: line.text.clone(), size: line.size, y: line.y });
        return;
    };
    if paragraph_break(current, line, body_size, leading) {
        flush_paragraph(blocks, paragraph);
        *paragraph = Some(Paragraph { text: line.text.clone(), size: line.size, y: line.y });
        return;
    }
    if current.text.ends_with('-') {
        current.text.push_str(&line.text);
    }
    else {
        current.text.push(' ');
        current.text.push_str(&line.text);
    }
    current.size = current.size.max(line.size);
    current.y = line.y.or(current.y);
}

fn paragraph_break(current: &Paragraph, line: &ExtractedLine, body_size: f32, leading: f32) -> bool {
    if (current.size - line.size).abs() > body_size * 0.12 && line.size > 0.0 && current.size > 0.0 {
        return true;
    }
    let (Some(previous_y), Some(y)) = (current.y, line.y)
    else {
        return false;
    };
    // Page space can be flipped, so the distance is compared without a sign.
    // A large jump in either direction leaves the run and starts a new one.
    (previous_y - y).abs() > leading * 1.45
}

/// Median baseline gap between consecutive lines of the body size.
fn estimate_leading(lines: &[ExtractedLine], body_size: f32) -> f32 {
    let gaps = leading_gaps(lines, body_size);
    if gaps.is_empty() {
        return body_size * 1.3;
    }
    let mut gaps = gaps;
    gaps.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    gaps[gaps.len() / 2]
}

/// Baseline gaps between consecutive body lines on one page.
fn leading_gaps(lines: &[ExtractedLine], body_size: f32) -> Vec<f32> {
    let mut gaps = Vec::new();
    for pair in lines.windows(2) {
        let (previous, next) = (&pair[0], &pair[1]);
        let (Some(previous_y), Some(y)) = (previous.y, next.y)
        else {
            continue;
        };
        let gap = (previous_y - y).abs();
        if gap > 0.0 && gap < body_size * 3.0 && (previous.size - body_size).abs() < 0.6 && (next.size - body_size).abs() < 0.6
        {
            gaps.push(gap);
        }
    }
    gaps
}

/// Median body baseline distance across every page.
///
/// Page boundaries never contribute a gap, because a line at the top of one
/// page is not one leading below the last line of the previous page.
fn document_leading(pages: &[Vec<PageItem>], body_size: f32) -> f32 {
    let mut gaps = Vec::new();
    for page in pages {
        let lines = page
            .iter()
            .filter_map(|item| match item {
                PageItem::Line(line) => Some(line.clone()),
                PageItem::NamedImage(_) | PageItem::InlineImage(_) => None,
            })
            .collect::<Vec<_>>();
        gaps.extend(leading_gaps(&lines, body_size));
    }
    if gaps.is_empty() {
        return body_size * 1.3;
    }
    gaps.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    gaps[gaps.len() / 2]
}

/// Page setup measured from the text layer and the page boxes.
///
/// The values are measurements, not authored facts: the page box comes from
/// `/MediaBox`, margins come from the body text bounding box, and body size and
/// line height come from the glyph sizes and baseline gaps. Column count stays
/// unset because the text layer does not declare it.
pub(crate) fn page_setup(pages: &[Vec<PageItem>], profile: &StyleProfile, boxes: &[(f64, f64)]) -> PageSetup {
    let body_size = profile.body_size();
    let mut setup = PageSetup { base_font_size: Some(f64::from(body_size)), ..PageSetup::default() };
    let leading = document_leading(pages, body_size);
    if leading > 0.0 && body_size > 0.0 {
        setup.line_height = Some(f64::from(leading / body_size));
    }
    let Some(&(width, height)) = boxes.first()
    else {
        return setup;
    };
    setup.page_width = Some(width);
    setup.page_height = Some(height);
    let lines = pages
        .iter()
        .flat_map(|page| page.iter())
        .filter_map(|item| match item {
            PageItem::Line(line) => Some(line),
            PageItem::NamedImage(_) | PageItem::InlineImage(_) => None,
        })
        .filter(|line| (line.size - body_size).abs() < 0.6)
        .collect::<Vec<_>>();
    let left = lines.iter().filter_map(|line| line.x).reduce(f32::min);
    if let Some(left) = left.filter(|left| *left > 0.0) {
        setup.margin_left = Some(f64::from(left));
    }
    let top = lines.iter().filter_map(|line| line.y).reduce(f32::max);
    if let Some(top) = top.filter(|top| *top > 0.0) {
        let margin = height - f64::from(top) - f64::from(body_size);
        setup.margin_top = (margin > 0.0).then_some(margin);
    }
    let right = lines.iter().flat_map(|line| line.runs.iter()).filter_map(|run| run.end).reduce(f32::max);
    if let Some(right) = right.filter(|right| *right > 0.0) {
        let margin = width - f64::from(right);
        setup.margin_right = (margin > 0.0).then_some(margin);
    }
    let bottom = lines.iter().filter_map(|line| line.y).reduce(f32::min);
    if let Some(bottom) = bottom.filter(|bottom| *bottom > 0.0) {
        let margin = f64::from(bottom) - f64::from(body_size) * 0.2;
        setup.margin_bottom = (margin > 0.0).then_some(margin);
    }
    setup
}

struct ListEntry {
    ordered: bool,
    number: Option<u32>,
    text: String,
    x: f32,
}

fn list_marker(line: &ExtractedLine) -> Option<(bool, Option<u32>, String)> {
    let text = line.text.trim_start();
    let mut characters = text.chars();
    let first = characters.next()?;
    if "•·‣▪◦●–—⁃".contains(first) {
        let rest = characters.as_str();
        if let Some(rest) = rest.strip_prefix([' ', '\t']) {
            return Some((false, None, rest.trim_start().to_owned()));
        }
        return None;
    }
    if first == '-' && characters.as_str().starts_with([' ', '\t']) {
        return Some((false, None, characters.as_str().trim_start().to_owned()));
    }
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = &text[digits..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
    let rest = rest.strip_prefix([' ', '\t'])?;
    let number = text[..digits].parse().ok();
    Some((true, number, rest.trim_start().to_owned()))
}

fn collect_list(lines: &[ExtractedLine], body_size: f32) -> (FlowList, usize) {
    let mut entries = Vec::new();
    let mut index = 0;
    let mut base_x = 0.0;
    while index < lines.len() {
        let line = &lines[index];
        let Some((ordered, number, text)) = list_marker(line)
        else {
            break;
        };
        if text.is_empty() {
            break;
        }
        if entries.is_empty() {
            base_x = line.x.unwrap_or(0.0);
        }
        let mut text = text;
        let mut y = line.y;
        entries.push(ListEntry { ordered, number, text: String::new(), x: line.x.unwrap_or(base_x) });
        index += 1;
        // A wrapped continuation is indented past the marker column and carries
        // no marker. A dedented line leaves the list.
        while let Some(next) = lines.get(index) {
            if list_marker(next).is_some() {
                break;
            }
            let Some(x) = next.x
            else {
                break;
            };
            if x < base_x {
                break;
            }
            if let (Some(previous_y), Some(next_y)) = (y, next.y)
                && (previous_y - next_y).abs() > body_size.max(next.size).max(1.0) * 1.8
            {
                break;
            }
            text.push(' ');
            text.push_str(&next.text);
            y = next.y.or(y);
            index += 1;
        }
        if let Some(entry) = entries.last_mut() {
            entry.text = text;
        }
    }
    let ordered = entries.first().map(|entry| entry.ordered).unwrap_or(false);
    let tolerance = (body_size * 0.8).max(1.0);
    (build_list(&entries, ordered, tolerance), index)
}

fn build_list(entries: &[ListEntry], ordered: bool, tolerance: f32) -> FlowList {
    let base_x = entries.first().map(|entry| entry.x).unwrap_or(0.0);
    let mut items = Vec::new();
    let mut index = 0;
    while index < entries.len() {
        let entry = &entries[index];
        let mut children = Vec::new();
        index += 1;
        if index < entries.len() && entries[index].x > base_x + tolerance {
            let start = index;
            while index < entries.len() && entries[index].x > base_x + tolerance {
                index += 1;
            }
            let nested_ordered = entries[start].ordered;
            children.push(build_list(&entries[start..index], nested_ordered, tolerance));
        }
        items.push(FlowItem { text: entry.text.clone(), number: entry.number, children });
    }
    FlowList { ordered, items }
}

/// Recognise table rows and return their cells plus how many lines were used.
///
/// The return value is `(rows, consumed, header_start)`: `header_start` is the
/// offset of the first table row inside `lines`, so the caller can keep the
/// lines above it in the paragraph flow. The table anchors on the first data
/// row, whose cells are all filled. Rows above the anchor stay part of the
/// table when they sit within the row distance the table layout uses, split
/// into at least two cells, and carry no more cells than the anchor row: a
/// merged header spans several columns, so its row leaves cells empty where a
/// span swallows the column text. A plain paragraph above the header either
/// splits into fewer than two cells or keeps its distance, and stays a
/// paragraph.
fn table_at(lines: &[ExtractedLine]) -> Option<(Vec<Vec<String>>, usize, usize)> {
    let anchor = lines.iter().position(|line| data_cells(line).is_some())?;
    let width = data_cells(&lines[anchor])?.len();
    let mut start = anchor;
    while start > 0 {
        let above = &lines[start - 1];
        let below = &lines[start];
        let Some(cells) = header_cells(above)
        else {
            break;
        };
        if cells.len() > width {
            break;
        }
        if let (Some(above_y), Some(below_y)) = (above.y, below.y)
            && (above_y - below_y).abs() > above.size.max(below.size).max(1.0) * 2.4
        {
            break;
        }
        start -= 1;
    }
    let mut rows =
        lines[start..=anchor].iter().filter_map(|line| header_cells(line).or_else(|| data_cells(line))).collect::<Vec<_>>();
    let mut index = anchor + 1;
    while let Some(line) = lines.get(index) {
        let Some(cells) = data_cells(line)
        else {
            break;
        };
        let previous = &lines[index - 1];
        if let (Some(previous_y), Some(y)) = (previous.y, line.y)
            && (previous_y - y).abs() > previous.size.max(line.size).max(1.0) * 2.4
        {
            break;
        }
        rows.push(cells);
        index += 1;
    }
    (rows.len() >= 2).then_some((rows, index, start))
}

/// Cells of a data row: every column carries its own text.
fn data_cells(line: &ExtractedLine) -> Option<Vec<String>> {
    let cells = split_cells(line);
    (cells.len() >= 2 && cells.iter().all(|cell| !cell.is_empty())).then_some(cells)
}

/// Cells of a merged header row: spans swallow columns, so empty cells appear
/// where a span covers them, and at least one cell still carries text.
fn header_cells(line: &ExtractedLine) -> Option<Vec<String>> {
    let cells = split_cells(line);
    (cells.len() >= 2 && cells.iter().any(|cell| !cell.is_empty())).then_some(cells)
}

fn split_cells(line: &ExtractedLine) -> Vec<String> {
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut previous_end: Option<f32> = None;
    for run in &line.runs {
        if let (Some(x), Some(end)) = (run.x, previous_end)
            && x - end >= TABLE_GAP_EM * line.size.max(1.0)
        {
            cells.push(current.trim().to_owned());
            current.clear();
        }
        current.push_str(&run.text);
        previous_end = run.end;
    }
    cells.push(current.trim().to_owned());
    cells
}
