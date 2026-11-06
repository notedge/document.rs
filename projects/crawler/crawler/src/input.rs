use std::path::Path;

use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_types::{BrowserSnapshot, CrawlError, FetchArtifact, Result, diagnostics as pd};

use crawler_browser::{load_snapshot_json, path_looks_like_snapshot};
use crawler_fetch::{FetchRequest, FileTransport, Transport};

/// Input format selector for extract and inspect commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    /// Local HTML or fetch artifact bytes via file transport.
    Html,
    /// JSON browser snapshot.
    Snapshot,
    /// Infer from file extension and content.
    Auto,
}

/// Loaded CLI input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadedInput {
    /// Fetch artifact from HTML or binary file.
    Artifact(FetchArtifact),
    /// Browser snapshot JSON.
    Snapshot(BrowserSnapshot),
}

/// Parse an input format string from CLI or N-API.
pub fn parse_input_format(from: Option<&str>) -> Result<InputFormat> {
    match from.unwrap_or("auto") {
        "html" => Ok(InputFormat::Html),
        "snapshot" => Ok(InputFormat::Snapshot),
        "auto" => Ok(InputFormat::Auto),
        other => {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::REQUEST_INVALID),
                DiagnosticSeverity::Error,
                pd::origin_crawl(),
                Message::new("crawler.diagnostic.inputFormatUnsupported")
                    .with_arg("format", MessageArg::Text(other.to_string()))
                    .with_fallback(format!("unsupported input format `{other}`")),
            );
            Err(CrawlError::from(diag))
        }
    }
}

/// Load extract or inspect input from a local path.
pub fn load_input(path: &str, format: InputFormat) -> Result<LoadedInput> {
    load_input_with_source(path, format, None)
}

/// Load extract or inspect input from a local path, optionally overriding the
/// page identity with the caller-supplied source URL.
///
/// A local HTML file is a stand-in for a page fetched from the web. When the
/// caller knows the page URL, relative asset and link targets inside the
/// document must resolve against that URL — not against the local `file://`
/// path. The override replaces the artifact's `request_url`/`final_url`
/// identity; the `cache_key` keeps the local path so cache lookups stay
/// per-file. Snapshots carry their own `final_url` from the browser session
/// and are never overridden.
pub fn load_input_with_source(path: &str, format: InputFormat, source_url: Option<&str>) -> Result<LoadedInput> {
    let mut loaded = load_input_impl(path, format)?;
    if let (LoadedInput::Artifact(artifact), Some(source_url)) = (&mut loaded, source_url) {
        let page_url = url::Url::parse(source_url).map_err(|error| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::URL_INVALID),
                DiagnosticSeverity::Error,
                pd::origin_crawl(),
                Message::new("crawler.diagnostic.sourceUrlInvalid")
                    .with_arg("url", MessageArg::Text(source_url.to_string()))
                    .with_fallback(format!("invalid source url `{source_url}`: {error}")),
            );
            CrawlError::from(diag)
        })?;
        artifact.request_url = page_url.clone();
        artifact.final_url = page_url;
    }
    Ok(loaded)
}

fn load_input_impl(path: &str, format: InputFormat) -> Result<LoadedInput> {
    let file_path = Path::new(path);
    let resolved = match format {
        InputFormat::Html => InputFormat::Html,
        InputFormat::Snapshot => InputFormat::Snapshot,
        InputFormat::Auto => {
            if path_looks_like_snapshot(file_path) {
                InputFormat::Snapshot
            }
            else {
                InputFormat::Html
            }
        }
    };

    match resolved {
        InputFormat::Snapshot => Ok(LoadedInput::Snapshot(load_snapshot_json(file_path)?)),
        InputFormat::Html => {
            let file_url = url::Url::from_file_path(file_path).map_err(|_| {
                let diag = boost_diagnostic::Diagnostic::new(
                    pd::code(pd::URL_INVALID),
                    DiagnosticSeverity::Error,
                    pd::origin_crawl(),
                    Message::new("crawler.diagnostic.inputPathInvalid")
                        .with_arg("path", MessageArg::Text(path.to_string()))
                        .with_fallback(format!("invalid input path `{path}`")),
                );
                CrawlError::from(diag)
            })?;
            let transport = FileTransport::new();
            let artifact = transport.fetch(&FetchRequest { url: file_url, headers: Default::default() })?;
            Ok(LoadedInput::Artifact(artifact))
        }
        InputFormat::Auto => unreachable!("auto resolved above"),
    }
}
