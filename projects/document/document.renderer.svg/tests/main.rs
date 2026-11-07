//! 冒烟入口：只保留 SVG 后端的 presentation 闭环。

mod presentation_smoke;

mod document;
mod math;
mod support;

/// The math geometry tests are ignored when no font pack was found at build
/// time. Their absence is visible in the run summary as `ignored`, and a gated
/// run can turn it into a failure so the suite never passes without executing
/// them.
#[test]
fn math_font_pack_presence_is_reported() {
    if cfg!(math_font_pack) {
        return;
    }
    let message = "18 math geometry tests are ignored: no math font pack at build time. \
                   Point `VUTEX_FONT_DIR` at `VuTeX/projects/vutex.rs/fonts` and rebuild";
    if matches!(std::env::var("VUTEX_REQUIRE_MATH_FONTS").as_deref(), Ok("1") | Ok("true")) {
        panic!("{message}");
    }
    println!("SKIP[math-font] {message}");
}
