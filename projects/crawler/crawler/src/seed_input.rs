use std::{
    fs,
    path::{Path, PathBuf},
};

use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_types::{CrawlError, Result, diagnostics as pd};
use url::Url;

/// Resolve crawl seed CLI input into one or more URL strings.
///
/// HTTP/HTTPS/`file://` inputs are treated as a single seed. Existing regular
/// files are read as newline-delimited seed lists (`#` comments and blank lines
/// are ignored). Other paths are normalized to a `file://` URL for a single
/// local seed.
pub fn resolve_crawl_seed_specs(input: &str) -> Result<Vec<String>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::REQUEST_INVALID),
            DiagnosticSeverity::Error,
            pd::origin_crawl(),
            Message::new("crawler.diagnostic.seedEmpty").with_fallback("crawl seed input is empty"),
        );
        return Err(CrawlError::from(diag));
    }
    if looks_like_url(trimmed) {
        return Ok(vec![trimmed.to_string()]);
    }
    let path = Path::new(trimmed);
    if path.is_file() {
        return read_seed_file(path);
    }
    Ok(vec![normalize_local_or_url_seed(trimmed)?])
}

fn looks_like_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://") || value.starts_with("file://")
}

fn read_seed_file(path: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(path).map_err(|error| {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::REQUEST_INVALID),
            DiagnosticSeverity::Error,
            pd::origin_crawl(),
            Message::new("crawler.diagnostic.seedFileRead")
                .with_arg("path", MessageArg::Text(path.display().to_string()))
                .with_arg("error", MessageArg::Text(error.to_string()))
                .with_fallback(format!("read seed file `{}`: {error}", path.display())),
        );
        CrawlError::from(diag)
    })?;
    let mut seeds = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let seed = if looks_like_url(trimmed) {
            trimmed.to_string()
        }
        else {
            normalize_local_or_url_seed(trimmed).map_err(|error| {
                let diag = boost_diagnostic::Diagnostic::new(
                    pd::code(pd::REQUEST_INVALID),
                    DiagnosticSeverity::Error,
                    pd::origin_crawl(),
                    Message::new("crawler.diagnostic.seedFileLine")
                        .with_arg("path", MessageArg::Text(path.display().to_string()))
                        .with_arg("line", MessageArg::U64((index + 1) as u64))
                        .with_arg("error", MessageArg::Text(error.to_string()))
                        .with_fallback(format!("seed file `{}` line {}: {error}", path.display(), index + 1)),
                );
                CrawlError::from(diag)
            })?
        };
        seeds.push(seed);
    }
    if seeds.is_empty() {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::REQUEST_INVALID),
            DiagnosticSeverity::Error,
            pd::origin_crawl(),
            Message::new("crawler.diagnostic.seedFileEmpty")
                .with_arg("path", MessageArg::Text(path.display().to_string()))
                .with_fallback(format!("seed file `{}` contains no usable seeds", path.display())),
        );
        return Err(CrawlError::from(diag));
    }
    Ok(seeds)
}

fn normalize_local_or_url_seed(seed: &str) -> Result<String> {
    if looks_like_url(seed) {
        return Ok(seed.to_string());
    }
    let path = PathBuf::from(seed);
    let url = Url::from_file_path(path).map_err(|_| {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::URL_INVALID),
            DiagnosticSeverity::Error,
            pd::origin_crawl(),
            Message::new("crawler.diagnostic.seedPath")
                .with_arg("seed", MessageArg::Text(seed.to_string()))
                .with_fallback(format!("invalid crawl seed path or URL `{seed}`")),
        );
        CrawlError::from(diag)
    })?;
    Ok(url.to_string())
}
