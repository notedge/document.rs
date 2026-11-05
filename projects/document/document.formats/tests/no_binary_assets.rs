//! Enforces the zero-binary asset boundary from `AGENTS.md` §资产边界.
//!
//! Every path returned by `git ls-files` must be text. Detection is content
//! based, not extension based. An asset `.svg` is rejected even though it is
//! text.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if dir.join(".git").exists() {
            return dir;
        }
        assert!(dir.pop(), "no `.git` above `{}`", env!("CARGO_MANIFEST_DIR"));
    }
}

fn binary_reason(path: &Path) -> Option<String> {
    let asset_svg = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    if asset_svg {
        return Some("SVG asset".to_string());
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return Some(format!("unreadable: {error}")),
    };
    if bytes.is_empty() {
        return None;
    }
    if bytes.contains(&0) {
        return Some("contains NUL".to_string());
    }
    if std::str::from_utf8(&bytes).is_err() {
        return Some("invalid UTF-8".to_string());
    }
    None
}

#[test]
fn tracked_files_are_text_only() {
    let root = repo_root();
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output()
        .expect("run `git ls-files`");
    assert!(output.status.success(), "`git ls-files` failed in {}", root.display());

    let mut hits: Vec<(u64, String, String)> = Vec::new();
    for raw in output.stdout.split(|byte| *byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let relative = String::from_utf8_lossy(raw);
        let path = root.join(relative.as_ref());
        if path.is_dir() {
            continue;
        }
        if let Some(reason) = binary_reason(&path) {
            let bytes = std::fs::metadata(&path).map(|metadata| metadata.len()).unwrap_or(0);
            hits.push((bytes, relative.to_string(), reason));
        }
    }

    if !hits.is_empty() {
        hits.sort_by(|left, right| right.0.cmp(&left.0));
        let mut report = format!("zero-binary boundary violated: {} tracked non-text files\n", hits.len());
        for (bytes, path, reason) in &hits {
            report.push_str(&format!("  {bytes:>10}  {path}  ({reason})\n"));
        }
        panic!("{report}");
    }
}
