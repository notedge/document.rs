//! 冒烟入口：只保留 HTML 后端的最小渲染断言。

mod diagram_snapshot;

mod document_html;
mod html_style;
mod math_reflow;

/// The reflow formula tests are ignored when no font pack was found at build
/// time. Their absence is visible in the run summary as `ignored`, and a gated
/// run can turn it into a failure so the suite never passes without executing
/// them.
#[test]
fn math_font_pack_presence_is_reported() {
    if cfg!(math_font_pack) {
        return;
    }
    let message = "5 reflow formula tests are ignored: no math font pack at build time. \
                   Point `VUTEX_FONT_DIR` at `VuTeX/projects/vutex.rs/fonts` and rebuild";
    if matches!(std::env::var("VUTEX_REQUIRE_MATH_FONTS").as_deref(), Ok("1") | Ok("true")) {
        panic!("{message}");
    }
    println!("SKIP[math-font] {message}");
}
