//! A2 golden：point / bar / text 的 Scene + CPU/SVG 回归。
//!
//! Checked-in SVG / fingerprint goldens live in apollo.rs product fixtures
//! (`APOLLO_FIXTURES/plotter-golden`). Without that host root this file still
//! asserts scene shape; it never writes goldens back into this crate.

use std::{fs, path::PathBuf};

use apollo::{
    AestheticExpr, ColumnTable, CompileOptions, LayerSpec, Mapping, PlotSpec, SceneNodeKind, compile_plot, render_rgba8,
    render_svg,
};

fn golden_dir() -> Option<PathBuf> {
    std::env::var_os("APOLLO_FIXTURES").map(|root| PathBuf::from(root).join("plotter-golden"))
}

fn read_or_write_golden(name: &str, actual: &str) {
    let Some(dir) = golden_dir()
    else {
        return;
    };
    let path = dir.join(name);
    if std::env::var("APOLLO_UPDATE_GOLDEN").ok().as_deref() == Some("1") {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|err| panic!("missing host golden {}: {err}", path.display()));
    assert_eq!(expected.replace("\r\n", "\n"), actual.replace("\r\n", "\n"), "golden mismatch: {name}");
}

fn assert_cpu_fingerprint(name: &str, fingerprint: u64) {
    let Some(dir) = golden_dir()
    else {
        return;
    };
    let path = dir.join(format!("{name}.fingerprint"));
    let actual = format!("{fingerprint:#018x}\n");
    if std::env::var("APOLLO_UPDATE_GOLDEN").ok().as_deref() == Some("1") {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap();
    assert_eq!(expected, actual, "cpu fingerprint mismatch: {name}");
}

#[test]
fn golden_point() {
    let table = ColumnTable::new().push_float("x", vec![0.0, 1.0, 2.0]).unwrap().push_float("y", vec![1.0, 2.0, 1.5]).unwrap();
    let plot = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_point());
    let scene = compile_plot(&plot, CompileOptions::golden()).unwrap();
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Points(_))));

    let svg = render_svg(&scene).unwrap();
    assert!(svg.contains("<circle"));
    read_or_write_golden("point.svg", &svg);

    let image = render_rgba8(&scene).unwrap();
    assert!(image.non_white_count() > 0);
    assert_cpu_fingerprint("point", image.fingerprint());
}

#[test]
fn golden_bar() {
    let table = ColumnTable::new().push_float("x", vec![0.0, 1.0, 2.0]).unwrap().push_float("y", vec![1.0, 2.0, 1.5]).unwrap();
    let plot = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_bar());
    let scene = compile_plot(&plot, CompileOptions::golden()).unwrap();
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Mesh(_))));

    let svg = render_svg(&scene).unwrap();
    assert!(svg.contains("<polygon"));
    read_or_write_golden("bar.svg", &svg);

    let image = render_rgba8(&scene).unwrap();
    assert!(image.non_white_count() > 0);
    assert_cpu_fingerprint("bar", image.fingerprint());
}

#[test]
fn golden_text() {
    let table = ColumnTable::new()
        .push_float("x", vec![0.0, 1.0, 2.0])
        .unwrap()
        .push_float("y", vec![1.0, 2.0, 1.5])
        .unwrap()
        .push_string("lab", vec!["a".into(), "b".into(), "c".into()])
        .unwrap();
    let mut mapping = Mapping::xy("x", "y");
    mapping.label = Some(AestheticExpr::column("lab"));
    let plot = PlotSpec::new(table).mapping(mapping).layer(LayerSpec::geom_text());
    let scene = compile_plot(&plot, CompileOptions::golden()).unwrap();
    assert!(scene.nodes.nodes().iter().any(|n| matches!(n.kind, SceneNodeKind::Text(_))));

    let svg = render_svg(&scene).unwrap();
    assert!(svg.contains("<text"));
    assert!(svg.contains(">a</text>"));
    read_or_write_golden("text.svg", &svg);

    let image = render_rgba8(&scene).unwrap();
    assert!(image.non_white_count() > 0);
    assert_cpu_fingerprint("text", image.fingerprint());
}

#[test]
fn golden_line_still_works() {
    let table = ColumnTable::new().push_float("x", vec![0.0, 1.0, 2.0]).unwrap().push_float("y", vec![0.0, 1.0, 0.5]).unwrap();
    let plot = PlotSpec::new(table).mapping(Mapping::xy("x", "y")).layer(LayerSpec::geom_line());
    let scene = compile_plot(&plot, CompileOptions::golden()).unwrap();
    let svg = render_svg(&scene).unwrap();
    read_or_write_golden("line.svg", &svg);
    let image = render_rgba8(&scene).unwrap();
    assert_cpu_fingerprint("line", image.fingerprint());
}
