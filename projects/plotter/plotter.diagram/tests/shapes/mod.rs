//! 形状与线型合同：每个新变体落到自己的场景几何，dash 不再降级为实线。

use plotter_diagram::{
    compile::{LayoutOptions, compile_layered},
    render::render,
    spec::{DiagramEdge, DiagramNode, DiagramSpec, Direction, EdgeStyle, Endpoint, NodeShape},
};
use plotter_scene::{SceneResolver, SemanticKind, SemanticNode, SemanticScene};

fn node(id: &str, shape: NodeShape) -> DiagramNode {
    DiagramNode { id: id.to_string(), label: None, shape, ports: Vec::new(), cluster: None }
}

fn spec(nodes: Vec<DiagramNode>, edges: Vec<DiagramEdge>) -> DiagramSpec {
    let mut spec = DiagramSpec::new(Direction::TopDown);
    spec.nodes = nodes;
    spec.edges = edges;
    spec
}

fn edge(from: &str, to: &str, style: EdgeStyle) -> DiagramEdge {
    DiagramEdge {
        from: Endpoint { node: from.to_string(), port: None },
        to: Endpoint { node: to.to_string(), port: None },
        label: None,
        style,
    }
}

fn scene_node<'a>(scene: &'a SemanticScene, id: &str) -> &'a SemanticNode {
    fn find<'a>(nodes: &'a [SemanticNode], id: &str) -> Option<&'a SemanticNode> {
        for node in nodes {
            if node.id.as_deref() == Some(id) {
                return Some(node);
            }
            if let SemanticKind::Group { children, .. } = &node.kind
                && let Some(found) = find(children, id)
            {
                return Some(found);
            }
        }
        None
    }
    find(&scene.nodes, id).unwrap_or_else(|| panic!("scene has no `{id}`"))
}

#[test]
fn every_native_shape_renders_its_own_geometry() {
    let cases = [
        ("hex", NodeShape::Hexagon, "path"),
        ("tri", NodeShape::Triangle, "path"),
        ("star", NodeShape::Star, "path"),
        ("trap", NodeShape::Trapezium, "path"),
        ("para", NodeShape::Parallelogram, "path"),
        ("note", NodeShape::Note, "group"),
        ("point", NodeShape::Point, "ellipse"),
        ("dbl", NodeShape::DoubleCircle, "group"),
        ("comp", NodeShape::Component, "group"),
        ("ell", NodeShape::Ellipse, "ellipse"),
        ("plain", NodeShape::Plaintext, "plaintext"),
        ("cyl", NodeShape::Cylinder, "group"),
    ];
    let nodes: Vec<DiagramNode> = cases.iter().map(|(id, shape, _)| node(id, *shape)).collect();
    let spec = spec(nodes, Vec::new());
    let plan = compile_layered(&spec, &LayoutOptions::default());
    assert!(
        plan.diagnostics.iter().all(|record| record.code().as_str() != plotter_scene::codes::APPROXIMATED),
        "native shapes must not be approximated: {:?}",
        plan.diagnostics
    );

    let scene = render(&plan);
    for (id, _, expected) in cases {
        let rendered = scene_node(&scene, &format!("node-{id}"));
        match (expected, &rendered.kind) {
            ("path", SemanticKind::Path { .. }) => {}
            ("ellipse", SemanticKind::Ellipse { .. }) => {}
            ("group", SemanticKind::Group { .. }) => {}
            ("plaintext", SemanticKind::Rect { style, .. }) => {
                assert!(style.fill.is_none() && style.stroke.is_none(), "plaintext must have no paint");
            }
            (expected, actual) => panic!("`{id}` should render a {expected}, found {actual:?}"),
        }
    }

    let note = scene_node(&scene, "node-note");
    match &note.kind {
        SemanticKind::Group { children, .. } => {
            assert!(children.iter().any(|child| child.id.as_deref() == Some("node-note-fold")));
        }
        other => panic!("note should be a group, found {other:?}"),
    }

    assert!(
        scene.nodes.iter().all(|node| node.id.as_deref() != Some("node-point-label")),
        "point nodes carry no label"
    );
    let svg = scene.resolve(&SceneResolver::new()).to_svg().expect("scene should serialize");
    assert!(svg.contains("style=\"fill:none;stroke:none\""), "plaintext must beat the role palette: {svg}");
}

#[test]
fn dash_and_bold_styles_reach_the_scene_surface() {
    let nodes = ["a", "b", "c", "d", "e"].map(|id| node(id, NodeShape::Box)).to_vec();
    let spec = spec(
        nodes,
        vec![
            edge("a", "b", EdgeStyle::DashedArrow),
            edge("b", "c", EdgeStyle::DottedArrow),
            edge("c", "d", EdgeStyle::BoldArrow),
            edge("d", "e", EdgeStyle::DashedLine),
        ],
    );
    let plan = compile_layered(&spec, &LayoutOptions::default());
    assert!(
        plan.diagnostics.iter().all(|record| record.code().as_str() != plotter_scene::codes::APPROXIMATED),
        "dash and bold styles are part of the contract: {:?}",
        plan.diagnostics
    );

    let scene = render(&plan);
    for (index, expected_dash, expected_width) in [(0, Some([3.0, 3.0]), 1.0), (1, Some([1.0, 3.0]), 1.0), (2, None, 2.5)] {
        let line = scene_node(&scene, &format!("edge-{index}-line"));
        match &line.kind {
            SemanticKind::Path { style, .. } => {
                assert_eq!(style.dash, expected_dash, "edge {index} dash");
                assert_eq!(style.stroke_width, expected_width, "edge {index} width");
            }
            other => panic!("edge {index} should be a path, found {other:?}"),
        }
    }

    let svg = scene.resolve(&SceneResolver::new()).to_svg().expect("scene should serialize");
    assert!(svg.contains("stroke-dasharray=\"3 3\""), "{svg}");
    assert!(svg.contains("stroke-dasharray=\"1 3\""), "{svg}");
}
