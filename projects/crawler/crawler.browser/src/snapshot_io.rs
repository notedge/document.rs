use std::{fs, path::Path};

use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_types::{BrowserSnapshot, CrawlError, Result, diagnostics as pd};

use crate::provider::FixtureBrowserProvider;

fn map_snapshot_io(action: &str, path: &Path, error: std::io::Error) -> CrawlError {
    let diag = boost_diagnostic::Diagnostic::new(
        pd::code(pd::BROWSER_UNAVAILABLE),
        DiagnosticSeverity::Error,
        pd::origin_browser(),
        Message::new("crawler.diagnostic.snapshotIo")
            .with_arg("action", MessageArg::Text(action.to_string()))
            .with_arg("path", MessageArg::Text(path.display().to_string()))
            .with_arg("error", MessageArg::Text(error.to_string()))
            .with_fallback(format!("{action} snapshot `{}`: {error}", path.display())),
    );
    CrawlError::from(diag)
}

fn map_snapshot_json(path: &Path, error: serde_json::Error) -> CrawlError {
    let diag = boost_diagnostic::Diagnostic::new(
        pd::code(pd::BROWSER_SNAPSHOT_INVALID),
        DiagnosticSeverity::Error,
        pd::origin_browser(),
        Message::new("crawler.diagnostic.snapshotJson")
            .with_arg("path", MessageArg::Text(path.display().to_string()))
            .with_arg("error", MessageArg::Text(error.to_string()))
            .with_fallback(format!("parse snapshot JSON `{}`: {error}", path.display())),
    );
    CrawlError::from(diag)
}

/// Load a `BrowserSnapshot` from JSON on disk.
pub fn load_snapshot_json(path: impl AsRef<Path>) -> Result<BrowserSnapshot> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|error| map_snapshot_io("read", path, error))?;
    serde_json::from_slice(&bytes).map_err(|error| map_snapshot_json(path, error))
}

/// Write a browser snapshot as JSON to disk.
pub fn save_snapshot_json(path: impl AsRef<Path>, snapshot: &BrowserSnapshot) -> Result<()> {
    let path = path.as_ref();
    let json = serde_json::to_string_pretty(snapshot).map_err(|error| {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::BROWSER_SNAPSHOT_INVALID),
            DiagnosticSeverity::Error,
            pd::origin_browser(),
            Message::new("crawler.diagnostic.snapshotSerialize")
                .with_arg("error", MessageArg::Text(error.to_string()))
                .with_fallback(format!("serialize snapshot JSON: {error}")),
        );
        CrawlError::from(diag)
    })?;
    fs::write(path, json).map_err(|error| map_snapshot_io("write", path, error))
}

/// Parse comma-separated browser fixture directories.
pub fn parse_browser_dirs(spec: &str) -> Vec<std::path::PathBuf> {
    spec.split(',').map(str::trim).filter(|part| !part.is_empty()).map(std::path::PathBuf::from).collect()
}

fn load_snapshots_from_dir_into(provider: &mut FixtureBrowserProvider, dir: &Path) -> Result<u32> {
    if !dir.is_dir() {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::BROWSER_UNAVAILABLE),
            DiagnosticSeverity::Error,
            pd::origin_browser(),
            Message::new("crawler.diagnostic.browserFixturesNotDir")
                .with_arg("dir", MessageArg::Text(dir.display().to_string()))
                .with_fallback(format!("browser fixtures path is not a directory `{}`", dir.display())),
        );
        return Err(CrawlError::from(diag));
    }

    let mut loaded = 0u32;
    for entry in fs::read_dir(dir).map_err(|error| map_snapshot_io("read browser fixtures dir", dir, error))? {
        let entry = entry.map_err(|error| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::BROWSER_UNAVAILABLE),
                DiagnosticSeverity::Warning,
                pd::origin_browser(),
                Message::new("crawler.diagnostic.browserFixturesEntry")
                    .with_arg("error", MessageArg::Text(error.to_string()))
                    .with_fallback(format!("read browser fixtures entry: {error}")),
            );
            CrawlError::from(diag)
        })?;
        let path = entry.path();
        if !path.is_file() || !path_looks_like_snapshot(&path) {
            continue;
        }
        let snapshot = load_snapshot_json(&path)?;
        provider.insert(snapshot);
        loaded += 1;
    }
    Ok(loaded)
}

/// Load fixture snapshots from multiple directories.
///
/// Later directories override earlier snapshots for the same URL.
pub fn load_fixture_provider_from_dirs(dirs: &[impl AsRef<Path>]) -> Result<FixtureBrowserProvider> {
    if dirs.is_empty() {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::BROWSER_UNAVAILABLE),
            DiagnosticSeverity::Error,
            pd::origin_browser(),
            Message::new("crawler.diagnostic.browserFixturesRequired")
                .with_fallback("at least one browser fixtures directory is required"),
        );
        return Err(CrawlError::from(diag));
    }
    let mut provider = FixtureBrowserProvider::new();
    let mut loaded = 0u32;
    for dir in dirs {
        loaded += load_snapshots_from_dir_into(&mut provider, dir.as_ref())?;
    }
    if loaded == 0 {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::BROWSER_UNAVAILABLE),
            DiagnosticSeverity::Error,
            pd::origin_browser(),
            Message::new("crawler.diagnostic.browserFixturesEmpty")
                .with_fallback("browser fixtures directories contain no snapshot JSON files"),
        );
        return Err(CrawlError::from(diag));
    }
    Ok(provider)
}

/// Load fixture snapshots from a directory into a browser provider.
pub fn load_fixture_provider_from_dir(dir: impl AsRef<Path>) -> Result<FixtureBrowserProvider> {
    let dir = dir.as_ref();
    let mut provider = FixtureBrowserProvider::new();
    let loaded = load_snapshots_from_dir_into(&mut provider, dir)?;
    if loaded == 0 {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::BROWSER_UNAVAILABLE),
            DiagnosticSeverity::Error,
            pd::origin_browser(),
            Message::new("crawler.diagnostic.browserFixturesDirEmpty")
                .with_arg("dir", MessageArg::Text(dir.display().to_string()))
                .with_fallback(format!("browser fixtures dir `{}` contains no snapshot JSON files", dir.display())),
        );
        return Err(CrawlError::from(diag));
    }
    Ok(provider)
}

/// Detect whether a path likely contains a browser snapshot.
pub fn path_looks_like_snapshot(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("json") || ext.eq_ignore_ascii_case("snapshot"))
        .unwrap_or(false)
}
