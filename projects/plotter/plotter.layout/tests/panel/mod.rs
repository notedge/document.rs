#![allow(missing_docs)]

use plotter_layout::{PanelRect, layout_facet_panels, layout_single_panel};

#[test]
fn single_panel_respects_margin() {
    let panel = layout_single_panel(200.0, 150.0, (24.0, 20.0, 12.0, 12.0)).unwrap();
    assert_eq!(panel, PanelRect::new(24.0, 20.0, 188.0, 138.0));
}

#[test]
fn two_panels_side_by_side() {
    let panels = layout_facet_panels(100.0, 100.0, (10.0, 10.0, 10.0, 10.0), 2, Some(2), 4.0).unwrap();
    assert_eq!(panels.len(), 2);
    assert!(panels[0].left < panels[1].left);
    assert_eq!(panels[0].bottom, panels[1].bottom);
    assert!((panels[0].width() - panels[1].width()).abs() < 1e-9);
}
