//! Detect the host math font pack so the geometry tests can declare
//! themselves ignored instead of failing on a machine that has none.
//!
//! This crate ships no font bytes. The formula tests need a host pack, which
//! `VUTEX_FONT_DIR` points at. Cargo cannot skip a test at run time, but it can
//! gate the `math_font_pack` cfg here, letting the test attributes turn into
//! `#[ignore]` with an explicit reason on machines without a pack. The tests
//! still run on every machine that has one, so CI keeps covering them.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=VUTEX_FONT_DIR");
    println!("cargo:rustc-check-cfg=cfg(math_font_pack)");
    let dir = font_dir();
    // Also rerun when the pack appears or disappears under the default path.
    println!("cargo:rerun-if-changed={}", dir.display());
    if dir.is_dir() {
        println!("cargo:rustc-cfg=math_font_pack");
    }
}

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
