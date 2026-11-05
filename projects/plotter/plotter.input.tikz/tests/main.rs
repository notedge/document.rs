//! TikZ 前端冒烟：tikzpicture 降到 scene，越界命令结构化拒绝。

use plotter_input_tikz::tikz_to_scene;
use plotter_scene::{Color, NodeKind};
use plotter_types::codes;

/// The children of the picture group a lowered source produced.
fn children(source: &str) -> Vec<plotter_scene::SceneNode> {
    let scene = tikz_to_scene(source).expect("source is inside the subset");
    let NodeKind::Group { children, .. } = &scene.nodes[0].kind
    else {
        panic!("a picture lowers to one root group");
    };
    children.clone()
}

#[test]
fn node_draw_strokes_a_box_around_the_label() {
    let children = children("\\begin{tikzpicture}\n\\node[draw] at (2,2) {box};\n\\end{tikzpicture}\n");

    assert_eq!(children.len(), 2, "a drawn node is a box plus its label");
    let NodeKind::Path { style, .. } = &children[0].kind
    else {
        panic!("the box paints before the label");
    };
    assert_eq!(style.stroke, Some(Color::BLACK), "bare `draw` uses the path colour");
    assert_eq!(style.fill, None, "a bare node outline has no interior paint");
    let bounds = children[0].bounds().expect("a closed box has bounds");
    // `box` is 3 chars at 12pt: 3 * 12 * 0.6 wide, plus 4pt inner sep per side.
    assert!((bounds.width() - 29.6).abs() < 0.001, "got {}", bounds.width());
    // 0.8 * 12 above the baseline and 0.2 * 12 below, plus 4pt per side.
    assert!((bounds.height() - 20.0).abs() < 0.001, "got {}", bounds.height());
    assert!(matches!(children[1].kind, NodeKind::Text { .. }), "the label follows the box");
}

#[test]
fn node_draw_colour_reaches_the_box_stroke() {
    let children = children("\\begin{tikzpicture}\n\\node[draw=red] at (0,0) {warn};\n\\end{tikzpicture}\n");

    let NodeKind::Path { style, .. } = &children[0].kind
    else {
        panic!("a drawn node starts with its box");
    };
    assert_eq!(style.stroke, Some(Color::rgb(255, 0, 0)), "`draw=red` strokes the box red");
}

#[test]
fn node_options_combine_with_draw() {
    let children =
        children("\\begin{tikzpicture}\n\\node[draw, above, text=red, scale=2] at (1,2) {tag};\n\\end{tikzpicture}\n");

    assert_eq!(children.len(), 2);
    let NodeKind::Path { style, .. } = &children[0].kind
    else {
        panic!("a drawn node starts with its box");
    };
    assert_eq!(style.stroke, Some(Color::BLACK), "`above` and `text` do not recolour the box");
    let NodeKind::Text { style: text, .. } = &children[1].kind
    else {
        panic!("a drawn node ends with its label");
    };
    assert_eq!(text.fill, Some(Color::rgb(255, 0, 0)), "`text=red` colours the label");
    assert!((text.font_size - 24.0).abs() < 0.001, "`scale=2` doubles the font size");
}

#[test]
fn a_tikzpicture_environment_lowers_to_one_scene() {
    let source = "\\begin{tikzpicture}\n\\draw (0,0) -- (2,1);\n\\end{tikzpicture}\n";
    let scene = tikz_to_scene(source).expect("a polyline is inside the subset");

    assert_eq!(scene.nodes.len(), 1);
    assert!(scene.nodes[0].bounds().is_some());
    assert!(matches!(&scene.nodes[0].kind, NodeKind::Group { .. }));
}

#[test]
fn unsupported_commands_are_refused_with_an_inline_span() {
    let source = "\\begin{tikzpicture}\n\\clip (0,0) rectangle (1,1);\n\\end{tikzpicture}\n";
    let error = tikz_to_scene(source).expect_err("clipping has no scene carrier");

    assert_eq!(error.code(), codes::UNSUPPORTED);
    let span = error.span().expect("the refusal names the command");
    let text = &source[span];
    assert!(text.starts_with("\\clip"), "got {text:?}");
}
