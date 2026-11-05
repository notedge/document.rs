#![allow(missing_docs)]

use plotter_data::{TreeData, TreeNode};
use plotter_layout::{LayoutOptions, RadialTreeLayout, TidyTreeLayout, TreeLayout};

fn sample_tree() -> TreeData {
    TreeData::new(
        "r",
        vec![
            TreeNode::root("r"),
            TreeNode::child("a", "r"),
            TreeNode::child("b", "r"),
            TreeNode::child("c", "a"),
            TreeNode::child("d", "a"),
        ],
    )
    .unwrap()
}

#[test]
fn tidy_is_deterministic_and_root_above_leaves() {
    let tree = sample_tree();
    let opt = LayoutOptions::new(200.0, 200.0, 20.0);
    let a = TidyTreeLayout.layout(&tree, &opt).unwrap();
    let b = TidyTreeLayout.layout(&tree, &opt).unwrap();
    assert_eq!(a, b);
    let yr = a.position_of("r").unwrap().y;
    let yc = a.position_of("c").unwrap().y;
    assert!(yr > yc);
}

#[test]
fn radial_places_root_near_center() {
    let tree = sample_tree();
    let opt = LayoutOptions::new(200.0, 200.0, 20.0);
    let result = RadialTreeLayout.layout(&tree, &opt).unwrap();
    let root = result.position_of("r").unwrap();
    assert!((root.x - 100.0).abs() < 1.0);
    assert!((root.y - 100.0).abs() < 1.0);
}
