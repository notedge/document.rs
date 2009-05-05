#![allow(missing_docs)]

use plotter_types::plot::{InteractionId, Rgba, Vec3};
use plotter_view::{CameraSpec, Mesh3Node, Scene, SceneArena, SceneNodeKind, Viewport, pick_at};

#[test]
fn picks_unit_triangle() {
    let mut arena = SceneArena::new();
    let mesh = arena.insert(SceneNodeKind::Mesh3(Mesh3Node {
        positions: vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)],
        indices: vec![0, 1, 2],
        fill: Rgba::BLACK,
        interaction: Some(InteractionId(7)),
    }));
    let root = arena.insert(SceneNodeKind::Group { children: vec![mesh] });
    let scene = Scene {
        root,
        nodes: arena,
        camera: CameraSpec::Perspective {
            eye: Vec3::new(0.3, 0.3, 3.0),
            target: Vec3::new(0.3, 0.3, 0.0),
            up: Vec3::Y,
            fovy_degrees: 45.0,
            near: 0.1,
            far: 100.0,
        },
        viewport: Viewport::new(200.0, 200.0),
        metadata: Default::default(),
    };
    let hit = pick_at(&scene, 100.0, 100.0).unwrap().expect("expected hit");
    assert_eq!(hit.interaction, InteractionId(7));
    assert!(hit.world_position.is_some());
}
