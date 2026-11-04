use plotter_input_tikz::tikz_to_scene;
use plotter_scene::{Color, DisplayScene, NodeKind, Point, TextAnchor};
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
fn a_bare_body_lowers_without_the_environment() {
    let source = "\\draw[red] (0,0) rectangle (1,1);\n";
    let scene = tikz_to_scene(source).expect("a bare body is accepted");

    assert_eq!(scene.nodes.len(), 1);
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

#[test]
fn an_empty_picture_is_refused_instead_of_fitting_a_blank_scene() {
    let source = "\\begin{tikzpicture}\n\\end{tikzpicture}\n";
    let error = tikz_to_scene(source).expect_err("an empty picture paints no geometry");

    assert_eq!(error.code(), codes::UNSUPPORTED);
    assert!(error.span().is_some());
}

/// The only label under the scene's root group.
fn only_label(scene: &DisplayScene) -> (&str, Point, &plotter_scene::TextStyle) {
    let mut labels = Vec::new();
    for node in &scene.nodes {
        if let NodeKind::Group { children, .. } = &node.kind {
            for child in children {
                if let NodeKind::Text { position, text, style } = &child.kind {
                    labels.push((text.as_str(), *position, style));
                }
            }
        }
    }
    assert_eq!(labels.len(), 1, "expected exactly one label");
    labels[0]
}

#[test]
fn a_standalone_node_is_placed_at_its_coordinate() {
    let source = "\\node at (0,0) {label};\n";
    let scene = tikz_to_scene(source).expect("a standalone node is inside the subset");
    let (text, position, _) = only_label(&scene);

    assert_eq!(text, "label");
    assert!(position.x.abs() < 1e-6, "got {position:?}");
    assert!((position.y - 3.6).abs() < 1e-6, "got {position:?}");
}

#[test]
fn node_anchor_options_move_the_label_around_its_point() {
    for option in ["above", "below", "left", "right"] {
        let source = format!("\\node[{option}, text=red] at (0,0) {{label}};\n");
        let scene = tikz_to_scene(&source).expect("a node anchor is inside the subset");
        let (_, position, style) = only_label(&scene);

        assert_eq!(style.fill, Some(Color::rgb(255, 0, 0)));
        match option {
            "above" => {
                assert!(position.y < 0.0, "got {position:?}");
                assert_eq!(style.text_anchor, TextAnchor::Middle);
            }
            "below" => {
                assert!(position.y > 0.0, "got {position:?}");
                assert_eq!(style.text_anchor, TextAnchor::Middle);
            }
            "left" => {
                assert!(position.x < 0.0, "got {position:?}");
                assert_eq!(style.text_anchor, TextAnchor::End);
            }
            "right" => {
                assert!(position.x > 0.0, "got {position:?}");
                assert_eq!(style.text_anchor, TextAnchor::Start);
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn standard_colour_names_reach_the_svg_stroke() {
    let cases = [
        ("white", "#ffffff"),
        ("black", "#000000"),
        ("red", "#ff0000"),
        ("green", "#00ff00"),
        ("blue", "#0000ff"),
        ("purple", "#bf0040"),
        ("orange", "#ff8000"),
        ("yellow", "#ffff00"),
        ("gray", "#808080"),
    ];
    for (name, css) in cases {
        for source in [format!("\\draw[{name}] (0,0) -- (1,1);\n"), format!("\\draw[color={name}] (0,0) -- (1,1);\n")] {
            let scene = tikz_to_scene(&source).expect("a standard colour is inside the subset");
            let svg = scene.to_svg().expect("a fitted scene serializes to SVG");
            assert!(svg.contains(&format!("stroke=\"{css}\"")), "{name}: {svg}");
        }
    }
}

#[test]
fn a_node_without_a_position_is_refused_instead_of_guessing_one() {
    let source = "\\node {label};\n";
    let error = tikz_to_scene(source).expect_err("a node without `at` has no position");

    assert_eq!(error.code(), codes::UNSUPPORTED);
    let span = error.span().expect("the refusal names the command");
    let text = &source[span];
    assert!(text.starts_with("\\node"), "got {text:?}");
}
