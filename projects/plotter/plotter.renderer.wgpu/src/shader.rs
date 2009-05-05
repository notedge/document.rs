//! 内建折线 shader（A4 首切片）。

/// 视口 uniform + 线段顶点着色。
pub const LINE_SHADER: &str = r#"
struct Uniforms {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    let x = (input.position.x / uniforms.viewport.x) * 2.0 - 1.0;
    let y = (input.position.y / uniforms.viewport.y) * 2.0 - 1.0;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    return input.color;
}
"#;
