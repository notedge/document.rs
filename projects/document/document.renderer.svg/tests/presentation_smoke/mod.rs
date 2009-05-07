use document_layout::{LayoutPage, LayoutPlan};
use document_presentation::build_display_presentation;
use document_renderer_svg::render_svg;

#[test]
fn empty_plan_renders_svg_root() {
    let plan = LayoutPlan { pages: vec![LayoutPage { width: 595.0, height: 842.0, items: Vec::new() }] };
    let presentation = build_display_presentation(&plan);
    let svg = render_svg(&presentation).expect("svg render");
    assert!(svg.contains("<svg"));
}
