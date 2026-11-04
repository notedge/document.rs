//! TikZ 前端冒烟：tikzpicture 降到 scene，越界命令结构化拒绝。

use plotter_input_tikz::tikz_to_scene;
use plotter_scene::NodeKind;
use plotter_types::codes;

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
