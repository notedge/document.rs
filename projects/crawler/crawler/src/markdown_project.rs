use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use boost_diagnostic::{DiagnosticCause, DiagnosticSeverity, Message, MessageArg};
use crawler_types::{CrawlError, ExtractResult, Result, diagnostics as pd};
use document_diagnostic::{DiagnosticEnvelope, diagnostics_from_graph, loss_severity};
use document_formats::export::markdown_project::{MarkdownProject, export_markdown_project};
use document_ir::DocumentGraph;

/// Project outputs owned by the publish contract: chapters and assets must not
/// shadow them.
const RESERVED_PROJECT_PATHS: [&str; 2] = ["index.md", "panduck.report.json"];

/// A sibling lock older than this is treated as left behind by a dead publisher.
const LOCK_STALE_AFTER: Duration = Duration::from_secs(600);

/// In-process publish guards, keyed by normalized output directory. Publishes
/// that target the same resolved directory queue behind each other.
static PUBLISH_GUARDS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

/// Disambiguates sibling staging/backup names created within the same nanosecond.
static PUBLISH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
    let output_dir = output_dir.as_ref().to_path_buf();
    let guard = publish_guard(&output_dir);
    let _serialized = guard.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    publish_markdown_project(&output_dir, &project, &report_json)
}

fn build_page_report_json(label: &str, extract: &ExtractResult, graph: &DocumentGraph, project: &MarkdownProject) -> String {
    let published_assets: Vec<String> = project.assets.iter().map(|asset| asset.relative_path.clone()).collect();
    let published_chapters: Vec<String> = project.chapters.iter().map(|chapter| chapter.relative_path.clone()).collect();
    let diagnostics = DiagnosticEnvelope::from_set(&diagnostics_from_graph(graph));
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
        "schema_version": document_diagnostic::SCHEMA_REPORT,
        "operation": "extract_page",
        "status": if graph.coverage.complete { "success" } else { "success_with_loss" },
        "inputs": [{ "path": label, "format": "html" }],
        "pipeline": {
            "reader": extract.extractor,
            "writer": "markdown-project",
            "stages": ["fetch", "extract", "ir", "write", "materialize"]
        },
        "coverage": graph.coverage,
        "losses": losses_with_spans(graph),
        "diagnostics": diagnostics,
        "outputs": outputs,
        "unresolved_assets": project.unresolved_asset_sources,
    })
    .to_string()
}

/// Serialize IR loss markers with the authored span each one points at, matching
/// the Panduck report envelope field-for-field.
fn losses_with_spans(graph: &DocumentGraph) -> serde_json::Value {
    let losses: Vec<serde_json::Value> = graph
        .coverage
        .loss
        .iter()
        .enumerate()
        .map(|(index, loss)| {
            let span = graph.loss_span(index);
            serde_json::json!({
                "code": loss.code,
                "message": loss.message,
                "status": loss.status,
                "severity": loss_severity(loss.status).to_string(),
                "span": match span {
                    Some(span) => serde_json::json!({ "start": span.start, "end": span.end }),
                    None => serde_json::Value::Null,
                },
            })
        })
        .collect();
    serde_json::Value::Array(losses)
}

fn publish_markdown_project(output_dir: &Path, project: &MarkdownProject, report_json: &str) -> Result<PublishedPageProject> {
    validate_project(project)?;
    let parent = output_dir.parent().filter(|dir| !dir.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(map_io_error)?;

    // Sibling lock across processes plus the in-process guard held by the
    // caller. A publish only claims the lock for the write-and-swap window.
    let lock_path = sibling_project_path(output_dir, "lock");
    acquire_publish_lock(&lock_path)?;

    // Stage into a sibling temp dir first so any mid-flow failure (bad
    // asset path, disk full, …) does not leave `output_dir` half-published.
    let staging_dir = sibling_project_path(output_dir, &format!("{}.staging", unique_suffix()));
    let result = stage_and_swap(&staging_dir, output_dir, project, report_json);
    let _ = fs::remove_dir_all(&staging_dir);
    let _ = fs::remove_file(&lock_path);
    result?;

    Ok(PublishedPageProject {
        output_dir: output_dir.to_path_buf(),
        index_path: output_dir.join("index.md"),
        chapter_paths: project.chapters.iter().map(|chapter| chapter.relative_path.clone()).collect(),
        asset_paths: project.assets.iter().map(|asset| asset.relative_path.clone()).collect(),
        report_path: output_dir.join("panduck.report.json"),
    })
}

fn stage_and_swap(staging_dir: &Path, output_dir: &Path, project: &MarkdownProject, report_json: &str) -> Result<()> {
    fs::create_dir(staging_dir).map_err(map_io_error)?;
    write_project_files(staging_dir, project, report_json)?;
    swap_project_dir(staging_dir, output_dir)
}

fn write_project_files(staging_dir: &Path, project: &MarkdownProject, report_json: &str) -> Result<()> {
    let assets_dir = staging_dir.join("assets");
    fs::create_dir_all(&assets_dir).map_err(map_io_error)?;

    publish_text(&staging_dir.join("index.md"), &project.index_markdown)?;

    for chapter in &project.chapters {
        let chapter_path = staging_dir.join(&chapter.relative_path);
        if let Some(parent) = chapter_path.parent() {
            fs::create_dir_all(parent).map_err(map_io_error)?;
        }
        publish_text(&chapter_path, &chapter.markdown)?;
    }

    for asset in &project.assets {
        publish_bytes(&staging_dir.join(&asset.relative_path), &asset.bytes)?;
    }

    publish_text(&staging_dir.join("panduck.report.json"), report_json)
}

/// Swaps the staged layout into place, keeping the previous output readable if
/// the swap itself fails.
fn swap_project_dir(staging_dir: &Path, output_dir: &Path) -> Result<()> {
    let backup_dir = move_project_aside(output_dir)?;
    match fs::rename(staging_dir, output_dir) {
        Ok(()) => {
            if let Some(backup_dir) = backup_dir {
                let _ = fs::remove_dir_all(backup_dir);
            }
            Ok(())
        }
        Err(error) => {
            if let Some(backup_dir) = backup_dir {
                let _ = fs::rename(backup_dir, output_dir);
            }
            Err(map_io_error(error))
        }
    }
}

/// Renames the existing output aside, returning the backup path when one existed.
fn move_project_aside(output_dir: &Path) -> Result<Option<PathBuf>> {
    if !output_dir.exists() {
        return Ok(None);
    }
    let backup_dir = sibling_project_path(output_dir, &format!("{}.backup", unique_suffix()));
    fs::rename(output_dir, &backup_dir).map_err(map_io_error)?;
    Ok(Some(backup_dir))
}

fn sibling_project_path(output_dir: &Path, suffix: &str) -> PathBuf {
    let file_name = output_dir.file_name().and_then(|name| name.to_str()).unwrap_or("output");
    let parent = output_dir.parent().filter(|dir| !dir.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    parent.join(format!(".{file_name}.{suffix}"))
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_nanos()).unwrap_or(0);
    let sequence = PUBLISH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{}.{}.{}", std::process::id(), nanos, sequence)
}

fn publish_guard(output_dir: &Path) -> Arc<Mutex<()>> {
    let key = project_path_key_of(&normalized_output_dir(output_dir));
    let registry = PUBLISH_GUARDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guards = registry.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guards.entry(key).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
}

fn normalized_output_dir(output_dir: &Path) -> String {
    let absolute = std::path::absolute(output_dir).unwrap_or_else(|_| output_dir.to_path_buf());
    project_path_key_of(&absolute.to_string_lossy().replace('\\', "/"))
}

/// Claims the sibling lock. A lock left by a dead publisher is reclaimed once.
fn acquire_publish_lock(lock_path: &Path) -> Result<()> {
    if try_write_lock(lock_path)? {
        return Ok(());
    }
    if is_stale_lock(lock_path) {
        let _ = fs::remove_file(lock_path);
        if try_write_lock(lock_path)? {
            return Ok(());
        }
    }
    let diag = boost_diagnostic::Diagnostic::new(
        pd::code(pd::PUBLISH_CONFLICT),
        DiagnosticSeverity::Error,
        pd::origin_store(),
        Message::new("crawler.diagnostic.publishConflict")
            .with_arg("lock", MessageArg::Text(lock_path.display().to_string()))
            .with_fallback(format!("output directory is locked by another publisher: {}", lock_path.display())),
    );
    Err(CrawlError::from(diag))
}

fn try_write_lock(lock_path: &Path) -> Result<bool> {
    let payload = format!("{{\"pid\":{},\"started_at_epoch\":{}}}", std::process::id(), now_epoch_seconds());
    let mut file = match OpenOptions::new().write(true).create_new(true).open(lock_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => return Err(map_io_error(error)),
    };
    if let Err(error) = file.write_all(payload.as_bytes()).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(lock_path);
        return Err(map_io_error(error));
    }
    Ok(true)
}

fn is_stale_lock(lock_path: &Path) -> bool {
    let Ok(payload) = fs::read_to_string(lock_path)
    else {
        return true;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&payload)
    else {
        return true;
    };
    let Some(started) = parsed.get("started_at_epoch").and_then(|value| value.as_u64())
    else {
        return true;
    };
    now_epoch_seconds().saturating_sub(started) > LOCK_STALE_AFTER.as_secs()
}

fn now_epoch_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs()).unwrap_or(0)
}

/// Rejects layouts that would overwrite each other or the fixed outputs before
/// anything is written: reserved names, duplicate paths (case-insensitive on
/// Windows), and file/directory collisions such as `x` next to `x/y.md`.
fn validate_project(project: &MarkdownProject) -> Result<()> {
    let mut files = HashSet::new();
    let mut directories = HashSet::new();
    for reserved in RESERVED_PROJECT_PATHS {
        files.insert(project_path_key_of(reserved));
    }
    for chapter in &project.chapters {
        reserve_project_path(&mut files, &mut directories, &chapter.relative_path, "chapter path")?;
    }
    for asset in &project.assets {
        let path = &asset.relative_path;
        if !path.starts_with("assets/") {
            return Err(invalid_layout(format!("asset path must start with assets/: {path}")));
        }
        reserve_project_path(&mut files, &mut directories, path, "asset path")?;
    }
    Ok(())
}

fn reserve_project_path(
    files: &mut HashSet<String>,
    directories: &mut HashSet<String>,
    project_path: &str,
    kind: &str,
) -> Result<()> {
    assert_safe_project_path(project_path)?;
    let segments: Vec<&str> = project_path.split(['/', '\\']).collect();
    let key = project_path_key_of(project_path);
    if RESERVED_PROJECT_PATHS.iter().any(|reserved| project_path_key_of(reserved) == key) {
        return Err(invalid_layout(format!("{kind} collides with a reserved output: {project_path}")));
    }
    if files.contains(&key) || directories.contains(&key) {
        return Err(invalid_layout(format!("{kind} is declared twice: {project_path}")));
    }
    for index in 1..segments.len() {
        let prefix = project_path_key_of(&segments[..index].join("/"));
        if files.contains(&prefix) {
            return Err(invalid_layout(format!("{kind} collides with another project path: {project_path}")));
        }
    }
    files.insert(key);
    for index in 1..segments.len() {
        directories.insert(project_path_key_of(&segments[..index].join("/")));
    }
    Ok(())
}

/// Rejects absolute paths, drive-letter paths, Windows alternate data streams,
/// control characters, `.`/`..`/empty segments and segments ending in a dot
/// or space (Windows rewrites those names silently).
fn assert_safe_project_path(project_path: &str) -> Result<()> {
    let escapes = || invalid_layout(format!("project path escapes the output directory: {project_path}"));
    if project_path.is_empty() || Path::new(project_path).is_absolute() || project_path.contains(':') {
        return Err(escapes());
    }
    if project_path.chars().any(|character| character.is_control()) {
        return Err(escapes());
    }
    let segments: Vec<&str> = project_path.split(['/', '\\']).collect();
    if segments.iter().any(|segment| segment.is_empty() || *segment == "." || *segment == "..") {
        return Err(escapes());
    }
    if segments.iter().any(|segment| segment.ends_with('.') || segment.ends_with(' ')) {
        return Err(escapes());
    }
    Ok(())
}

fn project_path_key_of(project_path: &str) -> String {
    let normalized = project_path.split(['/', '\\']).collect::<Vec<_>>().join("/");
    if cfg!(windows) { normalized.to_ascii_lowercase() } else { normalized }
}

fn invalid_layout(message: String) -> CrawlError {
    let diag = boost_diagnostic::Diagnostic::new(
        pd::code(pd::REQUEST_INVALID),
        DiagnosticSeverity::Error,
        pd::origin_extract(),
        Message::new("crawler.diagnostic.projectLayoutInvalid")
            .with_arg("detail", MessageArg::Text(message.clone()))
            .with_fallback(message),
    );
    CrawlError::from(diag)
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

#[cfg(test)]
mod tests {
    use super::*;
    use document_formats::export::markdown_project::{MarkdownProjectAsset, MarkdownProjectChapter};
    use document_ir::AssetId;

    fn project(index: &str) -> MarkdownProject {
        MarkdownProject {
            index_markdown: index.to_string(),
            chapters: Vec::new(),
            assets: Vec::new(),
            unresolved_asset_sources: Vec::new(),
            losses: Vec::new(),
        }
    }

    fn chapter(path: &str) -> MarkdownProjectChapter {
        MarkdownProjectChapter { relative_path: path.to_string(), markdown: "chapter".to_string() }
    }

    fn asset(path: &str) -> MarkdownProjectAsset {
        MarkdownProjectAsset { relative_path: path.to_string(), bytes: b"asset".to_vec(), asset_id: AssetId(1) }
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_nanos()).unwrap_or(0);
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }

    fn siblings(dir: &Path) -> Vec<String> {
        let parent = dir.parent().expect("parent");
        let name = dir.file_name().expect("name").to_string_lossy();
        let prefix = format!(".{name}.");
        fs::read_dir(parent)
            .expect("read parent")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|entry| entry.starts_with(&prefix))
            .collect()
    }

    #[test]
    fn duplicate_and_colliding_project_paths_are_rejected_before_writing() {
        let dir = temp_dir("crawler-publish-validate");
        publish_markdown_project(&dir, &project("good"), "{}").expect("baseline publish");

        let mut duplicate = project("bad");
        duplicate.chapters = vec![chapter("chapters/a.md"), chapter("chapters/a.md")];
        let error = publish_markdown_project(&dir, &duplicate, "{}").expect_err("duplicate chapter path must fail");
        assert_eq!(error.code(), Some(pd::REQUEST_INVALID));

        let mut collision = project("bad");
        collision.chapters = vec![chapter("chapters"), chapter("chapters/a.md")];
        let error = publish_markdown_project(&dir, &collision, "{}").expect_err("file/dir collision must fail");
        assert_eq!(error.code(), Some(pd::REQUEST_INVALID));

        let mut reserved = project("bad");
        reserved.chapters = vec![chapter("index.md")];
        let error = publish_markdown_project(&dir, &reserved, "{}").expect_err("reserved name must fail");
        assert_eq!(error.code(), Some(pd::REQUEST_INVALID));

        assert_eq!(fs::read_to_string(dir.join("index.md")).expect("read index"), "good");
        assert!(siblings(&dir).is_empty(), "rejected layouts must not leave residue");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unsafe_asset_paths_are_rejected() {
        for path in [
            "assets/../escape.png",
            "assets/a:b.png",
            "assets/a\u{0007}.png",
            "assets/a.png ",
            "/assets/a.png",
            "assets/a.png/",
        ] {
            let dir = temp_dir("crawler-publish-unsafe");
            let mut bad = project("bad");
            bad.assets = vec![asset(path)];
            let error = publish_markdown_project(&dir, &bad, "{}").expect_err("unsafe path must be rejected");
            assert_eq!(error.code(), Some(pd::REQUEST_INVALID), "path should be rejected: {path}");
            assert!(!dir.exists(), "rejected layout must not create the output directory");
        }

        let dir = temp_dir("crawler-publish-asset-prefix");
        let mut bad = project("bad");
        bad.assets = vec![asset("images/a.png")];
        let error = publish_markdown_project(&dir, &bad, "{}").expect_err("asset must live under assets/");
        assert_eq!(error.code(), Some(pd::REQUEST_INVALID));
        assert!(!dir.exists());
    }

    #[test]
    fn mid_flow_write_failure_keeps_previous_output() {
        let dir = temp_dir("crawler-publish-midflow");
        publish_markdown_project(&dir, &project("good"), "{}").expect("baseline publish");
        let before = fs::read_to_string(dir.join("index.md")).expect("read index");

        let mut bad = project("bad");
        bad.assets = vec![asset(&format!("assets/{}", "a".repeat(300)))];
        let error = publish_markdown_project(&dir, &bad, "{}").expect_err("over-long asset name must fail mid-flow");
        assert_eq!(error.code(), Some(pd::CACHE_WRITE_FAILED));
        assert_eq!(fs::read_to_string(dir.join("index.md")).expect("read index"), before);
        assert!(siblings(&dir).is_empty(), "failed publish must not leave staging/backup/lock residue");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn swap_failure_restores_previous_output() {
        let dir = temp_dir("crawler-publish-swap");
        fs::create_dir_all(&dir).expect("create output dir");
        fs::write(dir.join("index.md"), "old").expect("write old index");
        let missing_staging = dir.parent().expect("parent").join("missing-staging");

        let error = swap_project_dir(&missing_staging, &dir).expect_err("missing staging must fail the swap");
        assert_eq!(error.code(), Some(pd::CACHE_WRITE_FAILED));
        assert_eq!(fs::read_to_string(dir.join("index.md")).expect("read index"), "old");
        assert!(siblings(&dir).is_empty(), "restored swap must not leave a backup behind");

        let _ = fs::remove_dir_all(&dir);
    }
}
