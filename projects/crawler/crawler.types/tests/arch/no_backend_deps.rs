//! 架构约束：types 不得依赖后端 / FFI。

#[test]
fn types_has_no_backend_deps() {
    let manifest = include_str!("../../Cargo.toml");
    assert!(!manifest.contains("yydb"), "crawler-types must not depend on yydb");
    assert!(!manifest.contains("napi"), "crawler-types must not depend on napi");
    assert!(!manifest.contains("wasm-bindgen"), "crawler-types must not depend on wasm-bindgen");
}
