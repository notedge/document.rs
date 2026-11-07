//! Host font installation shared by the math integration tests.
//!
//! `build.rs` only enables the font-dependent tests when a pack exists, so a
//! pack missing here means the environment changed after the build. That is a
//! real error and is reported with the same instructions, not silently skipped.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use document_fonts::{MathFontPack, install_math_fonts};

static INSTALLED: OnceLock<()> = OnceLock::new();

/// `VUTEX_FONT_DIR` when set, otherwise the VuTeX checkout this workspace pins.
fn font_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("VUTEX_FONT_DIR") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("workspace root")
        .join("VuTeX")
        .join("projects")
        .join("vutex.rs")
        .join("fonts")
}

/// Install the host math font pack once for this test binary.
pub fn math_fonts() {
    INSTALLED.get_or_init(|| {
        let dir = font_dir();
        let pack = MathFontPack::load_from(&dir).unwrap_or_else(|error| {
            panic!(
                "math font pack at `{}` is unusable: {error}\n\
                 point `VUTEX_FONT_DIR` at `VuTeX/projects/vutex.rs/fonts` and rebuild",
                dir.display()
            )
        });
        install_math_fonts(&pack);
    });
}
