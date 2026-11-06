use document_ir::{Block, DocumentGraph, TableCell, TableRow};

/// Mutable table parse state for `w:tbl`/`w:tr`/`w:tc` walks.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub(crate) struct TableState {
    pub rows: Vec<Vec<TableCell>>,
    pub current_row: Vec<TableCell>,
}

pub(crate) fn push_table_block(graph: &mut DocumentGraph, table: TableState) {
    if table.rows.is_empty() {
        return;
    }
    let mut rows = table.rows.into_iter();
    let head: Vec<TableRow> = rows.next().map(|cells| TableRow { cells }).into_iter().collect();
    let body: Vec<TableRow> = rows.map(|cells| TableRow { cells }).collect();
    graph.push_block(Block::Table { caption: None, head, body, foot: Vec::new() });
}
