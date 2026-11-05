#![allow(missing_docs)]

use plotter_data::GridData;

#[test]
fn triangulates_2x2() {
    let grid = GridData::new(vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0, 0.0, 0.0, 1.0]).unwrap();
    let (pos, idx) = grid.triangulate();
    assert_eq!(pos.len(), 4);
    assert_eq!(idx.len(), 6);
}
