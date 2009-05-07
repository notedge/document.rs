use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use boost_diagnostic::{DiagnosticCause, DiagnosticSeverity, Message, MessageArg};
use crawler_types::{CrawlError, ExtractResult, Result, diagnostics as pd};
use document_formats::export::markdown_project::{MarkdownProject, export_markdown_project};
use document_ir::DocumentGraph;

/// Published Markdown project layout for a single extracted page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedPageProject {
    /// Output directory root.
    pub output_dir: PathBuf,
    /// Written `index.md` path.
    pub index_path: PathBuf,
    /// Written asset paths relative to `output_dir`.
    pub asset_paths: Vec<String>,
    /// Written chapter paths relative to `output_dir`.
    pub chapter_paths: Vec<String>,
    /// Written `panduck.report.json` path for the page export manifest.
    pub report_path: PathBuf,
}

/// Writes a single extracted HTML page to the shared Markdown project directory contract.
pub fn publish_extracted_page(
    output_dir: impl AsRef<Path>,
    label: &str,
    extract: &ExtractResult,
) -> Result<PublishedPageProject> {
    let document = extract.document.as_ref().ok_or_else(|| {
        let diag = boost_diagnostic::Diagnostic::new(
            pd::code(pd::EXTRACT_PARSE_FAILED),
            DiagnosticSeverity::Error,
            pd::origin_extract(),
            Message::new("crawler.diagnostic.missingDocumentGraphPublish")
                .with_fallback("missing document graph for page publish".to_string()),
        );
        CrawlError::from(diag)
    })?;
    let project = export_markdown_project(document).map_err(map_format_error)?;
    let report_json = build_page_report_json(label, extract, document, &project);
    publish_markdown_project(output_dir.as_ref(), &project, &report_json)
}

fn build_page_report_json(label: &str, extract: &ExtractResult, graph: &DocumentGraph, project: &MarkdownProject) -> String {
    let published_assets: Vec<String> = project.assets.iter().map(|asset| asset.relative_path.clone()).collect();
    let published_chapters: Vec<String> = project.chapters.iter().map(|chapter| chapter.relative_path.clone()).collect();
    let mut outputs = vec![
        serde_json::json!({ "path": "index.md", "format": "markdown", "published": true }),
        serde_json::json!({
            "path": "assets/",
            "format": "assets",
            "published_asset_count": published_assets.len()
        }),
    ];
    for chapter in &published_chapters {
        outputs.push(serde_json::json!({ "path": chapter, "format": "markdown", "published": true }));
    }
    outputs.push(serde_json::json!({ "path": "panduck.report.json", "format": "report", "published": true }));
    serde_json::json!({
        "schema_version": "panduck.report/v1",
        "operation": "extract_page",
        "status": if graph.coverage.complete { "success" } else { "success_with_loss" },
        "inputs": [{ "path": label, "format": "html", "source_url": extract.source_url.to_string() }],
        "pipeline": {
            "reader": extract.extractor,
            "writer": "markdown-project",
            "stages": ["fetch", "extract", "ir", "write", "materialize"]
        },
        "coverage": graph.coverage,
        "losses": graph.coverage.loss,
        "outputs": outputs,
        "unresolved_assets": project.unresolved_asset_sources,
    })
    .to_string()
}

fn publish_markdown_project(output_dir: &Path, project: &MarkdownProject, report_json: &str) -> Result<PublishedPageProject> {
    // Stage into a sibling temp dir first so any mid-flow failure (bad
    // asset path, disk full, …) does not leave `output_dir` half-published.
    let staging_dir = staging_project_dir_path(output_dir);
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir).map_err(map_io_error)?;
    }
    fs::create_dir_all(&staging_dir).map_err(map_io_error)?;
    let assets_dir = staging_dir.join("assets");
    fs::create_dir_all(&assets_dir).map_err(map_io_error)?;

    let index_path = staging_dir.join("index.md");
    publish_text(&index_path, &project.index_markdown)?;

    let mut chapter_paths = Vec::with_capacity(project.chapters.len());
    for chapter in &project.chapters {
        let chapter_path = staging_dir.join(&chapter.relative_path);
        if let Some(parent) = chapter_path.parent() {
            fs::create_dir_all(parent).map_err(map_io_error)?;
        }
        publish_text(&chapter_path, &chapter.markdown)?;
        chapter_paths.push(chapter.relative_path.clone());
    }

    let mut asset_paths = Vec::with_capacity(project.assets.len());
    for asset in &project.assets {
        let file_name = asset.relative_path.strip_prefix("assets/").ok_or_else(|| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::REQUEST_INVALID),
                DiagnosticSeverity::Error,
                pd::origin_extract(),
                Message::new("crawler.diagnostic.assetPathInvalid")
                    .with_arg("path", MessageArg::Text(asset.relative_path.clone()))
                    .with_fallback(format!("asset path must start with assets/: {}", asset.relative_path)),
            );
            CrawlError::from(diag)
        })?;
        let path = assets_dir.join(file_name);
        publish_bytes(&path, &asset.bytes)?;
        asset_paths.push(asset.relative_path.clone());
    }

    let report_path = staging_dir.join("panduck.report.json");
    publish_text(&report_path, report_json)?;

    // Whole-directory atomic swap.
    let result = replace_project_dir(&staging_dir, output_dir);
    let _ = fs::remove_dir_all(&staging_dir);
    result?;

    Ok(PublishedPageProject {
        output_dir: output_dir.to_path_buf(),
        index_path: output_dir.join("index.md"),
        chapter_paths,
        asset_paths,
        report_path: output_dir.join("panduck.report.json"),
    })
}

fn staging_project_dir_path(output_dir: &Path) -> PathBuf {
    let file_name = output_dir.file_name().and_then(|n| n.to_str()).unwrap_or("output");
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    parent.join(format!(".{}.{}.{}.staging", file_name, std::process::id(), nanos))
}

fn replace_project_dir(staging_dir: &Path, output_dir: &Path) -> Result<()> {
    match fs::rename(staging_dir, output_dir) {
        Ok(()) => Ok(()),
        Err(_) => {
            if output_dir.exists() {
                fs::remove_dir_all(output_dir).map_err(map_io_error)?;
            }
            fs::rename(staging_dir, output_dir).map_err(map_io_error)?;
            Ok(())
        }
    }
}

fn publish_text(path: &Path, content: &str) -> Result<()> {
    publish_bytes(path, content.as_bytes())
}

fn publish_bytes(path: &Path, content: &[u8]) -> Result<()> {
    publish_bytes_atomic(path, content).map_err(map_io_error)
}

fn publish_bytes_atomic(path: &Path, content: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "destination has no parent directory"))?;
    if !parent.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "destination parent directory does not exist"));
    }

    let staging_path = staging_path(parent, path)?;
    {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&staging_path)?;
        file.write_all(content)?;
        file.sync_all()?;
    }

    replace_path(&staging_path, path)?;
    sync_parent(path)?;
    Ok(())
}

fn staging_path(parent: &Path, target: &Path) -> io::Result<PathBuf> {
    let file_name =
        target.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no file name"))?;
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_nanos()).unwrap_or(0);
    Ok(parent.join(format!(".{}.{}.{}.tmp", file_name.to_string_lossy(), std::process::id(), nanos)))
}

fn replace_path(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(to)?;
            fs::rename(from, to)
        }
        Err(error) => Err(error),
    }
}

fn sync_parent(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "destination has no parent directory"))?;
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn map_format_error(error: document_formats::FormatError) -> CrawlError {
    // Mirror the crawler-extract convention: use the authoritative
    // `notedown.format.*` Diagnostic from `from_format_error` as the primary
    // wire code (so consumers key on stable Notedown codes), and add a
    // Crawler-origin wrapper with its own context code alongside.
    let notedown_diag = document_formats::from_format_error(&error);
    let wrapper = boost_diagnostic::Diagnostic::new(
        pd::code(pd::EXTRACT_PARSE_FAILED),
        DiagnosticSeverity::Error,
        pd::origin_extract(),
        Message::new("crawler.diagnostic.formatError")
            .with_arg("error", MessageArg::Text(error.to_string()))
            .with_arg("notedown_code", MessageArg::Text(notedown_diag.code().as_str().to_string()))
            .with_fallback(format!("format error ({}) at Crawler publish layer: {}", notedown_diag.code().as_str(), error)),
    );
    // Wrap the Notedown diagnostic as cause inside the Crawler wrapper so
    // consumers can walk both the Crawler layer and the original Notedown
    // layer code.
    let cause = DiagnosticCause::new(notedown_diag.code().clone(), notedown_diag.message().clone());
    let combined = wrapper.with_cause(cause);
    CrawlError::from(combined)
}

fn map_io_error(error: io::Error) -> CrawlError {
    let diag = boost_diagnostic::Diagnostic::new(
        pd::code(pd::CACHE_WRITE_FAILED),
        DiagnosticSeverity::Error,
        pd::origin_store(),
        Message::new("crawler.diagnostic.ioError")
            .with_arg("error", MessageArg::Text(error.to_string()))
            .with_arg("kind", MessageArg::Text(format!("{:?}", error.kind())))
            .with_fallback(format!("I/O error: {error}")),
    );
    CrawlError::from(diag)
}
