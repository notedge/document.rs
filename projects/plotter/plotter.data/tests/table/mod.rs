#![allow(missing_docs)]

use plotter_data::ColumnTable;
use plotter_types::plot::DiagnosticCode;

#[test]
fn builds_aligned_float_columns() {
    let table = ColumnTable::new().push_float("x", vec![1.0, 2.0, 3.0]).unwrap().push_float("y", vec![4.0, 5.0, 6.0]).unwrap();
    assert_eq!(table.row_count(), 3);
    assert!(table.validate().is_ok());
    assert_eq!(table.float_column("x").unwrap().values[1], 2.0);
}

#[test]
fn rejects_length_mismatch() {
    let err = ColumnTable::new().push_float("x", vec![1.0, 2.0]).unwrap().push_float("y", vec![3.0]).unwrap_err();
    assert_eq!(err.code, DiagnosticCode::ColumnLengthMismatch);
}
